use crate::core::app::EditorApp;
/// Search and navigation commands
use crate::core::command::Command;
use crate::core::dispatcher::{DispatchResult, InputAction};

/// Search forward for text (prompts)
#[derive(Clone)]
pub struct SearchForward;

impl Command for SearchForward {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Search: ".to_string(),
            action: InputAction::SearchForward,
        }
    }
}

/// Search backward for text (prompts)
#[derive(Clone)]
pub struct SearchBackward;

impl Command for SearchBackward {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Search backward: ".to_string(),
            action: InputAction::SearchBackward,
        }
    }
}

/// Query replace (prompts for pattern and replacement)
#[derive(Clone)]
pub struct QueryReplace;

impl Command for QueryReplace {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Query replace: ".to_string(),
            action: InputAction::QueryReplace,
        }
    }
}

/// Go to specific line number (prompts)
#[derive(Clone)]
pub struct GotoLine;

impl Command for GotoLine {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Goto line: ".to_string(),
            action: InputAction::GotoLine,
        }
    }
}
