//! Spell-suggestion commands.

use crate::core::app::EditorApp;
use crate::core::dispatcher::DispatchResult;
use crate::core::spell::SpellChecker;

pub fn spell_suggest(app: &mut EditorApp, _count: usize) -> DispatchResult {
        // Get word under cursor
        let word = {
            let window = match app.active_window_ref() {
                Some(w) => w,
                None => return DispatchResult::Info("No active window".to_string()),
            };
            let buffer = match app.active_buffer() {
                Some(b) => b,
                None => return DispatchResult::Info("No active buffer".to_string()),
            };

            let cursor_byte = match window.get_byte_offset(buffer) {
                Some(b) => b,
                None => return DispatchResult::Info("Invalid cursor position".to_string()),
            };

            // Find word boundaries
            let content = buffer.content_string();
            let bytes = content.as_bytes();

            // Find word start
            let mut start = cursor_byte;
            while start > 0
                && bytes
                    .get(start - 1)
                    .is_some_and(|&b| b.is_ascii_alphanumeric())
            {
                start -= 1;
            }

            // Find word end
            let mut end = cursor_byte;
            while end < bytes.len() && bytes.get(end).is_some_and(|&b| b.is_ascii_alphanumeric())
            {
                end += 1;
            }

            if start == end {
                return DispatchResult::Info("No word at cursor".to_string());
            }

            // start/end sit on ASCII alphanumeric boundaries, so this is a
            // char boundary.
            content[start..end].to_string()
        };

        // SpellChecker::new is cheap: every instance shares the one
        // process-wide dictionary by reference.
        let checker = SpellChecker::new();

        if checker.check(&word) {
            return DispatchResult::Info(format!("'{}' is spelled correctly", word));
        }

        let suggestions = checker.suggest(&word);
        if suggestions.is_empty() {
            DispatchResult::Info(format!("No suggestions for '{}'", word))
        } else {
            DispatchResult::Info(format!(
                "Suggestions for '{}': {}",
                word,
                suggestions.join(", ")
            ))
        }
    }
