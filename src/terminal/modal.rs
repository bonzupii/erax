//! Terminal glue for the vim modal engine (N-268).
//!
//! The engine itself lives in [`crate::core::modal`] (it owns no display
//! state). This module routes a keypress through the engine — taking it out of
//! `EditorApp` so dispatch can borrow the app — and reports the outcome.
//!
//! Mode UI state (mode segment, pending keys, the `:` command line, cursor
//! style) is deliberately NOT pushed to the display here: the screen model
//! (`Display::render`) derives it from the backend engine every frame, so
//! renderers only ever see backend-derived state (N-273/D6). The `display`
//! parameter stays because the (frozen) event-handler call site passes it.

use crate::core::app::EditorApp;
use crate::core::input::KeyPress;
use crate::core::modal::ModalOutcome;
use crate::terminal::display::Display;

/// Route one keypress through the modal engine when it is active.
pub fn handle_modal_key(
    app: &mut EditorApp,
    _display: &mut Display,
    key: &KeyPress,
) -> ModalOutcome {
    let mut engine = std::mem::take(&mut app.modal_engine);
    let outcome = engine.handle_key(app, key);
    app.modal_engine = engine;
    outcome
}
