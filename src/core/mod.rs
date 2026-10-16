//! This module constitutes the core, headless, and backend-agnostic editing engine of erax.
//! It manages fundamental editor components such as buffers, windows (logical views),
//! command dispatch, syntax parsing, and search functionality.

pub mod app;
pub mod bindings;
pub mod btext;
#[cfg(any(unix, windows))]
pub mod mmap;
pub mod buffer;
pub mod calculator;
pub mod command;
pub mod commands;
pub mod completion;
pub mod cue;
pub mod cursor_style;
pub mod diagnostics;
pub mod dispatcher;
pub mod encoding;
pub mod focus;
pub mod geometry;
pub mod hex;
pub mod id;
pub mod input;
pub mod keyfmt;
pub mod kill_ring;
pub mod layout;
pub mod lexer;
pub mod menu;
pub mod modal;
pub mod mouse;
pub mod path;
pub mod print;
pub mod prompt;
pub mod selection;
pub mod semantics;
pub mod snippets;
pub mod spell;
pub mod syntax;
pub mod terminal_host;
pub mod undo_group;
pub mod utf8;
pub mod window;
