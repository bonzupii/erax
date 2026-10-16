//! WebSocket server for the WebUI (VS Code-server style).
//!
//! Opt-in only. Binds 127.0.0.1 by default; a per-session random token is
//! REQUIRED (there is no unauthenticated mode); binding a non-loopback
//! address requires the explicit acknowledging flag
//! (`--web-allow-remote`, documented in README).
//!
//! Architecture: Core+TUI run server-side, headless. The TUI renders into
//! its `ScreenBuffer` (the backend screen model); instead of drawing to a
//! terminal, the server diffs the buffers and streams the typed protocol.
//! Input arrives as the backend's raw input types (`KeyPress`,
//! `MouseEvent`) and is dispatched through the same
//! `process_terminal_event` as the TUI — the GUI stays a dumb passthrough.

use std::io::Read;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use tungstenite::{Message, WebSocket, accept_with_config};
use tungstenite::protocol::WebSocketConfig;

use crate::cli::Invocation;
use crate::config::keys::{INPUT_MODEL, bindings_for_model};
use crate::core::app::EditorApp;
use crate::terminal::display::*;
use crate::terminal::events::EditorEvent;

use super::entropy::generate_token;
use super::protocol::*;

/// Errors from server startup and session handling.
#[derive(Debug)]
pub enum ServerError {
    /// Bind address is not loopback without the acknowledging flag.
    NonLoopbackWithoutFlag(String),
    /// TCP bind/listen failed.
    Bind(std::io::Error),
    /// WebSocket handshake failed.
    Handshake(String),
    /// Protocol error (bad frame, version mismatch, auth failure).
    Protocol(String),
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerError::NonLoopbackWithoutFlag(addr) => write!(
                f,
                "refusing to bind non-loopback address {addr} without --web-allow-remote \
                 (see README: WebUI server security)"
            ),
            ServerError::Bind(e) => write!(f, "cannot bind web server socket: {e}"),
            ServerError::Handshake(e) => write!(f, "websocket handshake failed: {e}"),
            ServerError::Protocol(e) => write!(f, "web protocol error: {e}"),
        }
    }
}

/// True if `addr` is a loopback address (127.0.0.0/8 or ::1).
fn is_loopback(addr: &SocketAddr) -> bool {
    match addr {
        SocketAddr::V4(v4) => v4.ip().is_loopback(),
        SocketAddr::V6(v6) => v6.ip().is_loopback(),
    }
}

/// Constant-time token comparison: no early exit on the first differing
/// byte. The token is a 128-bit random value on a loopback socket, so a
/// timing side channel is not a realistic attack here — this is
/// defense-in-depth so the comparison semantics don't depend on that
/// threat assessment staying true.
fn tokens_equal(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// A bound server: listening socket + the per-session token.
/// Returned by [`bind_server`]; the accept loop runs via [`serve`].
#[derive(Debug)]
pub struct BoundServer {
    /// The address actually bound (useful with port 0).
    pub addr: SocketAddr,
    /// The per-session random token (required by every client).
    pub token: String,
    listener: TcpListener,
}

/// Bind the WebUI server socket and generate the session token, without
/// running the accept loop. Used by `run_web_server` and by tests.
pub fn bind_server(bind: &str, allow_non_loopback: bool) -> Result<BoundServer, ServerError> {
    let addr: SocketAddr = bind
        .parse()
        .map_err(|_| ServerError::Bind(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid bind address: {bind}"),
        )))?;
    if !is_loopback(&addr) && !allow_non_loopback {
        return Err(ServerError::NonLoopbackWithoutFlag(bind.to_string()));
    }
    // Infallible: the token generator has no failure mode, so binding
    // cannot fail for lack of entropy.
    let token = generate_token();
    let listener = TcpListener::bind(addr).map_err(ServerError::Bind)?;
    let local = listener.local_addr().map_err(ServerError::Bind)?;
    Ok(BoundServer {
        addr: local,
        token,
        listener,
    })
}

