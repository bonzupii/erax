//! Focus-target state machine for input routing.
//!
//! Manages input focus routing between different editor components.
//! Enables the minibuffer, completion menu, calculator, and other
//! overlays to intercept keyboard input.

// FocusManager is in EditorApp - full input routing TBD

use std::fmt;

use crate::core::id::BufferId;

/// Map a prompt-requesting input action to its focus target: the single
/// rule for entering prompt mode, used by the TUI event loop and the
/// modal engine alike.
pub fn focus_target_for_action(action: crate::core::dispatcher::InputAction) -> FocusTarget {
    use crate::core::dispatcher::InputAction;
    match action {
        InputAction::Calculator => FocusTarget::Calculator,
        InputAction::GotoLine => FocusTarget::GoToLine,
        InputAction::SearchForward => FocusTarget::ISearch,
        InputAction::QueryReplace => FocusTarget::FindReplace,
        _ => FocusTarget::Minibuffer,
    }
}

/// Map a [`DispatchResult::AwaitKey`](crate::core::dispatcher::DispatchResult::AwaitKey)
/// action to its focus target: the single rule for the one-more-keypress
/// state, used by the TUI event loop, the modal engine, and the prompt
/// handler alike so the three cannot drift.
pub fn focus_target_for_await_key(action: crate::core::dispatcher::InputAction) -> FocusTarget {
    use crate::core::dispatcher::InputAction;
    match action {
        InputAction::DescribeKey => FocusTarget::DescribeKey,
        InputAction::QuoteCharacter => FocusTarget::QuoteCharacter,
        _ => FocusTarget::Editor,
    }
}

// =============================================================================
// FOCUS TARGET ENUM
// =============================================================================

/// The current focus target for keyboard input
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusTarget {
    /// Normal text editing in the active buffer
    #[default]
    Editor,
    /// Menu bar is open (dropdown menus)
    Menu,
    /// Minibuffer for commands (find-file, save, etc.)
    Minibuffer,
    /// Completion popup menu
    CompletionMenu,
    /// Calculator input prompt
    Calculator,
    /// Incremental search
    ISearch,
    /// Go to line prompt
    GoToLine,
    /// Find/Replace prompt
    FindReplace,
    /// Help/documentation overlay
    Help,
    /// Waiting for key (describe-key)
    DescribeKey,
    /// Waiting for one keypress to insert literally (quote-character)
    QuoteCharacter,
    /// Yes/no answer to "file changed on disk, reload?". One keypress
    /// answers; the state carries the buffer the question is about.
    ConfirmReload,
}

impl FocusTarget {
    /// Returns true if this focus allows ESC to cancel
    pub fn is_cancellable(&self) -> bool {
        !matches!(self, FocusTarget::Editor)
    }

    /// Returns true if this focus shows a minibuffer prompt
    pub fn uses_minibuffer(&self) -> bool {
        matches!(
            self,
            FocusTarget::Minibuffer
                | FocusTarget::GoToLine
                | FocusTarget::FindReplace
                | FocusTarget::ISearch
                | FocusTarget::Calculator
                | FocusTarget::ConfirmReload
        )
    }

    /// Returns true if this focus shows a popup menu
    pub fn shows_popup(&self) -> bool {
        matches!(self, FocusTarget::CompletionMenu | FocusTarget::Help)
    }

    /// Returns true if the event loop routes keypresses to the focus
    /// handler for this target: the minibuffer prompts and the
    /// one-keypress states (describe-key, quote-character). Other
    /// targets are served elsewhere, so routing them here would swallow
    /// their keys.
    pub fn routes_keypress_to_focus(&self) -> bool {
        self.uses_minibuffer()
            || matches!(
                self,
                FocusTarget::DescribeKey | FocusTarget::QuoteCharacter
            )
    }
}

impl fmt::Display for FocusTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FocusTarget::Editor => write!(f, "Editor"),
            FocusTarget::Menu => write!(f, "Menu"),
            FocusTarget::Minibuffer => write!(f, "Minibuffer"),
            FocusTarget::CompletionMenu => write!(f, "Completion"),
            FocusTarget::Calculator => write!(f, "Calculator"),
            FocusTarget::ISearch => write!(f, "I-Search"),
            FocusTarget::GoToLine => write!(f, "Go To Line"),
            FocusTarget::FindReplace => write!(f, "Find/Replace"),
            FocusTarget::Help => write!(f, "Help"),
            FocusTarget::DescribeKey => write!(f, "Describe Key"),
            FocusTarget::QuoteCharacter => write!(f, "Quote Character"),
            FocusTarget::ConfirmReload => write!(f, "Confirm Reload"),
        }
    }
}

