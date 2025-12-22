use crate::core::app::EditorApp;
/// Kill ring and yank (clipboard) commands
use crate::core::command::Command;
use crate::core::dispatcher::DispatchResult;

/// Kill text from cursor to end of line
#[derive(Clone)]
pub struct KillLine;

impl Command for KillLine {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            // First, gather all the data we need without holding borrows
            let kill_data: Option<(String, usize)> = {
                if let Some(window) = app.windows.get(&active_window_id) {
                    if let Some(buffer) = app.buffers.get(&buffer_id) {
                        if let Some(line) = buffer.line(window.cursor_y) {
                            let graphemes: Vec<&str> =
                                crate::core::utf8::GraphemeIterator::new(&line).collect();
                            let kill_text: String =
                                graphemes.iter().skip(window.cursor_x).copied().collect();
                            let byte_pos = window.get_byte_offset(buffer).unwrap_or(0);
                            Some((kill_text, byte_pos))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            // Now we can safely modify app
            if let Some((kill_text, byte_pos)) = kill_data {
                // Kill the text (add to kill ring)
                let append = app.last_command_was_kill;
                app.kill_ring.push(&kill_text, append);
                app.set_kill_flag();

                // Delete the text from buffer using clean Buffer API
                if !kill_text.is_empty() {
                    if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                        buffer.delete(byte_pos, kill_text.len());
                    }
                }
            }
        }
        DispatchResult::Success
    }
}

/// Kill word forward from cursor (actually kills backward - matching uEmacs M-backspace)
#[derive(Clone)]
pub struct KillWord;

impl Command for KillWord {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            let kill_data: Option<(String, usize, usize, usize)> = {
                if let Some(window) = app.windows.get(&active_window_id) {
                    if let Some(buffer) = app.buffers.get(&buffer_id) {
                        let start_byte = window.get_byte_offset(buffer).unwrap_or(0);
                        let mut temp_window = window.clone();
                        temp_window.forward_word(buffer);
                        let end_byte = temp_window.get_byte_offset(buffer).unwrap_or(0);

                        if end_byte > start_byte {
                            let text_to_kill =
                                buffer.get_range_as_string(start_byte, end_byte - start_byte);
                            Some((
                                text_to_kill,
                                start_byte,
                                end_byte,
                                buffer.byte_to_line(end_byte),
                            ))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            if let Some((text_to_kill, start_byte, _, _)) = kill_data {
                let append = app.last_command_was_kill;
                app.kill_ring.push(&text_to_kill, append);
                app.set_kill_flag();

                if !text_to_kill.is_empty() {
                    if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                        buffer.delete(start_byte, text_to_kill.len());
                    }
                    // Cursor position stays the same for forward kill
                }
            }
        }
        DispatchResult::Success
    }
}

/// Kill word backward from cursor
#[derive(Clone)]
pub struct BackwardKillWord;

impl Command for BackwardKillWord {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            let kill_data: Option<(String, usize, usize)> = {
                if let Some(window) = app.windows.get(&active_window_id) {
                    if let Some(buffer) = app.buffers.get(&buffer_id) {
                        let end_byte = window.get_byte_offset(buffer).unwrap_or(0);
                        let mut temp_window = window.clone();
                        temp_window.backward_word(buffer);
                        let start_byte = temp_window.get_byte_offset(buffer).unwrap_or(0);

                        if start_byte < end_byte {
                            let text_to_kill =
                                buffer.get_range_as_string(start_byte, end_byte - start_byte);
                            Some((text_to_kill, start_byte, end_byte))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            if let Some((text_to_kill, start_byte, _end_byte)) = kill_data {
                let append = app.last_command_was_kill;
                app.kill_ring.push(&text_to_kill, append);
                app.set_kill_flag();

                if !text_to_kill.is_empty() {
                    if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                        buffer.delete(start_byte, text_to_kill.len());
                    }
                    // Move cursor to start position
                    if let Some(window) = app.windows.get_mut(&active_window_id) {
                        if let Some(buffer) = app.buffers.get(&buffer_id) {
                            let line = buffer.byte_to_line(start_byte);
                            window.cursor_y = line;
                            // Calculate cursor_x from byte position
                            if let Some(line_start) = buffer.line_to_byte(line) {
                                if let Some(line_text) = buffer.line(line) {
                                    let byte_offset = start_byte.saturating_sub(line_start);
                                    window.cursor_x = crate::core::utf8::byte_to_grapheme_col(
                                        &line_text,
                                        byte_offset,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        DispatchResult::Success
    }
}

/// Yank (paste) text from kill ring
#[derive(Clone)]
pub struct Yank;

impl Command for Yank {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let text = app.kill_ring.peek().map(|s| s.to_string());

        if let Some(text) = text {
            if let Some(window) = app.windows.get_mut(&active_window_id) {
                if let Some(buffer) = app.buffers.get_mut(&window.buffer_id) {
                    // Insert text at cursor position
                    for c in text.chars() {
                        window.insert_char(buffer, c);
                    }
                }
            }
        }
        DispatchResult::Success
    }
}

/// Transpose characters at cursor (swap current and previous character)
#[derive(Clone)]
pub struct TransposeWords;

impl Command for TransposeWords {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            // Get current line content
            let cursor_y = app.windows.get(&active_window_id).map(|w| w.cursor_y);
            let line_content = cursor_y
                .and_then(|y| app.buffers.get(&buffer_id).and_then(|b| b.line(y)))
                .unwrap_or_default();

            let cursor_x = app
                .windows
                .get(&active_window_id)
                .map(|w| w.cursor_x)
                .unwrap_or(0);

            // Find word boundaries
            let graphemes: Vec<&str> =
                crate::core::utf8::GraphemeIterator::new(&line_content).collect();

            // Find start and end of current word
            let mut word2_end = cursor_x;
            while word2_end < graphemes.len()
                && !graphemes[word2_end].chars().all(char::is_whitespace)
            {
                word2_end += 1;
            }

            // Find start of current word (going backward)
            let mut word2_start = cursor_x;
            while word2_start > 0 && !graphemes[word2_start - 1].chars().all(char::is_whitespace) {
                word2_start -= 1;
            }

            // Find end of previous word (going backward from word2_start)
            let mut word1_end = word2_start;
            while word1_end > 0 && graphemes[word1_end - 1].chars().all(char::is_whitespace) {
                word1_end -= 1;
            }
            if word1_end == 0 {
                return DispatchResult::Success; // No previous word
            }

            // Find start of previous word
            let mut word1_start = word1_end;
            while word1_start > 0 && !graphemes[word1_start - 1].chars().all(char::is_whitespace) {
                word1_start -= 1;
            }

            // Extract words
            let word1: String = graphemes[word1_start..word1_end].join("");
            let word2: String = graphemes[word2_start..word2_end].join("");
            let space: String = graphemes[word1_end..word2_start].join("");

            if word1.is_empty() || word2.is_empty() {
                return DispatchResult::Success;
            }

            if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                if let Some(window) = app.windows.get_mut(&active_window_id) {
                    // Delete from word1_start to word2_end
                    let delete_len = word2_end - word1_start;
                    window.cursor_x = word1_start;
                    for _ in 0..delete_len {
                        window.delete_char(buffer, false);
                    }

                    // Insert in swapped order: word2 + space + word1
                    for c in word2.chars() {
                        window.insert_char(buffer, c);
                    }
                    for c in space.chars() {
                        window.insert_char(buffer, c);
                    }
                    for c in word1.chars() {
                        window.insert_char(buffer, c);
                    }
                }
            }
        }
        DispatchResult::Success
    }
}

/// Cycle through the kill ring
#[derive(Clone)]
pub struct YankPop;

impl Command for YankPop {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::Info("yank-pop: Not implemented".to_string())
    }
}
