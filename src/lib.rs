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
pub mod speech;
pub mod terminal;
pub mod web;

// The speech backend is enabled in config but the `speech` cargo
// feature is off: rebuild with `--features speech`
// (`accessibility.sh` passes it automatically for the speech
// profile). The const is the single source of truth; the feature is
// the mechanical linking gate.
#[cfg(speech_misconfigured)]
compile_error!(
    "SPEECH_BACKEND is not Off but the `speech` cargo feature is off; rebuild with --features speech"
);