/// Context a focus state carries that a text input cannot express.
/// Kept as data on the state (not in a side table) so the question and
/// its subject can never disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPayload {
    /// A pending "file changed on disk" question for this buffer.
    ReloadConfirm(BufferId),
}

// =============================================================================
// FOCUS STATE
// =============================================================================

/// Result of a focus operation
#[derive(Debug, Clone, PartialEq)]
pub enum FocusResult {
    /// Continue editing
    Continue,
    /// User confirmed the input
    Confirmed(String),
    /// User cancelled
    Cancelled,
    /// Selection made from menu
    Selected(usize),
}

/// State for an active focus operation
pub struct FocusState {
    /// The current focus target
    pub target: FocusTarget,
    /// The prompt text (for minibuffer-style inputs)
    pub prompt: String,
    /// The current input value
    pub input: String,
    /// Cursor position within input
    pub cursor: usize,
    /// Items for selection (completion menu, etc.)
    pub items: Vec<String>,
    /// Selected item index
    pub selected: usize,
    /// History for this focus type
    pub history: Vec<String>,
    /// History navigation index
    pub history_index: Option<usize>,
    /// Non-text context for targets that need it (e.g. which buffer a
    /// reload question is about). None for plain text prompts.
    pub payload: Option<FocusPayload>,
}

impl FocusState {
    /// Create a new focus state
    pub fn new(target: FocusTarget, prompt: &str) -> Self {
        Self {
            target,
            prompt: prompt.to_string(),
            input: String::new(),
            cursor: 0,
            items: Vec::new(),
            selected: 0,
            history: Vec::new(),
            history_index: None,
            payload: None,
        }
    }

    /// Create a focus state with initial input
    pub fn with_input(target: FocusTarget, prompt: &str, input: &str) -> Self {
        let cursor = input.len();
        Self {
            target,
            prompt: prompt.to_string(),
            input: input.to_string(),
            cursor,
            items: Vec::new(),
            selected: 0,
            history: Vec::new(),
            history_index: None,
            payload: None,
        }
    }

    /// Create a focus state with items for selection
    pub fn with_items(target: FocusTarget, prompt: &str, items: Vec<String>) -> Self {
        Self {
            target,
            prompt: prompt.to_string(),
            input: String::new(),
            cursor: 0,
            items,
            selected: 0,
            history: Vec::new(),
            history_index: None,
            payload: None,
        }
    }

    /// Create the reload-confirm state for this buffer. The only way to
    /// build a [`FocusTarget::ConfirmReload`] state, so the question always
    /// carries its subject.
    pub fn for_reload(buffer_id: BufferId, prompt: &str) -> Self {
        Self {
            target: FocusTarget::ConfirmReload,
            prompt: prompt.to_string(),
            input: String::new(),
            cursor: 0,
            items: Vec::new(),
            selected: 0,
            history: Vec::new(),
            history_index: None,
            payload: Some(FocusPayload::ReloadConfirm(buffer_id)),
        }
    }

    /// The buffer a [`FocusTarget::ConfirmReload`] state is asking about.
    pub fn reload_buffer_id(&self) -> Option<BufferId> {
        match self.payload {
            Some(FocusPayload::ReloadConfirm(id)) => Some(id),
            _ => None,
        }
    }

    /// One keypress answers the reload question. `y` reloads; every other
    /// way out (`n`, Enter, Esc, C-g) declines: Enter never reloads, so no
    /// stray keypress can trigger one. Any other key is ignored and the
    /// question stays up. The answer travels as `Confirmed("y"|"n")`, the
    /// same target-specific payload idiom the text prompts use.
    fn confirm_reload_key(&mut self, event: &crate::core::input::KeyPress) -> FocusResult {
        use crate::core::input::Key;
        let plain = |c: char| event.key == Key::Char(c) && !event.ctrl && !event.alt;
        if plain('y') || plain('Y') {
            return FocusResult::Confirmed("y".to_string());
        }
        if plain('n')
            || plain('N')
            || matches!(&event.key, Key::Enter | Key::Esc)
            || (event.key == Key::Char('g') && event.ctrl && !event.alt)
        {
            return FocusResult::Confirmed("n".to_string());
        }
        FocusResult::Continue
    }

