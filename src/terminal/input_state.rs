//! Input State Machine
//!
//! Centralizes input mode management for the terminal editor.
//! Replaces scattered conditional checks with a clean state machine pattern.

use crate::core::buffer::BufferKind;

/// The current input mode determining how key events are processed
#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    /// Normal editing mode - keys go through keybinding manager
    Normal,

    /// Focus state active (minibuffer, prompt, search, etc.)
    Focus {
        /// The target of the focus (used for dispatch after confirmation)
        target: FocusKind,
    },

    /// Menu bar is active and consuming navigation keys
    Menu {
        /// Currently active menu index
        active_menu: usize,
    },

    /// Special buffer mode with custom keybindings
    SpecialBuffer { kind: BufferKind },
}

/// Kind of focus for input routing
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FocusKind {
    Minibuffer,
    Calculator,
    GoToLine,
    ISearch,
    FindReplace,
    Other,
}

impl From<crate::core::focus::FocusTarget> for FocusKind {
    fn from(target: crate::core::focus::FocusTarget) -> Self {
        use crate::core::focus::FocusTarget;
        match target {
            FocusTarget::Calculator => FocusKind::Calculator,
            FocusTarget::Minibuffer => FocusKind::Minibuffer,
            FocusTarget::GoToLine => FocusKind::GoToLine,
            FocusTarget::ISearch => FocusKind::ISearch,
            FocusTarget::FindReplace => FocusKind::FindReplace,
            _ => FocusKind::Other,
        }
    }
}

/// Manages the current input mode and provides mode-based decision helpers
pub struct InputStateMachine {
    mode: InputMode,
}

impl Default for InputStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl InputStateMachine {
    /// Create a new state machine in Normal mode
    pub fn new() -> Self {
        Self {
            mode: InputMode::Normal,
        }
    }

    /// Get current input mode
    pub fn mode(&self) -> &InputMode {
        &self.mode
    }

    /// Update mode based on current app state
    /// Call this at the start of event processing to sync state
    pub fn sync_from_app(
        &mut self,
        focus_manager: &crate::core::focus::FocusManager,
        menu_open: bool,
        active_menu: usize,
        buffer_kind: Option<BufferKind>,
    ) {
        // Priority order: Focus > Menu > SpecialBuffer > Normal

        // 1. Check focus state first (highest priority)
        if let Some(focus) = focus_manager.current_state() {
            if focus.target.uses_minibuffer() {
                self.mode = InputMode::Focus {
                    target: focus.target.into(),
                };
                return;
            }
        }

        // 2. Check menu state
        if menu_open {
            self.mode = InputMode::Menu { active_menu };
            return;
        }

        // 3. Check special buffer
        if let Some(kind) = buffer_kind {
            match kind {
                BufferKind::Diagnostics
                | BufferKind::DiffOriginal
                | BufferKind::DiffModified
                | BufferKind::Terminal => {
                    self.mode = InputMode::SpecialBuffer { kind };
                    return;
                }
                _ => {}
            }
        }

        // 4. Default to normal
        self.mode = InputMode::Normal;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_state_machine_is_normal() {
        let sm = InputStateMachine::new();
        assert!(matches!(sm.mode(), InputMode::Normal));
    }

    #[test]
    fn test_mode_returns_current_mode() {
        let sm = InputStateMachine::new();
        assert_eq!(*sm.mode(), InputMode::Normal);
    }
}
