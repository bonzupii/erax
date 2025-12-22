//! Spell suggestion commands

use crate::core::app::EditorApp;
use crate::core::command::Command;
use crate::core::dispatcher::DispatchResult;

/// Suggest spellings for word under cursor
#[derive(Clone)]
pub struct SpellSuggest;

impl Command for SpellSuggest {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        // Get word under cursor
        let (word, _start, _end) = {
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
            let content = buffer.to_string();
            let bytes = content.as_bytes();

            // Find word start
            let mut start = cursor_byte;
            while start > 0
                && bytes
                    .get(start - 1)
                    .map_or(false, |&b| b.is_ascii_alphanumeric())
            {
                start -= 1;
            }

            // Find word end
            let mut end = cursor_byte;
            while end < bytes.len() && bytes.get(end).map_or(false, |&b| b.is_ascii_alphanumeric())
            {
                end += 1;
            }

            if start == end {
                return DispatchResult::Info("No word at cursor".to_string());
            }

            let word = content[start..end].to_string();
            (word, start, end)
        };

        // This is a placeholder - the suggest() function returns empty for now
        // but this wires the API so it's not dead code
        let suggestions = app
            .buffers
            .values()
            .next()
            .map(|_| crate::core::spell::SpellChecker::new().suggest(&word))
            .unwrap_or_default();

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
}