    /// Process a key event in the focused component.
    ///
    /// Matches on the typed [`KeyPress`]: base key plus modifiers (N-056).
    /// Ctrl combinations are `ctrl: true` with a base `Key::Char`, not a
    /// separate key variant.
    pub fn handle_key(&mut self, event: &crate::core::input::KeyPress) -> FocusResult {
        use crate::core::input::Key;
        if self.target == FocusTarget::ConfirmReload {
            return self.confirm_reload_key(event);
        }
        let ctrl = |c: char| event.key == Key::Char(c) && event.ctrl && !event.alt;
        match &event.key {
            Key::Enter => FocusResult::Confirmed(self.input.clone()),
            Key::Esc => FocusResult::Cancelled,
            _ if ctrl('g') => FocusResult::Cancelled,
            Key::Backspace => {
                self.delete_backward();
                FocusResult::Continue
            }
            Key::Delete => {
                self.delete_forward();
                FocusResult::Continue
            }
            Key::Left => {
                self.cursor_left();
                FocusResult::Continue
            }
            Key::Right => {
                self.cursor_right();
                FocusResult::Continue
            }
            Key::Home => {
                self.cursor_home();
                FocusResult::Continue
            }
            _ if ctrl('a') => {
                self.cursor_home();
                FocusResult::Continue
            }
            Key::End => {
                self.cursor_end();
                FocusResult::Continue
            }
            _ if ctrl('e') => {
                self.cursor_end();
                FocusResult::Continue
            }
            Key::Up => {
                self.history_prev();
                FocusResult::Continue
            }
            _ if ctrl('p') => {
                self.history_prev();
                FocusResult::Continue
            }
            Key::Down => {
                self.history_next();
                FocusResult::Continue
            }
            _ if ctrl('n') => {
                self.history_next();
                FocusResult::Continue
            }
            Key::Char(c) if !event.ctrl && !event.alt => {
                self.insert_char(*c);
                FocusResult::Continue
            }
            _ => FocusResult::Continue,
        }
    }

    /// Insert a character at cursor
    pub fn insert_char(&mut self, ch: char) {
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    /// Delete character before cursor (backspace)
    pub fn delete_backward(&mut self) {
        if self.cursor > 0
            && let Some(prev_char) = self.input[..self.cursor].chars().last() {
                self.cursor -= prev_char.len_utf8();
                self.input.remove(self.cursor);
            }
    }

    /// Delete character at cursor (delete)
    pub fn delete_forward(&mut self) {
        if self.cursor < self.input.len() {
            self.input.remove(self.cursor);
        }
    }

    /// Move cursor left
    pub fn cursor_left(&mut self) {
        if self.cursor > 0
            && let Some(prev_char) = self.input[..self.cursor].chars().last() {
                self.cursor -= prev_char.len_utf8();
            }
    }

    /// Move cursor right
    pub fn cursor_right(&mut self) {
        if self.cursor < self.input.len()
            && let Some(next_char) = self.input[self.cursor..].chars().next() {
                self.cursor += next_char.len_utf8();
            }
    }

    /// Move cursor to start
    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to end
    pub fn cursor_end(&mut self) {
        self.cursor = self.input.len();
    }

    /// Clear the input
    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor = 0;
    }

    /// Select next item in menu
    pub fn select_next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1) % self.items.len();
        }
    }

    /// Select previous item in menu
    pub fn select_prev(&mut self) {
        if !self.items.is_empty() {
            self.selected = match self.selected.checked_sub(1) {
                Some(idx) => idx,
                None => self.items.len().saturating_sub(1),
            };
        }
    }

    /// Navigate to previous history entry
    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let idx = match self.history_index {
            Some(i) if i + 1 < self.history.len() => i + 1,
            Some(i) => i,
            None => 0,
        };
        self.history_index = Some(idx);
        self.input = self.history[self.history.len() - 1 - idx].clone();
        self.cursor = self.input.len();
    }

    /// Navigate to next history entry
    pub fn history_next(&mut self) {
        match self.history_index {
            Some(0) => {
                self.history_index = None;
                self.input.clear();
                self.cursor = 0;
            }
            Some(i) => {
                self.history_index = Some(i - 1);
                self.input = self.history[self.history.len() - i].clone();
                self.cursor = self.input.len();
            }
            None => {}
        }
    }

    /// Add current input to history
    pub fn add_to_history(&mut self) {
        if !self.input.is_empty() {
            // Remove duplicate if exists
            self.history.retain(|h| h != &self.input);
            self.history.push(self.input.clone());
        }
    }
}

// =============================================================================
// FOCUS MANAGER
// =============================================================================

