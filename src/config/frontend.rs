//! Frontend selection and server defaults: typed `pub const`s.
//!
//! Which interface erax starts, and where the WebUI server listens, are
//! compile-time decisions like everything else. Explicit CLI flags
//! (`-g`, `-u`, `-a`, `-s`, `--web-server`, `--web-bind`) override these
//! defaults per invocation; the override is a typed invocation parameter
//! and never mutates the constants.

/// Which frontend erax starts when no CLI mode flag selects one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellMode {
    /// Terminal UI; piped stdin still selects stream-editor mode, as with
    /// no configuration at all.
    Tui,
    /// UTF-8 terminal UI (full color per environment detection).
    Utf8,
    /// WebUI server: Core+TUI run headless here, the presentation core
    /// runs in the browser. Requires `--web-allow-remote` (or
    /// [`WEB_ALLOW_REMOTE`]) for non-loopback binds, as always.
    WebUi,
}

/// Default frontend when no CLI flag selects one.
pub const SHELL_MODE: ShellMode = ShellMode::Tui;

/// Default bind address for `--web-server` (`ADDR:PORT`).
pub const WEB_BIND: &str = "127.0.0.1:8080";

/// Build-level acknowledgement for non-loopback `--web-bind`.
///
/// The per-invocation `--web-allow-remote` flag ORs with this: setting the
/// constant to `true` bakes the acknowledgement into the binary. It can
/// only enable, never disable — a security acknowledgement is not
/// something a default should silently take away.
pub const WEB_ALLOW_REMOTE: bool = false;
