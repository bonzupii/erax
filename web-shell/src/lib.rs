//! erax web shell (wasm32): the presentation core in the browser.
//!
//! The presentation core (`gui::core`) and the wire protocol (`web::protocol`)
//! are the SAME source files as the native build, included via `#[path]` —
//! no second representation, no drift. The DOM glue (`gui::shell::web`) is the
//! only web-specific code.

pub mod terminal;
pub mod core;
pub mod config;
pub mod gui;
pub mod web;
