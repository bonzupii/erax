//! Web wasm shell: browser canvas + DOM input forwarding.
//!
//! The same presentation core (`crate::gui::core::Renderer`) compiled to
//! wasm32, driving a `<canvas>` via wgpu's WebGPU backend. DOM keyboard,
//! mouse, IME-composition, and resize events are forwarded as the same raw
//! input types the native shell produces (`KeyPress`, `MouseEvent`); the
//! protocol carries them to the server, where the backend owns all input
//! semantics (dumb passthrough, principle 12).
//!
//! Fonts: the shell fetches the configured font stack from the server
//! (`/font-stack` manifest, then each entry's bytes) and passes it to the
//! core in order. NO system fontdb scan — there is no fontdb on the web.
//!
//! Browser-reserved keys (Ctrl+W/N/Tab etc.) are documented per input model
//! in KEYBINDINGS.md; they are never silently swallowed and never faked as
//! parity — the browser owns them.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    BinaryType, Document, HtmlCanvasElement, HtmlTextAreaElement, KeyboardEvent, MessageEvent,
    MouseEvent as DomMouseEvent, WebSocket, Window,
};

use crate::core::input::{Key, KeyPress, MouseButton, MouseEvent, MouseEventKind};
use crate::gui::core::renderer::FrameOutcome;
use crate::terminal::display::{Cell, CellRole, ScreenBuffer};
use crate::web::protocol::{ClientFrame, CursorState, InputEvent, ServerCue, ServerFrame, PROTOCOL_VERSION};
use crate::config::cursor_style::CursorStyle;
use crate::gui::shell::startup::{StartupStage, startup_failure};

/// The web client's session state.
struct WebSession {
    ws: WebSocket,
    renderer: crate::gui::core::Renderer,
    screen: ScreenBuffer,
    token: String,
    canvas: HtmlCanvasElement,
    /// Hidden IME scratchpad: positioned at the backend cursor and focused
    /// so the OS IME opens its composition popup at the text cursor.
    /// Real input still travels via keydown protocol frames; this element
    /// only hosts composition.
    ime: HtmlTextAreaElement,
    /// Last cursor state from the server (position/visibility/shape).
    /// The shape is handed to the presentation core verbatim; x/y drive
    /// the IME anchor. Never inferred client-side.
    cursor: CursorState,
    /// Device pixel ratio at startup (CSS px ↔ physical px for the anchor).
    dpr: f64,
    /// Last Screen `seq` seen; a gap means a dropped frame (warn, don't die).
    last_seq: u64,
    /// A `requestAnimationFrame` repaint is already queued. The retry path
    /// repaints the current screen model when the surface skips a frame;
    /// at most one retry is ever pending.
    repaint_pending: bool,
    /// Consecutive repaint retries. A skipped frame is transient
    /// (surface timeout, occluded tab); the chain gives up after ~10 s
    /// of continuous skipping so a permanently broken surface cannot
    /// burn a render per animation frame forever.
    repaint_attempts: u32,
    /// Lazily-created WebAudio context for accessibility earcons.
    /// Browsers gate audio behind a user gesture, so this stays `None`
    /// until the first cue frame arrives; a suspended context drops
    /// that cue and resumes on the next one.
    audio: Option<web_sys::AudioContext>,
}

/// Probe WebGPU adapter availability WITHOUT touching the canvas.
///
/// A canvas that has ever yielded a `webgpu` context can never yield a
/// `webgl2` context afterward (one context type per canvas, permanently),
/// so the backend decision must precede any surface creation: probing
/// `navigator.gpu.requestAdapter()` first keeps the WebGL2 fallback on a
/// pristine canvas on browsers whose WebGPU API exists but has no adapter.
/// Uses `Reflect` rather than web-sys's typed `Navigator::gpu` (which
/// needs the `web_sys_unstable_apis` cfg); `requestAdapter` itself is
/// stable in every browser that ships WebGPU.
async fn probe_webgpu_adapter(window: &Window) -> bool {
    let navigator = window.navigator();
    let gpu = match js_sys::Reflect::get(&navigator, &JsValue::from_str("gpu")) {
        Ok(v) if !v.is_undefined() && !v.is_null() => v,
        _ => return false,
    };
    let request_adapter = match js_sys::Reflect::get(&gpu, &JsValue::from_str("requestAdapter"))
    {
        Ok(v) => match v.dyn_ref::<js_sys::Function>() {
            Some(f) => f.clone(),
            None => return false,
        },
        Err(_) => return false,
    };
    let promise: js_sys::Promise = match request_adapter.call0(&gpu) {
        Ok(v) => match v.dyn_into() {
            Ok(p) => p,
            Err(_) => return false,
        },
        Err(_) => return false,
    };
    match wasm_bindgen_futures::JsFuture::from(promise).await {
        Ok(adapter) => !adapter.is_null() && !adapter.is_undefined(),
        Err(_) => false,
    }
}

/// Build a pristine `<canvas>` replacing `old` in the DOM.
///
/// Used when the WebGPU attempt may have poisoned the original canvas's
/// context type: the replacement has never yielded any context, so a
/// subsequent `webgl2` acquisition cannot fail with "canvas already in
/// use". Sizing and id are carried over so the page layout is unchanged.
fn fresh_canvas(
    document: &Document,
    old: &HtmlCanvasElement,
    width: u32,
    height: u32,
) -> Result<HtmlCanvasElement, JsValue> {
    let fresh: HtmlCanvasElement = document
        .create_element("canvas")?
        .dyn_into()
        .map_err(|_| JsValue::from_str("fresh element is not a <canvas>"))?;
    fresh.set_id(&old.id());
    fresh.set_width(width);
    fresh.set_height(height);
    if let Some(style) = old.get_attribute("style") {
        fresh
            .set_attribute("style", &style)
            .map_err(|e| js_error(&e))?;
    }
    let parent = old
        .parent_node()
        .ok_or_else(|| JsValue::from_str("canvas has no parent element"))?;
    parent
        .replace_child(&fresh, old)
        .map_err(|e| js_error(&e))?;
    Ok(fresh)
}

