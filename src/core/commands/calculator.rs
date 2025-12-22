use crate::core::app::EditorApp;
use crate::core::command::Command;
use crate::core::dispatcher::{DispatchResult, InputAction};

/// Programming calculator command
#[derive(Clone)]
pub struct CalculatorCommand;

impl Command for CalculatorCommand {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Calc: ".to_string(),
            action: InputAction::Calculator,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::app::EditorApp;

    #[test]
    fn test_calculator_command() {
        let mut app = EditorApp::new();
        let cmd = CalculatorCommand;
        let result = cmd.execute(&mut app, 1);

        match result {
            DispatchResult::NeedsInput { prompt, action } => {
                assert_eq!(prompt, "Calc: ");
                assert_eq!(action, InputAction::Calculator);
            }
            _ => panic!("Expected NeedsInput"),
        }
    }
}
