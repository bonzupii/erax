//! Native winit shell: windowing + raw input forwarding.
//!
//! Thinned to the minimum: create the winit window, resolve the configured
//! font to bytes (platform font lookup lives HERE, not in the presentation
//! core), construct the core `Renderer` from the window's surface target,
//! and forward winit events as the backend's raw input types.

use std::sync::Arc;
use winit::window::Window;

use crate::config::font::{FONT_SIZE_PX, FONT_STACK};
use crate::gui::core::font_manager::FontSource;
use crate::gui::core::Renderer;

/// Resolve the configured font stack to bytes via the platform font
/// database. This is the ONE place that decides *which* fonts the editor
/// uses: the presentation core never selects fonts, it only shapes with
/// the bytes it is given (plus the platform fallback as a last resort for
/// scripts the stack cannot cover).
///
/// Returns one entry per `FONT_STACK` family found on the system, in
/// config order (primary first), deduplicated by family name. Families
/// with no system match are skipped: the stack is a preference list, not
/// a requirement. An empty return means no configured font is available
/// (the core then shapes with an empty database plus its system
/// last-resort).
pub fn resolve_config_fonts() -> Vec<FontSource> {
    let mut db = fontdb::Database::new();
    // Platform font lookup is a shell concern, not a core concern.
    db.load_system_fonts();
    let mut out = Vec::new();
    for family in FONT_STACK {
        if out.iter().any(|f: &FontSource| f.family.as_deref() == Some(*family)) {
            continue;
        }
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            ..Default::default()
        };
        let id = match db.query(&query) {
            Some(id) => id,
            None => continue,
        };
        let (source, _) = match db.face_source(id) {
            Some(s) => s,
            None => continue,
        };
        let bytes: Vec<u8> = match source {
            fontdb::Source::Binary(data) => data.as_ref().as_ref().to_vec(),
            fontdb::Source::File(path) => match std::fs::read(path) {
                Ok(b) => b,
                Err(_) => continue,
            },
            fontdb::Source::SharedFile(path, ..) => match std::fs::read(path) {
                Ok(b) => b,
                Err(_) => continue,
            },
        };
        out.push(FontSource {
            bytes: Arc::new(bytes),
            family: Some(family.to_string()),
        });
    }
    out
}

/// Resolve the primary configured font (the stack head).
///
/// Kept for the web server's `/font` endpoint, which serves the single
/// primary font; the stack endpoints use [`resolve_config_fonts`].
pub fn resolve_config_font() -> Option<(Arc<Vec<u8>>, String)> {
    resolve_config_fonts()
        .into_iter()
        .next()
        .map(|f| (f.bytes, f.family.unwrap_or_default()))
}

/// Create the presentation core for a winit window.
pub fn create_renderer(
    window: Arc<Window>,
    scale_factor: f64,
) -> Result<Renderer, Box<dyn std::error::Error>> {
    let size = window.inner_size();
    let fonts = resolve_config_fonts();
    // Diagnosability: the shaped output depends on which fonts resolved.
    // A missing primary (no ligatures, tofu) is a configuration fact, not
    // a shaping bug; print the stack once so it is observable.
    if fonts.is_empty() {
        eprintln!("erax gui: no FONT_STACK family found on this system; shaping without fonts");
    } else {
        let names: Vec<&str> = fonts
            .iter()
            .map(|f| f.family.as_deref().unwrap_or("?"))
            .collect();
        eprintln!("erax gui: font stack: {}", names.join(", "));
    }
    let target = wgpu::SurfaceTarget::from(window);
    // The core constructor is async (adapter acquisition resolves through
    // the platform event loop); on a native thread blocking here is sound.
    pollster::block_on(Renderer::new(crate::gui::core::RendererParams {
        target,
        width: size.width,
        height: size.height,
        fonts,
        font_size: FONT_SIZE_PX,
        scale_factor,
        backends: wgpu::Backends::PRIMARY,
    }))
}
