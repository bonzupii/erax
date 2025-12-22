//! State for the minibuffer/prompt in terminal display mode.
//!
//! Note: Input handling has been moved to crate::core::focus to be shared
//! across TUI and GUI modes. This module now primarily supports rendering.

pub struct PromptState {
    pub prompt: String,
    pub input: String,
    pub cursor: usize,
}

impl PromptState {
    pub fn new(prompt: String) -> Self {
        Self {
            prompt,
            input: String::new(),
            cursor: 0,
        }
    }
}
