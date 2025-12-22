//! Command Pattern implementation for the erax editor
//!
//! This module defines the `Command` trait, which all editor commands must implement.
//! The trait-based architecture enables:
//!
//! - **Decoupling**: Command logic is isolated from the dispatcher
//! - **Extensibility**: New commands can be added without modifying the dispatcher
//! - **Composability**: Commands can be composed (e.g., macros execute sequences)
//! - **Testability**: Commands can be tested in isolation

use crate::core::app::EditorApp;
use crate::core::dispatcher::DispatchResult;

/// Core command trait implementing the Command Pattern
///
/// All editor commands implement this trait. The `execute` method
/// receives the editor application state and a repetition count,
/// allowing commands to support uEmacs-style universal arguments.
///
/// # Parameters
/// - `app`: Mutable reference to editor state (buffers, windows, config, etc.)
/// - `count`: Repetition count (typically 1, can be higher via universal argument)
///
/// # Returns
/// `DispatchResult` indicating command success, failure, or special handling needs
pub trait Command: Send + Sync + CloneCommand {
    /// Execute the command with the given repetition count
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult;
}

/// Helper trait for cloning boxed commands
/// This trait is automatically implemented for all Command types
pub trait CloneCommand {
    /// Create a boxed clone of this command
    fn clone_box(&self) -> Box<dyn Command>;
}

impl<T> CloneCommand for T
where
    T: 'static + Command + Clone,
{
    fn clone_box(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn Command> {
    fn clone(&self) -> Box<dyn Command> {
        self.as_ref().clone_box()
    }
}
