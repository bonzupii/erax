//! Application execution modes for erax.
//!
//! The three modes — sed (stream editor), terminal (TUI), and GUI — are all
//! compiled unconditionally: no feature gates, no stub fallbacks.
//! Each runner takes the typed [`crate::cli::Invocation`] and returns a
//! concrete typed error.

mod gui;
mod sed;
mod tui;

pub use gui::{run_gui_mode, GuiError};
pub use sed::run_sed_mode;
pub use tui::run_terminal_mode;