/// Entry point called from the JS glue (`index.html`).
///
/// Startup is a named stage machine (`shell::startup`): every stage
/// transition updates the page's `#status` line and the console, and every
/// failure names its stage in both places and in the promise rejection.
/// The client never sits in an unnamed state.
///
/// Stages: fetch-font → create-renderer → connect-socket → wire-events →
/// running (waiting for the server handshake; `AuthOk` flips the status
/// to `connected`).
///
/// - `canvas_id`: the `<canvas>` element id.
/// - `ws_url`: e.g. `ws://127.0.0.1:8080/ws`.
/// - `token`: the per-session token from the URL (`?token=...`).
/// - `font_stack_url`: where to fetch the configured font-stack manifest
///   (e.g. `/font-stack`).
#[wasm_bindgen]
pub async fn erax_start(
    canvas_id: &str,
    ws_url: &str,
    token: &str,
    font_stack_url: &str,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let mut canvas: HtmlCanvasElement = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| JsValue::from_str("canvas element not found"))?
        .dyn_into()
        .map_err(|_| JsValue::from_str("element is not a <canvas>"))?;

    // -- fetch-font --
    set_status(
        &document,
        "startup: fetch-font: requesting the configured font stack…",
    );
    let fonts = fetch_font_stack(&window, font_stack_url)
        .await
        .map_err(|e| fail(&document, StartupStage::FetchFont, &js_error(&e)))?;

    // Size the canvas to the window (physical pixels).
    let dpr = window.device_pixel_ratio();
    let css_w = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(1200.0);
    let css_h = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(800.0);
    let phys_w = (css_w * dpr) as u32;
    let phys_h = (css_h * dpr) as u32;
    canvas.set_width(phys_w);
    canvas.set_height(phys_h);

    // -- create-renderer --
    // The core constructor is async: adapter/device acquisition resolves
    // through the browser event loop, which a blocking wait would deadlock
    // (the page used to sit at "starting…" forever).
    //
    // Backend selection probes BEFORE touching the canvas: a canvas that
    // has ever yielded a `webgpu` context can never yield `webgl2`
    // afterward, so attempting WebGPU on the page canvas first would
    // poison the WebGL2 fallback on browsers whose WebGPU API exists but
    // has no adapter. The probe never creates a surface.
    set_status(
        &document,
        "startup: create-renderer: probing graphics backends…",
    );
    let use_webgpu = probe_webgpu_adapter(&window).await;
    web_sys::console::log_1(
        &format!("startup: create-renderer: WebGPU adapter probe: {use_webgpu}").into(),
    );

    // The stack is cheap to clone (font bytes are `Arc`); the WebGL2
    // fallback reuses it without re-fetching.
    let fonts_fallback = fonts.clone();
    let renderer = if use_webgpu {
        set_status(
            &document,
            "startup: create-renderer: requesting a WebGPU adapter…",
        );
        match crate::gui::core::Renderer::new(crate::gui::core::RendererParams {
            target: wgpu::SurfaceTarget::Canvas(canvas.clone()),
            width: phys_w,
            height: phys_h,
            fonts,
            font_size: crate::config::font::FONT_SIZE_PX,
            scale_factor: dpr,
            backends: wgpu::Backends::BROWSER_WEBGPU,
        })
        .await
        {
            Ok(r) => {
                web_sys::console::log_1(
                    &"startup: create-renderer: WebGPU adapter acquired".into(),
                );
                r
            }
            Err(webgpu_err) => {
                // The failed WebGPU attempt may have poisoned the canvas's
                // context type: retry WebGL2 on a fresh canvas element.
                let note =
                    format!("WebGPU failed ({webgpu_err}); retrying WebGL2 on a fresh canvas…");
                web_sys::console::warn_1(&note.clone().into());
                set_status(&document, &format!("startup: create-renderer: {note}"));
                canvas = fresh_canvas(&document, &canvas, phys_w, phys_h)
                    .map_err(|e| fail(&document, StartupStage::CreateRenderer, &js_error(&e)))?;
                crate::gui::core::Renderer::new(crate::gui::core::RendererParams {
                    target: wgpu::SurfaceTarget::Canvas(canvas.clone()),
                    width: phys_w,
                    height: phys_h,
                    fonts: fonts_fallback,
                    font_size: crate::config::font::FONT_SIZE_PX,
                    scale_factor: dpr,
                    backends: wgpu::Backends::GL,
                })
                .await
                .map_err(|gl_err| {
                    fail(
                        &document,
                        StartupStage::CreateRenderer,
                        &format!("WebGPU failed: {webgpu_err}; WebGL2 failed: {gl_err}"),
                    )
                })?
            }
        }
    } else {
        // No WebGPU adapter: the canvas is still pristine, so WebGL2 gets
        // a first-class context acquisition, not a poisoned retry.
        set_status(
            &document,
            "startup: create-renderer: no WebGPU adapter; using WebGL2…",
        );
        crate::gui::core::Renderer::new(crate::gui::core::RendererParams {
            target: wgpu::SurfaceTarget::Canvas(canvas.clone()),
            width: phys_w,
            height: phys_h,
            fonts,
            font_size: crate::config::font::FONT_SIZE_PX,
            scale_factor: dpr,
            backends: wgpu::Backends::GL,
        })
        .await
        .map_err(|gl_err| {
            fail(
                &document,
                StartupStage::CreateRenderer,
                &format!("WebGL2 failed: {gl_err}"),
            )
        })?
    };

    let (cols, rows) = renderer.grid_size();

    // -- connect-socket --
    set_status(
        &document,
        "startup: connect-socket: opening the WebSocket…",
    );
    let ws = connect_websocket(ws_url)
        .map_err(|e| fail(&document, StartupStage::ConnectSocket, &js_error(&e)))?;

    // -- wire-events --
    set_status(
        &document,
        "startup: wire-events: attaching input listeners…",
    );
    let ime = create_ime_textarea(&document)
        .map_err(|e| fail(&document, StartupStage::WireEvents, &js_error(&e)))?;
    let session = Rc::new(RefCell::new(WebSession {
        ws,
        renderer,
        screen: ScreenBuffer::new(cols as u16, rows as u16),
        token: token.to_string(),
        canvas: canvas.clone(),
        ime: ime.clone(),
        cursor: CursorState {
            x: 0,
            y: 0,
            visible: false,
            role: CellRole::EditorText,
            style: CursorStyle::Default,
        },
        dpr,
        last_seq: 0,
        repaint_pending: false,
        repaint_attempts: 0,
        audio: None,
    }));

    // Wire DOM events → protocol input.
    wire_keyboard(&document, session.clone())
        .map_err(|e| fail(&document, StartupStage::WireEvents, &js_error(&e)))?;
    wire_mouse(&canvas, session.clone())
        .map_err(|e| fail(&document, StartupStage::WireEvents, &js_error(&e)))?;
    wire_ime(&ime, session.clone())
        .map_err(|e| fail(&document, StartupStage::WireEvents, &js_error(&e)))?;
    wire_resize(&window, session.clone())
        .map_err(|e| fail(&document, StartupStage::WireEvents, &js_error(&e)))?;
    wire_visibility(&document, session.clone())
        .map_err(|e| fail(&document, StartupStage::WireEvents, &js_error(&e)))?;
    wire_socket(&document, session.clone())
        .map_err(|e| fail(&document, StartupStage::WireEvents, &js_error(&e)))?;

    // Focus the IME scratchpad so keyboard + IME work without a first
    // click (best effort; the canvas click path re-focuses anyway).
    let _ = ime.focus();

    // -- running --
    set_status(
        &document,
        "startup: running: waiting for the server handshake…",
    );
    Ok(())
}

