//! Word Completion Commands

use crate::core::app::EditorApp;
use crate::core::command::Command;
use crate::core::dispatcher::DispatchResult;

/// Buffer-local word completion command (M-/)
#[derive(Clone, Debug)]
pub struct WordCompletion;

impl Command for WordCompletion {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let (cursor_byte, prefix_len) = {
            let window = match app.active_window_ref() {
                Some(w) => w,
                None => {
                    app.message = Some("No active window".to_string());
                    return DispatchResult::Success;
                }
            };
            let buffer = match app.active_buffer() {
                Some(b) => b,
                None => {
                    app.message = Some("No active buffer".to_string());
                    return DispatchResult::Success;
                }
            };

            let byte_offset = match window.get_byte_offset(buffer) {
                Some(offset) => offset,
                None => {
                    app.message = Some("Could not determine cursor position".to_string());
                    return DispatchResult::Success;
                }
            };

            let content = buffer.to_string();
            let prefix_start = match content[..byte_offset]
                .char_indices()
                .rev()
                .take_while(|(_, c)| c.is_ascii_alphanumeric() || *c == '_')
                .last()
                .map(|(i, _)| i)
            {
                Some(i) => i,
                None => byte_offset,
            };

            (byte_offset, byte_offset - prefix_start)
        };

        if prefix_len == 0 {
            app.message = Some("No word prefix at cursor".to_string());
            return DispatchResult::Success;
        }

        let completions = {
            let Some(window) = app.windows.get(&app.active_window) else {
                return DispatchResult::Success;
            };
            let buffer_id = window.buffer_id;
            let Some(buffer) = app.buffers.get(&buffer_id) else {
                return DispatchResult::Success;
            };
            app.word_completer
                .complete_at_cursor(buffer, buffer_id, cursor_byte)
        };

        if completions.is_empty() {
            app.message = Some("No completions found".to_string());
            return DispatchResult::Success;
        }

        // For now, we take the first completion
        let completion = &completions[0];
        let prefix_start = cursor_byte - prefix_len;

        if let Some(buffer) = app.active_buffer_mut() {
            // Delete the prefix and insert completion
            buffer.delete(prefix_start, prefix_len);
            buffer.insert(prefix_start, completion);

            // Move cursor to end of completion
            app.goto_byte(prefix_start + completion.len());
        }

        if completions.len() > 1 {
            app.message = Some(format!(
                "Completed: {} ({} more)",
                completion,
                completions.len() - 1
            ));
        } else {
            app.message = Some(format!("Completed: {}", completion));
        }

        DispatchResult::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::app::EditorApp;

    #[test]
    fn test_word_completion() {
        let mut app = EditorApp::new();
        let buffer_id = match app.active_window_ref() {
            Some(w) => w.buffer_id,
            None => panic!("No active window"),
        };

        {
            if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                buffer.insert(0, "function_one function_two func");
            }
        }

        // Move cursor to end of "func"
        app.goto_byte(30);

        let cmd = WordCompletion;
        let result = cmd.execute(&mut app, 1);

        assert!(matches!(result, DispatchResult::Success));

        if let Some(buffer) = app.active_buffer() {
            let content = buffer.to_string();

            // Should complete "func" to "function_one" or "function_two"
            // WordCompleter sorts by length then alphabetically.
            // "function_one" and "function_two" have same length.
            // "function_one" < "function_two" alphabetically.
            assert!(content.contains("function_one function_two function_one"));
        }
        assert_eq!(
            app.message,
            Some("Completed: function_one (1 more)".to_string())
        );
    }
}
