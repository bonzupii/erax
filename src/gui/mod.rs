//! GUI module for erax using wgpu + winit + cosmic-text
//!
//! Architecture: Two-pass rendering with cosmic-text shaping
//! - cosmic-text handles font discovery, shaping, and rasterization
//! - Background pass: gap-free cell colors via storage buffer
//! - Glyph pass: instanced sprites at pixel positions

pub mod atlas;
pub mod font_manager;
pub mod input;
pub mod quad_renderer;
pub mod renderer;

pub use renderer::Renderer;

// Legacy compatibility alias
pub type GridRenderer = Renderer;

/// Default font size in pixels
pub const DEFAULT_FONT_SIZE: f32 = 16.0;
