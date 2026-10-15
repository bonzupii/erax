//! Compile-time configuration: a user-friendly list of typed `pub const`s.
//!
//! Principle 1: configuration is permanent and compile-checked. Every setting
//! is a statically typed `pub const`; every keybinding is a typed
//! ([`KeyPress`], [`Command`]) pair. A wrong config is a **compile error** —
//! there is no runtime config file, no HashMap, no stringly keys, and no
//! `ConfigValue` type erasure (N-008).
//!
//! Per-area modules (N-270) so a change in one area does not require
//! re-merging the whole configuration:
//!
//! - [`editor`]: editing behavior (tab width, kill ring, undo, shell)
//! - [`display`]: display and rendering toggles
//! - [`font`]: GUI font stack (N-011, N-119)
//! - [`frontend`]: default frontend and WebUI server defaults
//! - [`theme`]: theme and truecolor selection (N-014)
//! - [`keys`]: keybindings and input model (N-008, N-010, N-013, N-268)
//!
//! To customize: edit the `pub const` values below and rebuild. An invalid
//! value (wrong type, unknown theme, unknown command) fails compilation.

pub mod display;
pub mod cursor_style;
pub mod editor;
pub mod font;
pub mod frontend;
pub mod keys;
pub mod theme;