/// A `JsValue` rejection reason as a readable string.
fn js_error(e: &JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}

/// Write `text` to the page's `#status` element and the console: every
/// startup stage transition is visible in both.
fn set_status(document: &Document, text: &str) {
    if let Some(el) = document.get_element_by_id("status") {
        el.set_text_content(Some(text));
    }
    web_sys::console::log_1(&text.into());
}

/// Build the rejection for a failed stage: status line, console error,
/// and the promise rejection the JS glue renders.
fn fail(document: &Document, stage: StartupStage, detail: &str) -> JsValue {
    let msg = startup_failure(stage, detail);
    if let Some(el) = document.get_element_by_id("status") {
        el.set_text_content(Some(&msg));
    }
    web_sys::console::error_1(&msg.clone().into());
    JsValue::from_str(&msg)
}

/// Create the hidden IME scratchpad textarea.
///
/// Absolutely positioned, 1px, transparent, pointer-transparent: it never
/// intercepts clicks (those go to the canvas) but it can hold DOM focus so
/// the OS IME activates and positions its composition popup where we put
/// it — at the backend cursor, repositioned on every Screen frame.
fn create_ime_textarea(document: &Document) -> Result<HtmlTextAreaElement, JsValue> {
    use web_sys::HtmlTextAreaElement;
    let ime: HtmlTextAreaElement = document.create_element("textarea")?.dyn_into()?;
    ime.set_attribute("aria-label", "editor input")?;
    ime.set_attribute("autocomplete", "off")?;
    ime.set_attribute("autocorrect", "off")?;
    ime.set_attribute("autocapitalize", "off")?;
    ime.set_attribute("spellcheck", "false")?;
    let style = ime.style();
    for (k, v) in [
        ("position", "absolute"),
        ("width", "1px"),
        ("height", "1px"),
        ("opacity", "0"),
        ("border", "none"),
        ("outline", "none"),
        ("resize", "none"),
        ("overflow", "hidden"),
        ("background", "transparent"),
        ("pointer-events", "none"),
        ("z-index", "10"),
        ("left", "-100px"),
        ("top", "-100px"),
    ] {
        style.set_property(k, v)?;
    }
    document
        .body()
        .ok_or("no document body")?
        .append_child(&ime)?;
    Ok(ime)
}

/// One entry of the server's `/font-stack` manifest.
#[derive(serde::Deserialize)]
struct FontManifestEntry {
    family: String,
    url: String,
}

/// Fetch the configured font stack from the server.
///
/// The manifest (`/font-stack`) lists the stack in fallback order; each
/// entry's bytes are fetched from its URL (same origin). The browser gets
/// the same ordered stack the native shell resolves locally, so shaping
/// fallback behaves identically on both.
async fn fetch_font_stack(
    window: &Window,
    manifest_url: &str,
) -> Result<Vec<crate::gui::core::font_manager::FontSource>, JsValue> {
    let origin = window
        .location()
        .origin()
        .map_err(|e| JsValue::from_str(&format!("font stack: no origin: {e:?}")))?;
    let manifest_bytes = fetch_bytes(window, manifest_url, "font stack manifest").await?;
    let entries: Vec<FontManifestEntry> = serde_json::from_slice(&manifest_bytes).map_err(|e| {
        JsValue::from_str(&format!("font stack: bad manifest: {e}"))
    })?;
    if entries.is_empty() {
        return Err(JsValue::from_str(
            "font stack: server has no fonts configured",
        ));
    }
    let mut fonts = Vec::with_capacity(entries.len());
    for entry in &entries {
        let url = format!("{origin}{}", entry.url);
        let bytes =
            fetch_bytes(window, &url, &format!("font {}", entry.family)).await?;
        fonts.push(crate::gui::core::font_manager::FontSource {
            bytes: Arc::new(bytes),
            family: Some(entry.family.clone()),
        });
    }
    Ok(fonts)
}

