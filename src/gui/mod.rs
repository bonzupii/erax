//! GUI module: presentation core plus native and web shells.
//!
//! Architecture: Two-pass rendering with cosmic-text shaping
//! - cosmic-text handles font discovery, shaping, and rasterization
//! - Background pass: gap-free cell colors via storage buffer
//! - Glyph pass: instanced sprites at pixel positions

pub mod core;
pub mod shell;

// Legacy paths: the presentation core moved to `core/`. These re-exports
// keep the native shell and existing code working during the split.
pub mod atlas {
    pub use super::core::atlas::*;
}
pub mod font_manager {
    pub use super::core::font_manager::*;
}
pub mod quad_renderer {
    pub use super::core::quad_renderer::*;
}
pub mod renderer {
    pub use super::core::renderer::*;
}
pub mod input;

pub use core::renderer::{RenderError, Renderer};

// Legacy compatibility alias
pub type GridRenderer = Renderer;
