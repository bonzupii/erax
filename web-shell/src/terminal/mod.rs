//! Terminal screen-model types (wasm).

#[path = "../../../src/terminal/color.rs"]
pub mod color;

#[path = "../../../src/terminal/model.rs"]
pub mod model;

/// Shim: the protocol refers to `terminal::display::{Cell, CellRole}`;
/// on wasm those live in `terminal::model`.
pub mod display {
    pub use super::model::{Cell, CellRole, ScreenBuffer};
}
