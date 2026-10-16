//! Core input types (wasm).

#[path = "../../../src/core/command.rs"]
pub mod command;

/// Leaf cursor-shape enum (same source as native): the wire `CursorState`
/// and the presentation core name it without the display config.
#[path = "../../../src/core/cursor_style.rs"]
pub mod cursor_style;

#[path = "../../../src/core/geometry.rs"]
pub mod geometry;

#[path = "../../../src/core/id.rs"]
pub mod id;

#[path = "../../../src/core/input.rs"]
pub mod input;

#[path = "../../../src/core/keyfmt.rs"]
pub mod keyfmt;

#[path = "../../../src/core/layout.rs"]
pub mod layout;