/// Fetch raw bytes from the server; HTTP errors are errors (fetch only
/// rejects on network failure).
async fn fetch_bytes(window: &Window, url: &str, what: &str) -> Result<Vec<u8>, JsValue> {
    use wasm_bindgen_futures::JsFuture;
    let resp = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| JsValue::from_str(&format!("{what} fetch failed: {e:?}")))?;
    let resp: web_sys::Response = resp.dyn_into()?;
    if !resp.ok() {
        return Err(JsValue::from_str(&format!(
            "{what} fetch: HTTP {}",
            resp.status()
        )));
    }
    let buf = JsFuture::from(resp.array_buffer()?)
        .await
        .map_err(|e| JsValue::from_str(&format!("{what} read failed: {e:?}")))?;
    let bytes = js_sys::Uint8Array::new(&buf).to_vec();
    if bytes.is_empty() {
        return Err(JsValue::from_str(&format!("{what}: empty body")));
    }
    Ok(bytes)
}

fn connect_websocket(url: &str) -> Result<WebSocket, JsValue> {
    let ws = WebSocket::new(url).map_err(|e| JsValue::from_str(&format!("ws: {e:?}")))?;
    ws.set_binary_type(BinaryType::Arraybuffer);
    Ok(ws)
}

/// Send a client frame.
fn send(session: &Rc<RefCell<WebSession>>, frame: &ClientFrame) {
    let text = crate::web::protocol::encode(frame);
    session.borrow().ws.send_with_str(&text).ok();
}

/// WebSocket message handling: auth, then screen streaming.
///
/// Connection-level failures (refused, dropped) surface on the page's
/// `#status` line: a socket that never opens must not leave the client in
/// a silent "waiting" state.
fn wire_socket(document: &Document, session: Rc<RefCell<WebSession>>) -> Result<(), JsValue> {
    let onmessage = {
        let session = session.clone();
        Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let data = event.data();
            let text = match data.as_string() {
                Some(t) => t,
                None => return,
            };
            let frame: ServerFrame = match crate::web::protocol::decode(&text) {
                Ok(f) => f,
                Err(_) => return,
            };
            handle_server_frame(&session, frame);
        })
    };
    session
        .borrow()
        .ws
        .set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    // Auth on open.
    let onopen = {
        let session = session.clone();
        Closure::<dyn FnMut()>::new(move || {
            let token = session.borrow().token.clone();
            send(&session, &ClientFrame::Auth { token });
            // Tell the server our grid size.
            let (w, h) = {
                let s = session.borrow();
                (s.screen.width, s.screen.height)
            };
            send(&session, &ClientFrame::Resize { width: w, height: h });
        })
    };
    session
        .borrow()
        .ws
        .set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    // A refused or dropped connection: say so on the page and console.
    let onerror = {
        let document = document.clone();
        Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let text = "startup: running: WebSocket error (connection refused or lost; see console)";
            web_sys::console::error_1(&text.into());
            set_status(&document, text);
        })
    };
    session
        .borrow()
        .ws
        .set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    let onclose = {
        let document = document.clone();
        Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |event: web_sys::CloseEvent| {
            let text = format!(
                "startup: running: WebSocket closed (code {}, reason: {})",
                event.code(),
                event.reason()
            );
            web_sys::console::warn_1(&text.clone().into());
            set_status(&document, &text);
        })
    };
    session
        .borrow()
        .ws
        .set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();
    Ok(())
}

/// Map a cue event to its earcon: (frequency Hz, duration ms).
///
/// Every cue kind is distinct by construction (no two share a
/// frequency), and modes are distinct from each other so a vim mode
/// change is identifiable by ear. Errors are low and long; they must
/// be unmistakable.
fn earcon_for_cue(cue: &ServerCue) -> (f32, u32) {
    match cue {
        ServerCue::ModeEntered { mode } => match mode.as_str() {
            "insert" => (440.0, 90),
            "normal" => (660.0, 70),
            "visual" => (550.0, 100),
            "operator_pending" => (330.0, 130),
            "command_line" => (880.0, 70),
            _ => (494.0, 80),
        },
        ServerCue::Error => (196.0, 220),
        ServerCue::SaveCompleted => (784.0, 70),
        ServerCue::ReloadQuestion => (392.0, 150),
    }
}