/// Maximum concurrent client connections (WebSocket sessions and plain
/// HTTP asset fetches alike). Each connection holds a thread; without a
/// cap, unauthenticated connection floods exhaust threads/memory. Beyond
/// the cap the accept loop drops the socket immediately — no thread, no
/// session, no token check.
const MAX_CONCURRENT_CONNECTIONS: usize = 32;

/// RAII guard for the concurrent-connection counter: the count is released
/// when the handler thread exits, including on panic.
struct ConnectionGuard {
    active: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.active
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Run the accept loop for a bound server: one editor session per
/// connection, each on its own thread. Blocks forever.
pub fn serve(server: BoundServer, inv: &Invocation) {
    let token = server.token.clone();
    let active = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for stream in server.listener.incoming() {
        match stream {
            Ok(stream) => {
                if active.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    >= MAX_CONCURRENT_CONNECTIONS
                {
                    active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    eprintln!("web: concurrent connection cap reached; dropping connection");
                    drop(stream);
                    continue;
                }
                let token = token.clone();
                let files = inv.files.clone();
                let theme = inv.theme;
                let wrap = inv.wrap;
                let active = active.clone();
                thread::spawn(move || {
                    let _guard = ConnectionGuard { active };
                    if let Err(e) = handle_connection(stream, &token, &files, theme, wrap) {
                        eprintln!("web session error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("web accept error: {e}"),
        }
    }
}

/// Run the WebUI server. Blocks forever (Ctrl-C to stop).
///
/// - `bind`: e.g. `"127.0.0.1:8080"`. Default is loopback-only.
/// - `allow_non_loopback`: the explicit acknowledging flag required for a
///   non-loopback bind. Without it, a non-loopback bind is refused.
/// - `inv.files`: files to open in the editor session.
///
/// Prints the URL and the per-session token to stdout. The token is
/// required by every client; there is no unauthenticated mode.
pub fn run_web_server(
    inv: &Invocation,
    bind: &str,
    allow_non_loopback: bool,
) -> Result<(), ServerError> {
    let server = bind_server(bind, allow_non_loopback)?;
    println!("erax web server listening on {}", server.addr);
    println!("token: {}", server.token);
    println!("open: http://{}/?token={}", server.addr, server.token);
    println!("(token is required; no unauthenticated mode exists)");
    serve(server, inv);
    Ok(())
}

/// Diff the back buffer against the front buffer: the ground truth of what
/// changed. Returns the bounding dirty rect and the changed cells within it.
fn diff_buffers(back: &ScreenBuffer, front: &ScreenBuffer) -> (Vec<Rect>, Vec<CellUpdate>) {
    let w = back.width;
    let h = back.height;
    if w != front.width || h != front.height || back.cells.len() != front.cells.len() {
        // Size mismatch (resize race): treat everything as dirty.
        let cells = back
            .cells
            .iter()
            .enumerate()
            .map(|(i, cell)| CellUpdate {
                x: (i % w as usize) as u16,
                y: (i / w as usize) as u16,
                cell: *cell,
            })
            .collect();
        return (vec![Rect { x: 0, y: 0, w, h }], cells);
    }
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u16;
    let mut max_y = 0u16;
    let mut cells = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let i = y as usize * w as usize + x as usize;
            if back.cells[i] != front.cells[i] {
                if x < min_x {
                    min_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if x > max_x {
                    max_x = x;
                }
                if y > max_y {
                    max_y = y;
                }
                cells.push(CellUpdate {
                    x,
                    y,
                    cell: back.cells[i],
                });
            }
        }
    }
    if cells.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let dirty = vec![Rect {
        x: min_x,
        y: min_y,
        w: max_x - min_x + 1,
        h: max_y - min_y + 1,
    }];
    (dirty, cells)
}

/// Read one text frame from the client (with timeout).
fn read_frame(
    ws: &mut WebSocket<TcpStream>,
    timeout: Duration,
) -> Result<Option<ClientFrame>, ServerError> {
    ws.get_ref()
        .set_read_timeout(Some(timeout))
        .map_err(|e| ServerError::Protocol(format!("set_read_timeout: {e}")))?;
    match ws.read() {
        Ok(Message::Text(text)) => {
            let frame: ClientFrame =
                decode(&text).map_err(|e| ServerError::Protocol(format!("bad client frame: {e}")))?;
            Ok(Some(frame))
        }
        Ok(Message::Close(_)) | Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {
            // Handled by tungstenite; keep waiting.
            Ok(None)
        }
        Ok(Message::Binary(_)) => Err(ServerError::Protocol("binary frames not supported".into())),
        Ok(Message::Frame(_)) => Ok(None),
        Err(tungstenite::Error::Io(e))
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
        {
            Ok(None)
        }
        Err(tungstenite::Error::ConnectionClosed) => Err(ServerError::Protocol("closed".into())),
        Err(e) => Err(ServerError::Protocol(format!("websocket read: {e}"))),
    }
}

/// Send one frame to the client.
fn send_frame(ws: &mut WebSocket<TcpStream>, frame: &ServerFrame) -> Result<(), ServerError> {
    ws.send(Message::Text(encode(frame).into()))
        .map_err(|e| ServerError::Protocol(format!("websocket send: {e}")))
}

/// Handle one client connection.
///
/// Peeks at the HTTP request: a WebSocket upgrade to `/ws` becomes an editor
/// session; anything else is served as a static web asset (`/`, JS, wasm,
/// `/font`). The page is what makes the one-command startup work: the
/// browser loads the client from the same server it connects to.
fn handle_connection(
    stream: TcpStream,
    token: &str,
    files: &[std::path::PathBuf],
    theme: Option<crate::terminal::theme::ThemeId>,
    wrap: Option<bool>,
) -> Result<(), ServerError> {
    // Bound how long a client may take to send its first bytes: without
    // this, a connection that sends nothing holds a handler thread forever
    // (thread-per-connection accept loop). The WebSocket path installs its
    // own per-read timeouts; the HTTP path inherits this one.
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|e| ServerError::Protocol(format!("set_read_timeout: {e}")))?;
    // Peek at the request line without consuming it. A request line
    // split across TCP packets must not be evaluated partial: wait
    // (bounded) for its line terminator before deciding. The per-peek
    // read timeout fires while no bytes arrive at all; this deadline
    // caps a client that dribbles bytes just under that timeout.
    let mut peek_buf = [0u8; 2048];
    let peek_deadline = Instant::now() + Duration::from_secs(15);
    let n = loop {
        let n = stream
            .peek(&mut peek_buf)
            .map_err(|e| ServerError::Protocol(format!("peek: {e}")))?;
        if n == 0 || peek_buf[..n].contains(&b'\n') || n == peek_buf.len() {
            break n;
        }
        if Instant::now() >= peek_deadline {
            return Err(ServerError::Protocol("peek: request line too slow".into()));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let head = String::from_utf8_lossy(&peek_buf[..n]);
    let request_line = head.lines().next().unwrap_or("");
    let is_websocket = head.to_ascii_lowercase().contains("upgrade: websocket");

    if is_websocket && request_line.starts_with("GET /ws ") {
        // Bound frame size: the default tungstenite cap is 64 MiB, far
        // beyond anything legitimate in either direction (inbound:
        // key/mouse/resize frames are tens of bytes, IME commits bounded
        // by what a user can compose; outbound: Screen frames scale with
        // the grid). A hostile 64MB text frame would otherwise reach
        // serde_json as one giant allocation. Note tungstenite enforces
        // this cap on OUTGOING messages too, so it must cover the largest
        // legitimate Screen frame: a big-grid client (4K window, small
        // font) can exceed 1 MiB, which previously killed the session on
        // the first frame with a blank canvas client-side.
        // (WebSocketConfig is #[non_exhaustive], so the builder method is
        // used, not a literal.)
        let config = WebSocketConfig::default().max_message_size(Some(16 << 20));
        let ws = accept_with_config(stream, Some(config))
            .map_err(|e| ServerError::Handshake(e.to_string()))?;
        return handle_websocket(ws, token, files, theme, wrap);
    }

    // Static asset serving.
    handle_http(stream, request_line)
}

/// Serve a static web asset over plain HTTP.
fn handle_http(mut stream: TcpStream, request_line: &str) -> Result<(), ServerError> {
    use std::io::Write;

    // Consume the request (we already peeked at the line).
    let mut reader = std::io::BufReader::new(stream.try_clone().map_err(|e| {
        ServerError::Protocol(format!("clone stream: {e}"))
    })?);
    let mut line = String::new();
    use std::io::BufRead;
    // Read headers until the blank line. Bounded: a client streaming
    // endless headers must not hold the handler thread (or grow memory)
    // without limit. The 15s stream read timeout from `handle_connection`
    // applies on top.
    const MAX_HEADER_LINE: u64 = 8192;
    let mut header_lines = 0usize;
    let mut header_bytes = 0usize;
    loop {
        line.clear();
        // Bound the line itself, not just the total: read_line would
        // otherwise read an unbounded line into memory before the
        // aggregate check below ever runs.
        let n = reader
            .by_ref()
            .take(MAX_HEADER_LINE + 1)
            .read_line(&mut line)
            .map_err(|e| ServerError::Protocol(format!("read http: {e}")))?;
        if n == 0 {
            break;
        }
        if line.len() as u64 > MAX_HEADER_LINE {
            return Err(ServerError::Protocol("http header line too long".into()));
        }
        header_lines += 1;
        header_bytes += n;
        if header_lines > 128 || header_bytes > 65536 {
            return Err(ServerError::Protocol("http headers too large".into()));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
    }
    drop(reader);

    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/");

    let (status, content_type, body): (&str, &str, Vec<u8>) = match path {
        "/" | "/index.html" => ("200 OK", "text/html; charset=utf-8", web_asset("index.html")),
        "/erax-web-shell.js" => (
            "200 OK",
            "text/javascript; charset=utf-8",
            web_asset("erax-web-shell.js"),
        ),
        "/erax-web-shell_bg.wasm" => (
            "200 OK",
            "application/wasm",
            web_asset("erax-web-shell_bg.wasm"),
        ),
        "/font" => match crate::gui::shell::native::resolve_config_font() {
            Some((bytes, _)) => ("200 OK", "font/ttf", bytes.to_vec()),
            None => ("404 Not Found", "text/plain", b"no font available".to_vec()),
        },
        // The configured font stack as a JSON manifest: the web client
        // fetches this, then each entry's bytes, so the browser gets the
        // same ordered fallback the native shell resolves locally.
        "/font-stack" => {
            let stack = crate::gui::shell::native::resolve_config_fonts();
            let manifest: Vec<serde_json::Value> = stack
                .iter()
                .enumerate()
                .map(|(i, font)| {
                    serde_json::json!({
                        "family": font.family.as_deref().unwrap_or("monospace"),
                        "url": format!("/font/{i}"),
                    })
                })
                .collect();
            // Serialization of plain string maps cannot fail; a 500 here
            // would be a bug, not a client error.
            match serde_json::to_vec(&manifest) {
                Ok(body) => ("200 OK", "application/json", body),
                Err(_) => (
                    "500 Internal Server Error",
                    "text/plain",
                    b"manifest serialization failed".to_vec(),
                ),
            }
        }
        _ if path.starts_with("/font/") => {
            match path["/font/".len()..].parse::<usize>() {
                Ok(i) => {
                    match crate::gui::shell::native::resolve_config_fonts().get(i) {
                        Some(font) => ("200 OK", "font/ttf", font.bytes.to_vec()),
                        None => ("404 Not Found", "text/plain", b"no such font".to_vec()),
                    }
                }
                Err(_) => ("404 Not Found", "text/plain", b"not found".to_vec()),
            }
        }
        _ => ("404 Not Found", "text/plain", b"not found".to_vec()),
    };

    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .and_then(|_| stream.write_all(&body))
        .map_err(|e| ServerError::Protocol(format!("write http: {e}")))?;
    Ok(())
}

/// Serve a web asset.
///
/// The release-built client files are EMBEDDED in the server binary
/// (`include_bytes!`): `erax --web-server` works from any install layout —
/// `cargo install` puts the binary in `~/.cargo/bin` without a `web-assets/`
/// sibling, and the old "next to the executable" search broke exactly that
/// case (the compile-time repo-dir fallback only works while the build
/// tree still exists at its baked path).
///
/// `ERAX_WEB_ASSETS_DIR`, when set, overrides with files from disk: web
/// dev iteration (tweaking index.html/JS/wasm) without recompiling the
/// server. Only the three literal asset names are ever served — the
/// client's request path cannot reach the filesystem.
fn web_asset(name: &str) -> Vec<u8> {
    let override_dir = std::env::var_os("ERAX_WEB_ASSETS_DIR");
    let override_path = override_dir.as_ref().map(std::path::Path::new);
    web_asset_with(name, override_path)
}

/// Pure asset lookup (the env-var reading lives in [`web_asset`], so this
/// is unit-testable without touching the process environment).
///
/// `override_dir` (from `ERAX_WEB_ASSETS_DIR`) wins when it holds the file;
/// otherwise the embedded copy is served. Only the three literal asset
/// names are ever served — the client's request path cannot reach the
/// filesystem.
fn web_asset_with(name: &str, override_dir: Option<&std::path::Path>) -> Vec<u8> {
    if let Some(dir) = override_dir {
        let p = dir.join(name);
        if let Ok(b) = std::fs::read(p) {
            return b;
        }
    }
    match name {
        "index.html" => INDEX_HTML.to_vec(),
        "erax-web-shell.js" => WEB_SHELL_JS.to_vec(),
        "erax-web-shell_bg.wasm" => WEB_SHELL_WASM.to_vec(),
        _ => Vec::new(),
    }
}

/// The web client, embedded at compile time: index.html is the hand-written
/// file in web-assets/; the JS glue and wasm are generated by build.rs
/// (see build/web_assets.rs) into OUT_DIR from web-shell/ at build time
/// and embedded over the ERAX_WEB_ASSETS_OUT env it emits.
const INDEX_HTML: &[u8] = include_bytes!("../../web-assets/index.html");
const WEB_SHELL_JS: &[u8] =
    include_bytes!(concat!(env!("ERAX_WEB_ASSETS_OUT"), "/erax-web-shell.js"));
const WEB_SHELL_WASM: &[u8] =
    include_bytes!(concat!(env!("ERAX_WEB_ASSETS_OUT"), "/erax-web-shell_bg.wasm"));

/// The WebSocket editor session (auth + headless Core+TUI).
fn handle_websocket(
    mut ws: WebSocket<TcpStream>,
    token: &str,
    files: &[std::path::PathBuf],
    theme: Option<crate::terminal::theme::ThemeId>,
    wrap: Option<bool>,
) -> Result<(), ServerError> {
    // Default headless size; the client sends Resize on connect.
    let (mut width, mut height) = (120u16, 36u16);
    send_frame(
        &mut ws,
        &ServerFrame::Hello {
            version: PROTOCOL_VERSION,
            width,
            height,
        },
    )?;

    // --- Auth: the first frame MUST be Auth with the session token. ---
    // The auth phase is deadline-bounded: a client that connects and then
    // idles (each read timing out) must not spin this loop forever.
    let auth_deadline = Instant::now() + Duration::from_secs(30);
    let authed = loop {
        match read_frame(&mut ws, Duration::from_secs(10))? {
            Some(ClientFrame::Auth { token: t }) => break tokens_equal(&t, token),
            Some(_) => {
                send_frame(
                    &mut ws,
                    &ServerFrame::AuthError {
                        reason: "first frame must be Auth".to_string(),
                    },
                )?;
                return Err(ServerError::Protocol("auth: first frame was not Auth".into()));
            }
            None => {
                if Instant::now() >= auth_deadline {
                    return Err(ServerError::Protocol("auth: timed out waiting for Auth".into()));
                }
            }
        }
    };
    if !authed {
        send_frame(
            &mut ws,
            &ServerFrame::AuthError {
                reason: "invalid token".to_string(),
            },
        )?;
        return Err(ServerError::Protocol("auth: invalid token".into()));
    }
    send_frame(&mut ws, &ServerFrame::AuthOk)?;

    // The client sends Resize immediately on open (its grid size). Take
    // it now so the initial frame renders at the right size: otherwise
    // the first frame is the 120x36 default, the client clips it, and a
    // second full frame follows on the Resize. A short wait; a slow
    // client just gets the default and resizes via the main loop. The
    // client sends nothing but Resize before AuthOk, so any other frame
    // here is a protocol violation.
    match read_frame(&mut ws, Duration::from_millis(500))? {
        Some(ClientFrame::Resize { width: w, height: h }) => {
            width = w.max(20);
            height = h.max(10);
        }
        Some(_) => {
            return Err(ServerError::Protocol(
                "expected Resize after Auth".into(),
            ));
        }
        None => {}
    }

    // --- Headless editor session (Core+TUI, no terminal). ---
    let mut keybind_manager = crate::terminal::keybinds::KeyBindingManager::new();
    keybind_manager.load(bindings_for_model(INPUT_MODEL));
    let mut app = EditorApp::initialize(files);
    let mut display = crate::terminal::display::Display::new(width, height, theme, wrap);
    // WebUI renders with full color (the client does its own styling).
    display.display_mode = crate::terminal::capabilities::DisplayMode::TrueColor;

    // Initial full render.
    display
        .render(&mut app)
        .map_err(|e| ServerError::Protocol(format!("initial render: {e}")))?;
    {
        let show = display.show_menu_bar;
        display.render_menu_bar(show);
    }
    let mut seq = 0u64;
    send_screen(&mut ws, &mut display, &mut seq)?;

    // Baseline for the screen-reader announcement diffs: connecting is
    // not a semantic change, so the first summary seeds and is never
    // announced.
    let mut last_summary = crate::core::semantics::capture_summary(&app);

    let mut await_deadline: Option<Instant> = None;
    let await_timeout =
        Duration::from_millis(crate::config::keys::KEY_SEQUENCE_TIMEOUT_MS);

    loop {
        // AwaitLonger deadline (N-013), same as the TUI loop.
        if await_deadline.map(|d| Instant::now() >= d).unwrap_or(false) {
            await_deadline = None;
            if let Some(cmd) = keybind_manager.take_awaited()
                && crate::terminal::event_handler::dispatch_command(&mut app, &mut display, cmd)
                    .map_err(|e| ServerError::Protocol(format!("dispatch: {e}")))?
            {
                break;
            }
        }
        if keybind_manager.is_awaiting() && await_deadline.is_none() {
            await_deadline = Some(Instant::now() + await_timeout);
        }

        // Read input (100ms poll so the deadline above stays responsive).
        match read_frame(&mut ws, Duration::from_millis(100))? {
            Some(ClientFrame::Input { event }) => {
                // IME composition commit becomes one plain KeyPress per
                // character, dispatched through the normal backend key path
                // (dumb passthrough — the backend owns the semantics).
                let events: Vec<EditorEvent> = match event {
                    InputEvent::Key(kp) => vec![EditorEvent::Input(kp)],
                    InputEvent::Mouse(me) => vec![EditorEvent::Mouse(me)],
                    InputEvent::ImeCommit { text } => text
                        .chars()
                        .map(|c| {
                            EditorEvent::Input(crate::core::input::KeyPress::plain(
                                crate::core::input::Key::Char(c),
                            ))
                        })
                        .collect(),
                };
                for editor_event in events {
                    let exit = crate::terminal::event_handler::process_terminal_event(
                        &mut app,
                        &mut display,
                        &mut keybind_manager,
                        editor_event,
                    )
                    .map_err(|e| ServerError::Protocol(format!("event: {e}")))?;
                    if exit {
                        break;
                    }
                }
            }
            Some(ClientFrame::Resize { width: w, height: h }) => {
                width = w.max(20);
                height = h.max(10);
                let exit = crate::terminal::event_handler::process_terminal_event(
                    &mut app,
                    &mut display,
                    &mut keybind_manager,
                    EditorEvent::Resize(width, height),
                )
                .map_err(|e| ServerError::Protocol(format!("resize: {e}")))?;
                if exit {
                    break;
                }
            }
            Some(ClientFrame::Auth { .. }) => {
                // Already authed; ignore.
            }
            None => {}
        }

        // Poll the built-in terminal the way the TUI loop does: child
        // output must reach the client on its own, not wait for the next
        // client event to trigger a frame.
        if let Some(ref mut host) = app.terminal_host
            && host.is_alive()
            && host.read().is_some()
        {
            display.dirty = true;
        }

        if display.dirty {
            display
                .render(&mut app)
                .map_err(|e| ServerError::Protocol(format!("render: {e}")))?;
            let show = display.show_menu_bar;
            display.render_menu_bar(show);
            send_screen(&mut ws, &mut display, &mut seq)?;
        }

        // Accessibility: cue events and snapshot-diff announcements ride
        // the existing frame channel. Cues map to earcons client-side
        // and are gated by the cue bus (a disabled bus delivers
        // nothing). Announcements are computed unconditionally: the
        // aria-live region they feed is inert without assistive
        // technology, so there is no one to disturb, and gating them
        // behind the earcon opt-in would deny screen-reader users
        // their only channel.
        for cue in app.cue_bus.drain() {
            send_frame(&mut ws, &ServerFrame::Cue { cue: server_cue(cue) })?;
        }
        if let Some(summary) = crate::core::semantics::capture_summary(&app) {
            let announce = last_summary.as_ref().and_then(|old| {
                crate::core::semantics::announce_diff(
                    old,
                    &summary,
                    crate::config::accessibility::blindness::ANNOUNCE_CURSOR_LINE,
                )
            });
            last_summary = Some(summary);
            if let Some(text) = announce {
                send_frame(&mut ws, &ServerFrame::Announce { text })?;
            }
        }
    }

    send_frame(&mut ws, &ServerFrame::Bye {
        reason: "editor exited".to_string(),
    })?;
    Ok(())
}

/// Map a core cue event to its wire projection.
fn server_cue(cue: crate::core::cue::Cue) -> ServerCue {
    use crate::config::keys::ModalMode;
    use crate::core::cue::Cue;
    match cue {
        Cue::ModeEntered(mode) => ServerCue::ModeEntered {
            mode: match mode {
                ModalMode::Normal => "normal",
                ModalMode::Insert => "insert",
                ModalMode::Visual => "visual",
                ModalMode::OperatorPending => "operator_pending",
                ModalMode::CommandLine => "command_line",
            }
            .to_string(),
        },
        Cue::Error => ServerCue::Error,
        Cue::SaveCompleted => ServerCue::SaveCompleted,
        Cue::ReloadQuestionAsked => ServerCue::ReloadQuestion,
    }
}

/// Rendered screen → protocol frame.
fn send_screen(
    ws: &mut WebSocket<TcpStream>,
    display: &mut crate::terminal::display::Display,
    seq: &mut u64,
) -> Result<(), ServerError> {
    let (dirty, cells) = diff_buffers(&display.back_buffer, &display.front_buffer);
    *seq += 1;
    // Cursor from the backend screen model (N-273 metadata included).
    let (cx, cy) = display.cursor_pos.unwrap_or((0, 0));
    let role = display
        .back_buffer
        .get(cx as u16, cy as u16)
        .map(|c| c.role)
        .unwrap_or_default();
    let frame = ServerFrame::Screen {
        seq: *seq,
        dirty,
        cells,
        cursor: CursorState {
            x: cx as u16,
            y: cy as u16,
            visible: display.cursor_pos.is_some(),
            role,
            // The backend's cursor shape for the current input model/mode;
            // the client draws this verbatim (protocol v2).
            style: display.cursor_style,
        },
    };
    send_frame(ws, &frame)?;
    display.swap_buffers();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{tokens_equal, web_asset_with};

    #[test]
    fn token_comparison_semantics() {
        // Equal tokens (the only case that authenticates).
        assert!(tokens_equal("abc", "abc"));
        assert!(tokens_equal("", ""));
        // Any difference rejects: content, length, case, empty-vs-not.
        assert!(!tokens_equal("abc", "abd"));
        assert!(!tokens_equal("abc", "ab"));
        assert!(!tokens_equal("ab", "abc"));
        assert!(!tokens_equal("", "abc"));
        assert!(!tokens_equal("abc", ""));
        assert!(!tokens_equal("abc", "ABC"));
        // Real token shape: 32 hex chars.
        assert!(tokens_equal(
            "0123456789abcdef0123456789abcdef",
            "0123456789abcdef0123456789abcdef"
        ));
        assert!(!tokens_equal(
            "0123456789abcdef0123456789abcdef",
            "1123456789abcdef0123456789abcdef"
        ));
    }


    /// Golden: the web server's resize path feeds the unified model.
    /// A resize arriving over the wire is forwarded as
    /// `EditorEvent::Resize` through the same handler the TUI uses (the
    /// real server code path, minus the socket); the resulting frame map
    /// must equal the TUI golden's, and the cursor the protocol ships is
    /// the model's verdict. No protocol change: the wire already carries
    /// cells plus the cursor's visible flag, and the client paints only
    /// what the model derived.
    #[test]
    fn golden_web_resize_frame_consistency() {
        use crate::core::app::EditorApp;
        use crate::core::buffer::Buffer;
        use crate::core::geometry::{compute_frame, FrameOptions};
        use crate::core::id::BufferId;
        use crate::web::server::EditorEvent;
        use crate::terminal::display::Display;
        use crate::terminal::event_handler::process_terminal_event;
        use crate::terminal::keybinds::KeyBindingManager;

        let mut app = EditorApp::new();
        let text = (0..100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.buffers.insert(BufferId(0), Buffer::from_string(&text));
        let mut display = Display::new(80, 24, None, None);
        let mut keybind_manager = KeyBindingManager::new();

        // The wire resize: clamped and forwarded as EditorEvent::Resize.
        let (w, h) = (80u16, 24u16);
        process_terminal_event(&mut app, &mut display, &mut keybind_manager,
            EditorEvent::Resize(w, h))
            .expect("resize");
        display.render(&mut app).expect("render");

        let frame = compute_frame(
            FrameOptions {
                cols: 80,
                rows: 24,
                menu_rows: 1,
                show_line_numbers: true,
            },
            &app.layout,
            &app.geom_inputs(),
        );
        assert_eq!(frame.panes.len(), 1);
        let p = &frame.panes[0];
        assert_eq!(
            (p.frame.x, p.frame.y, p.frame.width, p.frame.height),
            (0, 1, 80, 23)
        );
        assert_eq!(p.gutter_width, 3);
        assert_eq!(
            (p.text.x, p.text.y, p.text.width, p.text.height),
            (3, 1, 76, 21)
        );
        assert_eq!(p.status_row, 23);
        assert_eq!(p.hscroll_row, 22);
        let v = p.v_scrollbar.expect("vertical scrollbar");
        assert_eq!((v.cross, v.track_start, v.track_len), (79, 2, 20));
        assert_eq!((v.thumb_start, v.thumb_len), (0, 4));
        // The cursor the protocol ships is the model's.
        assert_eq!(display.cursor_pos, p.cursor);
        assert_eq!(display.cursor_pos, Some((3, 1)));
    }

    #[test]
    fn web_asset_override_dir() {
        // Override dir containing only index.html: that asset comes from
        // disk, everything else stays embedded; unknown names stay 404.
        // Pure function — no process-environment manipulation, so this is
        // safe to run alongside every other test.
        let dir = std::env::temp_dir().join(format!("erax-web-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), b"<override>").unwrap();

        assert_eq!(web_asset_with("index.html", Some(&dir)), b"<override>");
        let wasm = web_asset_with("erax-web-shell_bg.wasm", Some(&dir));
        assert_eq!(&wasm[..4], b"\0asm");
        assert!(wasm.len() > 2_000_000);
        assert!(web_asset_with("nope", Some(&dir)).is_empty());

        std::fs::remove_dir_all(&dir).ok();

        // Without an override dir, the embedded copy is served.
        let html = web_asset_with("index.html", None);
        assert!(html.windows(7).any(|w| w == b"<title>"));
    }
}
