//! Compile-time configuration: a user-friendly list of typed `pub const`s.
//!
//! Principle 1: configuration is permanent and compile-checked. Every setting
//! is a statically typed `pub const`; every keybinding is a typed
//! ([`KeyPress`], [`Command`]) pair. A wrong config is a compile error:
//! there is no runtime config file, no HashMap, no stringly keys, and no
//! `ConfigValue` type erasure.
//!
//! Per-area modules, so a change in one area does not require re-merging
//! the whole configuration:
//!
//! - [`accessibility`]: accessibility behavior, one module per disability class
//! - [`display`]: display and rendering toggles
//! - [`editor`]: editing behavior (tab width, kill ring, undo, shell)
//! - [`effects`]: GUI shader effect toggles
//! - [`font`]: GUI font stack
//! - [`frontend`]: default frontend and WebUI server defaults
//! - [`keys`]: keybindings and input model
//! - [`scrollbar`]: scrollbar presentation and behavior
//! - [`theme`]: theme and truecolor selection
//!
//! To customize: edit the `pub const` values and rebuild. An invalid value
//! (wrong type, unknown theme, unknown command) fails compilation.

pub mod accessibility;
pub mod display;
pub mod editor;
pub mod effects;
pub mod font;
pub mod frontend;
pub mod keys;
pub mod scrollbar;
pub mod theme;