/// Play one earcon through WebAudio: a sine oscillator with a short
/// attack/decay envelope (no clicks).
///
/// The audio context is created lazily on the first cue. Browsers
/// suspend audio contexts until a user gesture, so a suspended
/// context resumes in the background and the cue that found it is
/// dropped; the next cue plays. A context that cannot be created at
/// all disables earcons silently: cues are an accessibility aid, not
/// a failure domain.
fn play_earcon(session: &Rc<RefCell<WebSession>>, freq_hz: f32, duration_ms: u32) {
    let mut s = session.borrow_mut();
    if s.audio.is_none() {
        s.audio = web_sys::AudioContext::new().ok();
    }
    let ctx = match s.audio.as_ref() {
        Some(ctx) => ctx,
        None => return,
    };
    if ctx.state() == web_sys::AudioContextState::Suspended {
        let _ = ctx.resume();
        return;
    }
    let osc = match ctx.create_oscillator() {
        Ok(osc) => osc,
        Err(_) => return,
    };
    let gain = match ctx.create_gain() {
        Ok(gain) => gain,
        Err(_) => return,
    };
    osc.set_type(web_sys::OscillatorType::Sine);
    osc.frequency().set_value(freq_hz);
    let start = ctx.current_time();
    let dur = f64::from(duration_ms) / 1000.0;
    // 10 ms attack, exponential decay to silence by the duration:
    // the envelope is what keeps a 70 ms blip from clicking.
    let envelope = gain.gain();
    let _ = envelope.set_value_at_time(0.0001, start);
    let _ = envelope.exponential_ramp_to_value_at_time(0.25, start + 0.01);
    let _ = envelope.exponential_ramp_to_value_at_time(0.0001, start + dur);
    if osc.connect_with_audio_node(&gain).is_err() {
        return;
    }
    if gain.connect_with_audio_node(&ctx.destination()).is_err() {
        return;
    }
    let _ = osc.start_with_when(start);
    let _ = osc.stop_with_when(start + dur + 0.05);
}

/// Write an announcement to the page's `aria-live` region. The text
/// comes from the server's snapshot diffs, verbatim: the client
/// never derives announcements from the rendered grid. Identical
/// repeats are dropped: re-announcing the same string is spam, and
/// most screen readers skip it anyway.
fn announce(session: &Rc<RefCell<WebSession>>, text: &str) {
    let s = session.borrow();
    let document = match s.canvas.owner_document() {
        Some(document) => document,
        None => return,
    };
    if let Some(region) = document.get_element_by_id("erax-live") {
        let current = region.text_content().unwrap_or_default();
        if current != text {
            region.set_text_content(Some(text));
        }
    }
}

fn handle_server_frame(session: &Rc<RefCell<WebSession>>, frame: ServerFrame) {
    // The page status line is driven from here once the socket is up:
    // `erax_start` leaves it at "waiting for the server handshake…".
    let set_page_status = |text: &str| {
        if let Some(document) = session.borrow().canvas.owner_document() {
            set_status(&document, text);
        }
    };
    match frame {
        ServerFrame::Hello { version, .. } => {
            // width/height are informational (the server resizes to the
            // grid we report in our Resize frames); the client's own canvas
            // grid is authoritative for rendering. Only the version gates.
            if version != PROTOCOL_VERSION {
                web_sys::console::error_1(
                    &format!(
                        "protocol version mismatch: server {version}, client {PROTOCOL_VERSION}"
                    )
                    .into(),
                );
                // The protocol requires a client that doesn't speak the
                // version to disconnect (protocol.rs); continuing would
                // speak frames the client may misinterpret.
                let _ = session.borrow().ws.close();
            }
        }
        ServerFrame::AuthOk => {
            set_page_status("connected");
        }
        ServerFrame::AuthError { reason } => {
            web_sys::console::error_1(&format!("auth failed: {reason}").into());
            set_page_status(&format!("startup: running: auth failed: {reason}"));
        }
        ServerFrame::Screen {
            seq,
            cells,
            cursor,
            ..
        } => {
            // `dirty` is intentionally unused: the cells ARE the update and
            // the client re-renders the full grid each frame, so dirty rects
            // would only add bookkeeping, not work saved.
            //
            // `seq` is consumed for loss detection: frames are contiguous
            // per connection, so a gap means a dropped frame.
            //
            // The block scopes the session borrow: it ends before the
            // skip-retry path below re-borrows the session.
            let outcome = {
                // `s` derefs the RefMut so the borrow checker sees the
                // disjoint field borrows below.
                let s = &mut *session.borrow_mut();
                // `seq` is consumed for loss detection: frames are contiguous
                // per connection, so a gap means a dropped frame. The predicate
                // is pure and unit-tested (`cursor::seq_gap_detected`).
                if crate::gui::core::cursor::seq_gap_detected(s.last_seq, seq) {
                    web_sys::console::warn_1(
                        &format!("screen frame gap: {} -> {}", s.last_seq, seq).into(),
                    );
                }
                s.last_seq = seq;

                // Track the backend cursor verbatim: shape → presentation core,
                // position/visibility → IME anchor. Nothing inferred here.
                s.cursor = cursor;
                let dpr = s.dpr;
                s.renderer.set_cursor(cursor.style, dpr as f32);

                for cu in cells {
                    if let Some(cell) = s.screen.get_mut(cu.x, cu.y) {
                        *cell = cu.cell;
                    }
                }

                // Reposition the IME scratchpad at the backend cursor so the OS
                // composition popup opens at the text cursor (F1). Parked
                // offscreen while the cursor is hidden. The decision itself is
                // pure and unit-tested (`cursor::ime_anchor_for_frame`); this is
                // only the DOM write.
                let rect = s.canvas.get_bounding_client_rect();
                let (left_css, top_css) = crate::gui::core::cursor::ime_anchor_for_frame(
                    cursor.visible,
                    cursor.x,
                    cursor.y,
                    rect.left(),
                    rect.top(),
                    f64::from(s.renderer.cell_width()) / dpr,
                    f64::from(s.renderer.cell_height()) / dpr,
                );
                let ime_style = s.ime.style();
                let _ = ime_style.set_property("left", &format!("{left_css}px"));
                let _ = ime_style.set_property("top", &format!("{top_css}px"));

                // Disjoint field borrows through the &mut.
                let (renderer, screen) = (&mut s.renderer, &s.screen);
                renderer.render(screen)
            };
            match outcome {
                Err(e) => {
                    web_sys::console::error_1(&format!("render: {e}").into());
                }
                Ok(FrameOutcome::Presented) => {
                    session.borrow_mut().repaint_attempts = 0;
                }
                Ok(FrameOutcome::Skipped) => {
                    // The surface was not ready (or the tab occluded): the
                    // cells are already in the screen model, so retry the
                    // paint on the next animation frame instead of waiting
                    // for the next server frame (an idle editor sends none).
                    // Loud on purpose: a silently skipped first frame is
                    // exactly the blank-on-open report.
                    web_sys::console::warn_1(
                        &"render: surface not ready, frame skipped; retrying".into(),
                    );
                    schedule_repaint(&session);
                }
            }
        }
        ServerFrame::Bye { reason } => {
            web_sys::console::log_1(&format!("server: {reason}").into());
            set_page_status(&format!(
                "startup: running: server closed the session: {reason}"
            ));
        }
        ServerFrame::Cue { cue } => {
            let (freq_hz, duration_ms) = earcon_for_cue(&cue);
            play_earcon(session, freq_hz, duration_ms);
        }
        ServerFrame::Announce { text } => {
            announce(session, &text);
        }
    }
}

