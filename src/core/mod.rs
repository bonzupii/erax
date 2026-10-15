//! This module constitutes the core, headless, and backend-agnostic editing engine of erax.
//! It manages fundamental editor components such as buffers, windows (logical views),
//! command dispatch, syntax parsing, and search functionality.

pub mod app;
pub mod btext;
#[cfg(unix)]
pub mod mmap;
pub mod buffer;
pub mod calculator;
pub mod command;
pub mod commands;
pub mod completion;
pub mod diagnostics;
pub mod dispatcher;
pub mod encoding;
pub mod focus;
pub mod geometry;
pub mod hex;
pub mod id;
pub mod input;
pub mod input_router;
pub mod kill_ring;
pub mod layout;
pub mod lexer;
pub mod menu;
pub mod modal;
pub mod mouse;
pub mod print;
pub mod prompt;
pub mod selection;
pub mod snippets;
pub mod spell;
pub mod syntax;
pub mod terminal_host;
pub mod undo_group;
pub mod utf8;
pub mod window;
