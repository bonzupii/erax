//! GUI module for era using wgpu + winit
//!
//! This is a terminal emulator shell that renders the TUI's ScreenBuffer
//! using GPU acceleration. The GUI handles display only - all editing logic
//! stays in the TUI layer. The menu system is now in core/ and rendered via
//! the TUI's ScreenBuffer for feature parity.

mod grid_renderer;
pub mod input;

pub use grid_renderer::GridRenderer;

/// Default font size in pixels
pub const DEFAULT_FONT_SIZE: f32 = 16.0;