/// Repaint the current screen model on the next animation frame.
///
/// A skipped surface frame (timeout, occluded tab) leaves the canvas
/// blank with no server frame on the way to recover it: an idle editor
/// sends none. The retry repaints the cells already in the screen model,
/// so the first flush lands even when the first surface acquisitions
/// fail. At most one retry is pending at a time; a presented frame ends
/// the chain, and the chain gives up after ~10 s of continuous skipping.
fn schedule_repaint(session: &Rc<RefCell<WebSession>>) {
    /// Retry budget: ~10 s at 60 fps of transient surface skipping.
    const MAX_REPAINT_ATTEMPTS: u32 = 600;
    {
        let mut s = session.borrow_mut();
        if s.repaint_pending {
            return;
        }
        if s.repaint_attempts >= MAX_REPAINT_ATTEMPTS {
            web_sys::console::error_1(
                &"render: surface kept skipping frames; giving up the repaint retry".into(),
            );
            return;
        }
        s.repaint_pending = true;
        s.repaint_attempts += 1;
    }
    let again = Rc::clone(session);
    let cb = Closure::<dyn FnMut()>::new(move || repaint_once(&again));
    if let Some(window) = web_sys::window()
        && let Err(e) = window.request_animation_frame(cb.as_ref().unchecked_ref())
    {
        web_sys::console::error_1(&format!("repaint: request_animation_frame: {e:?}").into());
    }
    cb.forget();
}

/// One repaint attempt: render the screen model as-is. A further skip
/// re-queues; anything else ends the retry chain and resets the budget.
fn repaint_once(session: &Rc<RefCell<WebSession>>) {
    let outcome = {
        // Deref once so the field borrows below are disjoint (a `RefMut`
        // method call would borrow through `DerefMut` and conflict).
        let s = &mut *session.borrow_mut();
        s.repaint_pending = false;
        let (renderer, screen) = (&mut s.renderer, &s.screen);
        renderer.render(screen)
    };
    match outcome {
        Err(e) => {
            session.borrow_mut().repaint_attempts = 0;
            web_sys::console::error_1(&format!("render (repaint): {e}").into());
        }
        Ok(FrameOutcome::Presented) => {
            session.borrow_mut().repaint_attempts = 0;
        }
        Ok(FrameOutcome::Skipped) => schedule_repaint(session),
    }
}

/// Tab visibility → repaint.
///
/// A page opened in a background tab never presents: the surface reports
/// occluded, every frame skips, and `requestAnimationFrame` never fires
/// while hidden. When the tab becomes visible, paint the current screen
/// model instead of waiting for input to produce a server frame.
fn wire_visibility(document: &Document, session: Rc<RefCell<WebSession>>) -> Result<(), JsValue> {
    let onvisible = {
        let session = session.clone();
        Closure::<dyn FnMut()>::new(move || {
            let visible = web_sys::window()
                .and_then(|w| w.document())
                .map(|d| d.visibility_state() == web_sys::VisibilityState::Visible)
                .unwrap_or(false);
            if visible {
                repaint_once(&session);
            }
        })
    };
    document.add_event_listener_with_callback("visibilitychange", onvisible.as_ref().unchecked_ref())?;
    onvisible.forget();
    Ok(())
}

/// DOM keyboard → KeyPress (same raw types as the native shell).
fn wire_keyboard(
    document: &Document,
    session: Rc<RefCell<WebSession>>,
) -> Result<(), JsValue> {
    // keydown: physical keys + chords.
    let onkeydown = {
        let session = session.clone();
        Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
            // Browser-reserved keys go to the browser ONLY: the editor
            // never sees them (KEYBINDINGS.md: "the key never reaches the
            // backend"). Don't preventDefault, don't forward.
            if is_browser_reserved(&event) {
                return;
            }
            if let Some(kp) = dom_key_to_press(&event) {
                // Keys we forward are ours: keep the browser out of them.
                event.prevent_default();
                send(&session, &ClientFrame::Input {
                    event: InputEvent::Key(kp),
                });
            }
        })
    };
    document
        .add_event_listener_with_callback("keydown", onkeydown.as_ref().unchecked_ref())?;
    onkeydown.forget();

    // keypress is deprecated; printable text comes from keydown's `key`
    // for single characters (handled in dom_key_to_press).
    Ok(())
}

