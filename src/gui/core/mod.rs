//! Presentation core: platform-agnostic GPU rendering.
//!
//! This is the shared rendering core used by both shells:
//! - the native winit shell (`crate::gui::shell::native`)
//! - the web wasm shell (wasm32, browser canvas)
//!
//! The core takes a wgpu `SurfaceTarget` and explicit font bytes. It has
//! NO winit dependency, NO web_sys dependency, and performs NO OS font
//! enumeration. It consumes the backend screen model (`ScreenBuffer` +
//! N-273 `CellRole` metadata) and stays a dumb passthrough (principle 12).

pub mod atlas;
pub mod cursor;
pub mod font_manager;
pub mod quad_renderer;
pub mod renderer;

pub use renderer::{mouse_pos_to_grid, RenderError, Renderer, RendererParams, ShapedLineInput, TextSpan};
