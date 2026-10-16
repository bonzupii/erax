//! Snippet-expansion commands.
use crate::core::app::EditorApp;
use crate::core::dispatcher::DispatchResult;
use crate::core::snippets::SnippetLanguage;

pub fn expand_snippet(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = match app.windows.get(&active_window_id) {
            Some(w) => w.buffer_id,
            None => return DispatchResult::Info("No active window".to_string()),
        };

        // Determine language and trigger
        let (language, trigger, word_start_col, word_end_col) = {
            let buffer = match app.buffers.get(&buffer_id) {
                Some(b) => b,
                None => return DispatchResult::Info("No buffer".to_string()),
            };
            let Some(window) = app.windows.get(&active_window_id) else {
                return DispatchResult::Info("No window".to_string());
            };

            let line_text = match buffer.line(window.cursor_y) {
                Some(t) => t,
                None => return DispatchResult::Info("Cannot get line text".to_string()),
            };

            let graphemes: Vec<&str> =
                crate::core::utf8::GraphemeIterator::new(&line_text).collect();

            let word_end = window.cursor_x;
            let mut word_start = word_end;
            while word_start > 0 && crate::core::utf8::is_word_grapheme(graphemes[word_start - 1]) {
                word_start -= 1;
            }

            if word_start == word_end {
                app.message = Some("No snippet trigger found before cursor".to_string());
                return DispatchResult::Success;
            }

            let trigger = graphemes[word_start..word_end].join("");

            // Determine language based on file extension
            let language = buffer
                .filename
                .as_ref()
                .and_then(|p| p.extension())
                .and_then(|e| e.to_str())
                .map(SnippetLanguage::from_extension)
                .unwrap_or(SnippetLanguage::Generic);

            (language, trigger, word_start, word_end)
        };

        let snippets = app.snippet_manager.get_snippets(language, &trigger);
        if let Some(snippet) = snippets.first() {
            let expanded = app.snippet_manager.expand(snippet);
            let cursor_offset = expanded.initial_cursor_offset();

            // Need to calculate byte offsets for deletion
            let (trigger_start_byte, trigger_len) = {
                let Some(buffer) = app.buffers.get(&buffer_id) else {
                    return DispatchResult::Success;
                };
                let Some(window) = app.windows.get(&active_window_id) else {
                    return DispatchResult::Success;
                };
                let Some(line_text) = buffer.line(window.cursor_y) else {
                    return DispatchResult::Success;
                };
                let graphemes: Vec<&str> =
                    crate::core::utf8::GraphemeIterator::new(&line_text).collect();

                let mut byte_start = 0;
                for i in 0..word_start_col {
                    byte_start += graphemes.get(i).map(|g| g.len()).unwrap_or_default();
                }
                let mut byte_end = byte_start;
                for i in word_start_col..word_end_col {
                    byte_end += graphemes.get(i).map(|g| g.len()).unwrap_or_default();
                }

                let Some(line_start_byte) = buffer.line_to_byte(window.cursor_y) else {
                    return DispatchResult::Success;
                };
                (line_start_byte + byte_start, byte_end - byte_start)
            };

            // Mutably borrow app elements to apply changes
            if let Some(buffer_mut) = app.buffers.get_mut(&buffer_id) {
                buffer_mut.delete(trigger_start_byte, trigger_len);
                buffer_mut.insert(trigger_start_byte, &expanded.text);

                // Position cursor at the first tab stop ($1), else $0 / end.
                app.goto_byte(trigger_start_byte + cursor_offset);

                // Clear any previous error message
                app.message = None;
            }

            DispatchResult::Success
        } else {
            app.message = Some(format!("No snippet match for '{}'", trigger));
            DispatchResult::Success
        }
    }