/// Convert a DOM KeyboardEvent to the backend's KeyPress.
///
/// Printable characters use `event.key()` (the OS/layout-correct text,
/// like the native shell's `KeyEvent.text`). Ctrl/Alt chords use
/// `event.code()` (physical position, like the native shell).
fn dom_key_to_press(event: &KeyboardEvent) -> Option<KeyPress> {
    let ctrl = event.ctrl_key();
    let alt = event.alt_key();
    let shift = event.shift_key();
    let key = event.key();
    let code = event.code();

    // Single printable character without Ctrl/Alt: the character is the key.
    if !ctrl && !alt && key.chars().count() == 1 {
        let c = key.chars().next()?;
        if !c.is_control() {
            return Some(KeyPress {
                key: Key::Char(c),
                shift,
                alt: false,
                ctrl: false,
            });
        }
    }

    // Special keys and Ctrl/Alt chords by physical code.
    let base = match code.as_str() {
        "Enter" | "NumpadEnter" => Key::Enter,
        "Tab" => Key::Tab,
        "Backspace" => Key::Backspace,
        "Delete" => Key::Delete,
        "Escape" => Key::Esc,
        "ArrowUp" => Key::Up,
        "ArrowDown" => Key::Down,
        "ArrowLeft" => Key::Left,
        "ArrowRight" => Key::Right,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "Insert" => Key::Insert,
        "Space" => Key::Char(' '),
        "F1" => Key::F(1),
        "F2" => Key::F(2),
        "F3" => Key::F(3),
        "F4" => Key::F(4),
        "F5" => Key::F(5),
        "F6" => Key::F(6),
        "F7" => Key::F(7),
        "F8" => Key::F(8),
        "F9" => Key::F(9),
        "F10" => Key::F(10),
        "F11" => Key::F(11),
        "F12" => Key::F(12),
        _ => {
            // Ctrl/Alt chord: physical position → base key (positional, like
            // the native shell). Letters: "KeyX" → 'x'.
            if (ctrl || alt) && let Some(stripped) = code.strip_prefix("Key") {
                let mut chars = stripped.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    Key::Char(c.to_ascii_lowercase())
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
    };
    Some(KeyPress {
        key: base,
        shift,
        alt,
        ctrl,
    })
}

/// Browser-reserved keys the page must NOT preventDefault (KEYBINDINGS.md).
/// These go to the browser; the editor never sees them.
fn is_browser_reserved(event: &KeyboardEvent) -> bool {
    let ctrl = event.ctrl_key();
    let key = event.key();
    // Ctrl+W (close tab), Ctrl+N (new window), Ctrl+T (new tab),
    // Ctrl+Tab / Ctrl+Shift+Tab (tab switching), Alt+Left/Right.
    (ctrl && matches!(key.as_str(), "w" | "W" | "n" | "N" | "t" | "T" | "Tab"))
        || (event.alt_key() && matches!(key.as_str(), "ArrowLeft" | "ArrowRight"))
}

/// DOM mouse → grid cell converter (shared by the mouse listeners).
type ToCell = Rc<dyn Fn(&DomMouseEvent) -> Option<(u16, u16)>>;
/// DOM mouse → backend `MouseEvent` builder.
type MkMouseEvent = Rc<dyn Fn(&DomMouseEvent, MouseEventKind) -> Option<MouseEvent>>;

/// DOM mouse → MouseEvent (cell coordinates via the core's metrics).
fn wire_mouse(
    canvas: &HtmlCanvasElement,
    session: Rc<RefCell<WebSession>>,
) -> Result<(), JsValue> {
    // to_cell and mk are Rc'd so the 'static DOM closures can share them.
    let to_cell: ToCell = {
        let session = session.clone();
        Rc::new(move |event: &DomMouseEvent| {
            let s = session.borrow();
            let rect = s.canvas.get_bounding_client_rect();
            let px = (event.client_x() as f64 - rect.left()) as f32;
            let py = (event.client_y() as f64 - rect.top()) as f32;
            // Shared geometry from the presentation core (no second impl).
            s.renderer.grid_cell_at(px, py)
        })
    };

    let mk: MkMouseEvent = {
        let to_cell = to_cell.clone();
        Rc::new(move |event: &DomMouseEvent, kind: MouseEventKind| {
            let (column, row) = to_cell(event)?;
            Some(MouseEvent {
                column,
                row,
                kind,
                shift: event.shift_key(),
                alt: event.alt_key(),
                ctrl: event.ctrl_key(),
                // DOM click count (1/2/3 for single/double/triple click):
                // the backend maps 2/3 to word/line selection.
                click_count: event.detail().clamp(1, 3) as u8,
                scroll_amount: 0,
            })
        })
    };

    // Which button is currently held for drag-selection (None = no drag).
    // Shared by mousedown (set), mouseup (clear), mousemove (emit Drag).
    let drag_button: Rc<std::cell::Cell<Option<MouseButton>>> =
        Rc::new(std::cell::Cell::new(None));

    let onmousedown = {
        let session = session.clone();
        let mk = mk.clone();
        let drag_button = drag_button.clone();
        Closure::<dyn FnMut(DomMouseEvent)>::new(move |event: DomMouseEvent| {
            let button = match event.button() {
                0 => MouseButton::Left,
                1 => MouseButton::Middle,
                2 => MouseButton::Right,
                _ => return,
            };
            drag_button.set(Some(button));
            // Keep the IME scratchpad focused so composition keeps working
            // after clicks (best effort; keydown goes to the document anyway).
            let _ = session.borrow().ime.focus();
            if let Some(me) = (*mk)(&event, MouseEventKind::Down(button)) {
                send(&session, &ClientFrame::Input {
                    event: InputEvent::Mouse(me),
                });
            }
        })
    };
    canvas.add_event_listener_with_callback("mousedown", onmousedown.as_ref().unchecked_ref())?;
    onmousedown.forget();

    let onmouseup = {
        let session = session.clone();
        let mk = mk.clone();
        let drag_button = drag_button.clone();
        Closure::<dyn FnMut(DomMouseEvent)>::new(move |event: DomMouseEvent| {
            let button = match event.button() {
                0 => MouseButton::Left,
                1 => MouseButton::Middle,
                2 => MouseButton::Right,
                _ => return,
            };
            drag_button.set(None);
            if let Some(me) = (*mk)(&event, MouseEventKind::Up(button)) {
                send(&session, &ClientFrame::Input {
                    event: InputEvent::Mouse(me),
                });
            }
        })
    };
    canvas.add_event_listener_with_callback("mouseup", onmouseup.as_ref().unchecked_ref())?;
    onmouseup.forget();

    // Mouse drag → Drag events (text selection). Only while a button is
    // held (tracked from mousedown); the button pressed started the drag.
    let onmousemove = {
        let session = session.clone();
        let mk = mk.clone();
        let drag_button = drag_button.clone();
        Closure::<dyn FnMut(DomMouseEvent)>::new(move |event: DomMouseEvent| {
            let Some(button) = drag_button.get() else {
                return;
            };
            if let Some(me) = (*mk)(&event, MouseEventKind::Drag(button)) {
                send(&session, &ClientFrame::Input {
                    event: InputEvent::Mouse(me),
                });
            }
        })
    };
    canvas.add_event_listener_with_callback("mousemove", onmousemove.as_ref().unchecked_ref())?;
    onmousemove.forget();

    let onwheel = {
        let session = session.clone();
        let mk = mk.clone();
        Closure::<dyn FnMut(web_sys::WheelEvent)>::new(move |event: web_sys::WheelEvent| {
            event.prevent_default();
            let kind = if event.delta_y() > 0.0 {
                MouseEventKind::ScrollDown
            } else if event.delta_y() < 0.0 {
                MouseEventKind::ScrollUp
            } else {
                return;
            };
            if let Some(me) = (*mk)(&event, kind) {
                send(&session, &ClientFrame::Input {
                    event: InputEvent::Mouse(me),
                });
            }
        })
    };
    canvas.add_event_listener_with_callback("wheel", onwheel.as_ref().unchecked_ref())?;
    onwheel.forget();

    // Right-click is forwarded to the editor (MouseButton::Right); don't
    // also pop the browser's context menu over the canvas.
    let oncontextmenu = {
        Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            event.prevent_default();
        })
    };
    canvas.add_event_listener_with_callback(
        "contextmenu",
        oncontextmenu.as_ref().unchecked_ref(),
    )?;
    oncontextmenu.forget();
    Ok(())
}