/// Manages the focus state machine
#[derive(Default)]
pub struct FocusManager {
    /// Stack of focus states (allows nested focus)
    stack: Vec<FocusState>,
}

impl FocusManager {
    /// Create a new focus manager
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Get the current focus target
    pub fn current_target(&self) -> FocusTarget {
        match self.stack.last() {
            Some(state) => state.target,
            None => FocusTarget::Editor,
        }
    }

    /// Get the current focus state (if any)
    pub fn current_state(&self) -> Option<&FocusState> {
        self.stack.last()
    }

    /// Get the current focus state mutably (if any)
    pub fn current_state_mut(&mut self) -> Option<&mut FocusState> {
        self.stack.last_mut()
    }

    /// Check if focus is on editor
    pub fn is_editor(&self) -> bool {
        self.current_target() == FocusTarget::Editor
    }

    /// Push a new focus target
    pub fn push(&mut self, state: FocusState) {
        self.stack.push(state);
    }

    /// Pop the current focus (returns to previous or editor)
    pub fn pop(&mut self) -> Option<FocusState> {
        self.stack.pop()
    }

    /// Pop all focus states (return to editor)
    pub fn pop_all(&mut self) {
        self.stack.clear();
    }

    /// Begin minibuffer input
    pub fn begin_minibuffer(&mut self, prompt: &str) {
        self.push(FocusState::new(FocusTarget::Minibuffer, prompt));
    }

    /// Begin calculator input
    pub fn begin_calculator(&mut self) {
        self.push(FocusState::new(FocusTarget::Calculator, "Calc: "));
    }

    /// Begin incremental search
    pub fn begin_isearch(&mut self, forward: bool) {
        let prompt = if forward {
            "I-search: "
        } else {
            "I-search backward: "
        };
        self.push(FocusState::new(FocusTarget::ISearch, prompt));
    }

    /// Begin go-to-line
    pub fn begin_goto_line(&mut self) {
        self.push(FocusState::new(FocusTarget::GoToLine, "Go to line: "));
    }

    /// Begin completion menu
    pub fn begin_completion(&mut self, items: Vec<String>) {
        self.push(FocusState::with_items(
            FocusTarget::CompletionMenu,
            "",
            items,
        ));
    }

    /// Cancel current focus and return result
    pub fn cancel(&mut self) -> FocusResult {
        self.pop();
        FocusResult::Cancelled
    }

    /// True when a reload-confirm question for this buffer is already on
    /// the stack: asking twice for the same on-disk state is noise.
    pub fn has_reload_question(&self, buffer_id: BufferId) -> bool {
        self.stack
            .iter()
            .any(|s| s.target == FocusTarget::ConfirmReload && s.reload_buffer_id() == Some(buffer_id))
    }

    /// Confirm current focus and return result
    pub fn confirm(&mut self) -> FocusResult {
        if let Some(mut state) = self.pop() {
            state.add_to_history();
            if state.target.shows_popup() {
                FocusResult::Selected(state.selected)
            } else {
                FocusResult::Confirmed(state.input)
            }
        } else {
            FocusResult::Cancelled
        }
    }
}

// =============================================================================
// RELOAD CONFIRM
// =============================================================================

/// Push the reload-confirm focus state for `buffer_id`, unless the same
/// question is already pending: the single rule for entering the
/// reload-confirm, used by the TUI event loop, the modal engine, and the
/// prompt handler alike so the three cannot drift. Questions nest on the
/// focus stack (LIFO); each prompt names its buffer, and a buffer never
/// gets two pending questions for the same on-disk state.
pub fn push_reload_question(app: &mut crate::core::app::EditorApp, buffer_id: BufferId) {
    if app.focus_manager.has_reload_question(buffer_id) {
        return;
    }
    let prompt = reload_prompt(app, buffer_id);
    app.focus_manager
        .push(FocusState::for_reload(buffer_id, &prompt));
}

