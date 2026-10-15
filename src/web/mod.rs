//! WebUI: VS Code-server-style remote frontend for erax.
//!
//! Architecture (owner's design): Core > TUI > GUI > WebUI. The WebUI is a
//! CHILD of the GUI. In server deployment, Core+TUI run server-side; the
//! GUI presentation core runs client-side in wasm and renders the screen
//! model streamed over the wire.
//!
//! This module holds:
//! - `protocol`: the typed, versioned wire protocol. Frames mirror the
//!   internal screen-model types (`Cell`, `CellRole`, `KeyPress`,
//!   `MouseEvent`) — there is no second representation of editor state.
//!   Dirty regions + semantic metadata go out; raw input comes in.
//! - `server`: the opt-in WebSocket server (127.0.0.1 default, per-session
//!   random token REQUIRED, no unauthenticated mode).
//! - `entropy`: the platform-neutral session token generator (no
//!   `/dev/urandom`, no platform syscalls; in-tree SHA-256 and ChaCha20).
//!
//! The GUI is a dumb passthrough: all input models and mode-aware UI live
//! in the backend screen model. The web shell inherits them iff the split
//! is honest — the protocol carries raw input events and the backend's
//! screen model, nothing else.

pub mod entropy;
pub mod protocol;
pub mod server;