/// IME composition → ImeCommit, hosted on the hidden textarea.
///
/// The textarea holds DOM focus (positioned at the backend cursor), so the
/// OS IME activates and its composition popup opens at the text cursor.
/// On `compositionend` the committed text goes to the backend as
/// `ImeCommit`; the scratch value is cleared immediately after.
fn wire_ime(
    ime: &HtmlTextAreaElement,
    session: Rc<RefCell<WebSession>>,
) -> Result<(), JsValue> {
    let oncompositionend = {
        let session = session.clone();
        let ime = ime.clone();
        Closure::<dyn FnMut(web_sys::CompositionEvent)>::new(
            move |event: web_sys::CompositionEvent| {
                if let Some(text) = event.data()
                    && !text.is_empty()
                {
                    send(&session, &ClientFrame::Input {
                        event: InputEvent::ImeCommit { text },
                    });
                }
                // Scratchpad discipline: the value must never accumulate.
                ime.set_value("");
            },
        )
    };
    ime.add_event_listener_with_callback(
        "compositionend",
        oncompositionend.as_ref().unchecked_ref(),
    )?;
    oncompositionend.forget();

    // Defensive: if anything lands in the scratchpad outside a composition
    // (autofill, drag-drop, mobile keyboards), drop it — real input travels
    // via the keydown protocol frames, never via this element's value.
    let oninput = {
        let ime = ime.clone();
        Closure::<dyn FnMut(web_sys::InputEvent)>::new(move |event: web_sys::InputEvent| {
            if !event.is_composing() {
                ime.set_value("");
            }
        })
    };
    ime.add_event_listener_with_callback("input", oninput.as_ref().unchecked_ref())?;
    oninput.forget();
    Ok(())
}

/// Browser resize → Resize frame + canvas + core resize.
fn wire_resize(
    window: &Window,
    session: Rc<RefCell<WebSession>>,
) -> Result<(), JsValue> {
    let onresize = {
        let session = session.clone();
        Closure::<dyn FnMut()>::new(move || {
            let window = match web_sys::window() {
                Some(w) => w,
                None => return,
            };
            let dpr = window.device_pixel_ratio();
            let css_w = window.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1200.0);
            let css_h = window.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(800.0);
            let phys_w = (css_w * dpr) as u32;
            let phys_h = (css_h * dpr) as u32;
            let mut s = session.borrow_mut();
            s.canvas.set_width(phys_w);
            s.canvas.set_height(phys_h);
            s.renderer.resize((phys_w, phys_h));
            let (cols, rows) = s.renderer.grid_size();
            s.screen = ScreenBuffer::new(cols as u16, rows as u16);
            send(&session, &ClientFrame::Resize {
                width: cols as u16,
                height: rows as u16,
            });
        })
    };
    window.add_event_listener_with_callback("resize", onresize.as_ref().unchecked_ref())?;
    onresize.forget();
    Ok(())
}

// ---------------------------------------------------------------------------
// ScreenBuffer helpers
// ---------------------------------------------------------------------------

impl ScreenBuffer {
    fn get_mut(&mut self, x: u16, y: u16) -> Option<&mut Cell> {
        if x < self.width && y < self.height {
            let idx = (y as usize) * (self.width as usize) + (x as usize);
            Some(&mut self.cells[idx])
        } else {
            None
        }
    }
}
