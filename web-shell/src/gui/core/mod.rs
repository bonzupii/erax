//! Presentation core (wasm): same sources as native.

#[path = "../../../../src/gui/core/atlas.rs"]
pub mod atlas;

#[path = "../../../../src/gui/core/cursor.rs"]
pub mod cursor;

#[path = "../../../../src/gui/core/font_manager.rs"]
pub mod font_manager;

#[path = "../../../../src/gui/core/quad_renderer.rs"]
pub mod quad_renderer;

#[path = "../../../../src/gui/core/renderer.rs"]
pub mod renderer;

pub use renderer::{RenderError, Renderer, RendererParams};
