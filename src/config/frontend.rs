//! Frontend selection and server defaults: typed `pub const`s.
//!
//! Which interface erax starts, and where the WebUI server listens, are
//! compile-time decisions like everything else. Explicit CLI flags
//! (`-g`, `-u`, `-a`, `-s`, `--web-server`, `--web-bind`) override these
//! defaults per invocation; the override is a typed invocation parameter
//! and never mutates the constants.

use crate::cli::ShellMode;

/// Default frontend when no CLI flag selects one.
///
/// The native GUI: with a display reachable it opens the window; without
/// one the resolver degrades the preference to the best terminal rung
/// the environment supports (piped stdin: stream-editor mode), and the
/// shell reports the degradation on stderr. Explicit CLI flags
/// (`-g`, `-u`, `-a`, `-s`, `-e`/`-f`, `--web-server`) override this
/// default per invocation; the override is a typed invocation parameter
/// and never mutates the constant.
pub const SHELL_MODE: ShellMode = ShellMode::Gui;

/// Default bind address for `--web-server` (`ADDR:PORT`).
pub const WEB_BIND: &str = "127.0.0.1:8080";

/// Build-level acknowledgement for non-loopback `--web-bind`.
///
/// The per-invocation `--web-allow-remote` flag ORs with this: setting the
/// constant to `true` bakes the acknowledgement into the binary. It can
/// only enable, never disable. A security acknowledgement is not
/// something a default should silently take away.
pub const WEB_ALLOW_REMOTE: bool = false;
