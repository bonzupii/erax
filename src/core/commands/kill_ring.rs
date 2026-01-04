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
                            let byte_pos = match window.get_byte_offset(buffer) {
                                Some(b) => b,
                                None => 0,
                            };
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
                        let start_byte = match window.get_byte_offset(buffer) {
                            Some(b) => b,
                            None => 0,
                        };
                        let mut temp_window = window.clone();
                        temp_window.forward_word(buffer);
                        let end_byte = match temp_window.get_byte_offset(buffer) {
                            Some(b) => b,
                            None => 0,
                        };

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
                        let end_byte = match window.get_byte_offset(buffer) {
                            Some(b) => b,
                            None => 0,
                        };
                        let mut temp_window = window.clone();
                        temp_window.backward_word(buffer);
                        let start_byte = match temp_window.get_byte_offset(buffer) {
                            Some(b) => b,
                            None => 0,
                        };

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
            let text_len = text.len();
            if let Some(window) = app.windows.get_mut(&active_window_id) {
                if let Some(buffer) = app.buffers.get_mut(&window.buffer_id) {
                    // Record position before insert for yank-pop
                    let insert_pos = window.get_byte_offset(buffer).unwrap_or(0);

                    // Insert text at cursor position
                    for c in text.chars() {
                        window.insert_char(buffer, c);
                    }

                    // Record for yank-pop
                    app.last_yank_pos = Some(insert_pos);
                    app.last_yank_len = text_len;
                    app.last_command_was_yank = true;
                    return DispatchResult::Success;
                }
            }
        }
        app.last_command_was_yank = false;
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
            let line_content =
                match cursor_y.and_then(|y| app.buffers.get(&buffer_id).and_then(|b| b.line(y))) {
                    Some(s) => s,
                    None => String::new(),
                };

            let cursor_x = match app.windows.get(&active_window_id).map(|w| w.cursor_x) {
                Some(x) => x,
                None => 0,
            };

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

/// Cycle through the kill ring (replace previous yank with next item)
#[derive(Clone)]
pub struct YankPop;

impl Command for YankPop {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        // yank-pop only works immediately after yank or yank-pop
        if !app.last_command_was_yank {
            return DispatchResult::Info("yank-pop: No preceding yank".to_string());
        }

        let Some(yank_pos) = app.last_yank_pos else {
            return DispatchResult::Info("yank-pop: No yank position recorded".to_string());
        };

        let old_len = app.last_yank_len;

        // Rotate the kill ring to get next item
        let new_text = match app.kill_ring.rotate() {
            Some(text) => text.clone(),
            None => return DispatchResult::Info("yank-pop: Kill ring empty".to_string()),
        };

        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                // Delete the previously yanked text
                buffer.delete(yank_pos, old_len);

                // Insert the new text at the same position
                buffer.insert(yank_pos, &new_text);

                // Update cursor position - move to end of inserted text
                if let Some(window) = app.windows.get_mut(&active_window_id) {
                    // Calculate new cursor position from byte position
                    let new_end_pos = yank_pos + new_text.len();
                    let line = buffer.byte_to_line(new_end_pos);
                    let line_start = buffer.line_to_byte(line).unwrap_or(0);
                    let col = crate::core::utf8::grapheme_count(
                        &buffer.get_range_as_string(line_start, new_end_pos - line_start),
                    );
                    window.cursor_y = line;
                    window.cursor_x = col;
                    window.update_visual_cursor(buffer);
                }

                // Update tracking for potential next yank-pop
                app.last_yank_len = new_text.len();
                // last_yank_pos stays the same
                app.last_command_was_yank = true; // Allow chaining yank-pops

                return DispatchResult::Success;
            }
        }

        app.last_command_was_yank = false;
        DispatchResult::NotHandled
    }
}
