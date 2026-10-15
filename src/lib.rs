//! The erax editor library.
//!
//! Native builds include the full editor (CLI, core, TUI, GUI, server).
//! The wasm32 web shell is a separate crate (`web-shell/`) that includes
//! the presentation core and wire-protocol sources via `#[path]` — the
//! same files, no second representation.

pub mod cli;
pub mod config;
pub mod core;
pub mod gui;
pub mod run;
pub mod sed;
pub mod terminal;
pub mod web;
