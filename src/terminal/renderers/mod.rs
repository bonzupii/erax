//! Specialized renderers for TUI components
//!
//! This module contains extracted rendering logic from `Display`,
//! organized into focused, testable components.

mod dirty_tracker;
pub mod menu_renderer;
pub mod status_renderer;
pub mod window_renderer;

pub use dirty_tracker::DirtyTracker;
pub use menu_renderer::MenuRenderer;
pub use status_renderer::StatusRenderer;
pub use window_renderer::WindowRenderer;
