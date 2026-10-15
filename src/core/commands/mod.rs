//! Command implementations for the erax editor.
//!
//! This module contains all editor commands organized into logical sub-modules:
//!
//! - **movement**: Cursor navigation (forward-char, next-line, forward-word, etc.)
//! - **marks**: Mark management and region operations (set-mark, kill-region, etc.)
//! - **editing**: Basic text editing (insert-newline, delete-char, etc.)
//! - **window**: Window management (split-window, delete-window, etc.)
//! - **file**: File operations (save-buffer, find-file, kill-buffer, etc.)
//! - **search**: Search and navigation (search-forward, goto-line, etc.)
//! - **kill_ring**: Kill ring operations (kill-line, yank, transpose-words, etc.)
//! - **macro_cmd**: Macro recording and playback (begin-macro, execute-macro, etc.)
//! - **buffer**: Buffer introspection (buffer-info, count-words, etc.)
//! - **text**: Text transformation (justify-paragraph, upper-word, etc.)
//! - **control**: Application control (exit, etc.)
//!
//! All commands implement the [`Command`](crate::core::command::Command) trait,
//! which defines a uniform interface for command execution.

/// Buffer introspection
pub mod buffer;
/// Programming calculator
pub mod calculator;
/// Word completion
pub mod completion;
/// Application control
pub mod control;
/// Diagnostics pane
pub mod diagnostics;
/// Diff and Sed preview
pub mod diff;
/// Basic editing (insert, delete)
pub mod editing;
/// Expand selection to syntactic units
pub mod expand_selection;
/// File operations
pub mod file;
/// Kill ring and yank
pub mod kill_ring;
/// Macro recording
pub mod macro_cmd;
/// Mark and region commands
pub mod marks;
/// Cursor movement commands
pub mod movement;
/// Printing commands
pub mod print;
/// Search and navigation
pub mod search;
/// Code snippets
pub mod snippets;
/// Spell suggestions
pub mod spell_suggest;
/// Terminal emulation
pub mod terminal;
/// Text transformation
pub mod text;
/// Undo/redo operations
pub mod undo;
/// Window management
pub mod window;