/// The question text: names the file, and names the consequence when the
/// buffer holds unsaved edits, so `y` never destroys work the prompt did
/// not warn about.
fn reload_prompt(app: &crate::core::app::EditorApp, buffer_id: BufferId) -> String {
    let (name, modified) = app
        .buffers
        .get(&buffer_id)
        .map(|b| (b.display_name(), b.modified))
        .unwrap_or_else(|| ("<closed>".to_string(), false));
    if modified {
        format!(
            "File changed on disk; '{name}' has unsaved edits. Reload and lose the edits? (y/n)"
        )
    } else {
        format!("File changed on disk. Reload '{name}'? (y/n)")
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focus_target_properties() {
        assert!(!FocusTarget::Editor.is_cancellable());
        assert!(FocusTarget::Minibuffer.is_cancellable());
        assert!(FocusTarget::Calculator.is_cancellable());

        assert!(FocusTarget::Minibuffer.uses_minibuffer());
        assert!(FocusTarget::Calculator.uses_minibuffer());
        assert!(!FocusTarget::Editor.uses_minibuffer());

        assert!(FocusTarget::CompletionMenu.shows_popup());
        assert!(!FocusTarget::Minibuffer.shows_popup());
    }

    #[test]
    fn test_focus_state_input() {
        let mut state = FocusState::new(FocusTarget::Minibuffer, "Test: ");

        state.insert_char('h');
        state.insert_char('e');
        state.insert_char('l');
        state.insert_char('l');
        state.insert_char('o');
        assert_eq!(state.input, "hello");
        assert_eq!(state.cursor, 5);

        state.cursor_left();
        state.cursor_left();
        assert_eq!(state.cursor, 3);

        state.insert_char('X');
        assert_eq!(state.input, "helXlo");

        state.delete_backward();
        assert_eq!(state.input, "hello");
    }

    #[test]
    fn reload_key_answers_yes_no_or_ignores() {
        use crate::core::input::{Key, KeyPress};
        let mut s = FocusState::for_reload(BufferId(0), "Reload? (y/n)");
        assert_eq!(s.reload_buffer_id(), Some(BufferId(0)));
        assert!(matches!(
            s.handle_key(&KeyPress::plain(Key::Char('y'))),
            FocusResult::Confirmed(a) if a == "y"
        ));
        assert!(matches!(
            s.handle_key(&KeyPress::plain(Key::Char('Y'))),
            FocusResult::Confirmed(a) if a == "y"
        ));
        for key in [
            KeyPress::plain(Key::Char('n')),
            KeyPress::plain(Key::Char('N')),
            KeyPress::plain(Key::Enter),
            KeyPress::plain(Key::Esc),
            KeyPress::ctrl(Key::Char('g')),
        ] {
            assert!(
                matches!(s.handle_key(&key), FocusResult::Confirmed(a) if a == "n"),
                "declines on {key:?}"
            );
        }
        // Stray keys (including modified y) leave the question up.
        assert!(matches!(
            s.handle_key(&KeyPress::plain(Key::Char('x'))),
            FocusResult::Continue
        ));
        assert!(matches!(
            s.handle_key(&KeyPress::ctrl(Key::Char('y'))),
            FocusResult::Continue
        ));
    }

    #[test]
    fn reload_question_dedups_per_buffer() {
        let mut m = FocusManager::new();
        assert!(!m.has_reload_question(BufferId(3)));
        m.push(FocusState::for_reload(BufferId(3), "Reload? (y/n)"));
        assert!(m.has_reload_question(BufferId(3)));
        assert!(!m.has_reload_question(BufferId(4)));
    }

    #[test]
    fn test_focus_manager_stack() {
        let mut manager = FocusManager::new();
        assert!(manager.is_editor());

        manager.begin_minibuffer("Find: ");
        assert_eq!(manager.current_target(), FocusTarget::Minibuffer);

        manager.begin_completion(vec!["foo".into(), "bar".into()]);
        assert_eq!(manager.current_target(), FocusTarget::CompletionMenu);

        manager.pop();
        assert_eq!(manager.current_target(), FocusTarget::Minibuffer);

        manager.pop();
        assert!(manager.is_editor());
    }

    #[test]
    fn test_focus_history() {
        let mut state = FocusState::new(FocusTarget::Minibuffer, "Test: ");
        state.history = vec!["first".into(), "second".into(), "third".into()];

        state.history_prev();
        assert_eq!(state.input, "third");

        state.history_prev();
        assert_eq!(state.input, "second");

        state.history_next();
        assert_eq!(state.input, "third");
    }

    #[test]
    fn test_menu_selection() {
        let mut state = FocusState::with_items(
            FocusTarget::CompletionMenu,
            "",
            vec!["apple".into(), "banana".into(), "cherry".into()],
        );
        assert_eq!(state.selected, 0);

        state.select_next();
        assert_eq!(state.selected, 1);

        state.select_next();
        assert_eq!(state.selected, 2);

        state.select_next();
        assert_eq!(state.selected, 0); // Wraps around

        state.select_prev();
        assert_eq!(state.selected, 2);
    }
}
