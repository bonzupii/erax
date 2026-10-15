//! Terminal frontend: raw mode, events, and rendering.
pub mod capabilities;
pub mod color;
pub mod cue_sink;
pub mod display;
pub mod event_handler;
pub mod events;
pub mod model;
#[cfg(target_os = "linux")]
pub mod gpm;
pub mod input_state;
pub mod keybinds;
pub mod modal;

pub mod prompt;
pub mod raw;
pub mod render;
pub mod renderers;
pub mod scrollbar;
pub mod theme;
