//! Calculator commands.
use crate::core::app::EditorApp;
use crate::core::dispatcher::{DispatchResult, InputAction};

pub fn calculator(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Calc: ".to_string(),
            action: InputAction::Calculator,
        }
    }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::app::EditorApp;

    #[test]
    fn test_calculator_command() {
        let mut app = EditorApp::new();
        let result = calculator(&mut app, 1);

        match result {
            DispatchResult::NeedsInput { prompt, action } => {
                assert_eq!(prompt, "Calc: ");
                assert_eq!(action, InputAction::Calculator);
            }
            _ => panic!("Expected NeedsInput"),
        }
    }
}
