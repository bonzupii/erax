//! Search commands (incremental and non-incremental).
use crate::core::app::EditorApp;
/// Search and navigation commands
use crate::core::dispatcher::{DispatchResult, InputAction};

pub fn search_forward(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Search: ".to_string(),
            action: InputAction::SearchForward,
        }
    }

pub fn search_reverse(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Search backward: ".to_string(),
            action: InputAction::SearchBackward,
        }
    }

pub fn query_replace(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Query replace: ".to_string(),
            action: InputAction::QueryReplace,
        }
    }

pub fn goto_line(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Goto line: ".to_string(),
            action: InputAction::GotoLine,
        }
    }
