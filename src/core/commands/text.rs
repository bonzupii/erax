//! Text-transformation commands (case, fill, sort).
use crate::core::app::EditorApp;
/// Text transformation and manipulation commands
use crate::core::dispatcher::{DispatchResult, InputAction};

// Helper function to calculate cursor position from byte offset
fn byte_to_cursor_position(
    buffer: &crate::core::buffer::Buffer,
    byte_pos: usize,
) -> (usize, usize) {
    let line = buffer.byte_to_line(byte_pos);
    if let Some(line_start) = buffer.line_to_byte(line)
        && let Some(line_text) = buffer.line(line) {
            let byte_offset_in_line = byte_pos.saturating_sub(line_start);
            let grapheme_col =
                crate::core::utf8::byte_to_grapheme_col(&line_text, byte_offset_in_line);
            return (line, grapheme_col);
        }
    (line, 0)
}

/// Get the byte range of the word at cursor position.
/// Returns (start_byte, end_byte) of the word, or None if no word is found.
fn get_word_range_at_cursor(
    window: &crate::core::window::Window,
    buffer: &crate::core::buffer::Buffer,
) -> Option<(usize, usize)> {
    let start_byte = window.get_byte_offset(buffer)?;
    let mut temp_window = window.clone();
    temp_window.forward_word(buffer);
    let end_byte = temp_window.get_byte_offset(buffer)?;
    if end_byte > start_byte {
        Some((start_byte, end_byte))
    } else {
        None
    }
}

pub fn justify_paragraph(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id
            && let (Some(window), Some(buffer)) = (
                app.windows.get(&active_window_id),
                app.buffers.get_mut(&buffer_id),
            ) {
                let current_line = window.cursor_y;
                let total_lines = buffer.line_count();

                // 1. Find start of paragraph (search backwards for empty line)
                let mut start_line = current_line;
                while start_line > 0 {
                    if let Some(prev_line) = buffer.line(start_line - 1)
                        && prev_line.trim().is_empty() {
                            break;
                        }
                    start_line -= 1;
                }

                // 2. Find end of paragraph (search forwards for empty line)
                let mut end_line = current_line + 1;
                while end_line < total_lines {
                    if let Some(this_line) = buffer.line(end_line)
                        && this_line.trim().is_empty() {
                            break;
                        }
                    end_line += 1;
                }

                // 3. Capture indentation from the FIRST line of the paragraph
                let indent_string = if let Some(line) = buffer.line(start_line) {
                    line.chars()
                        .take_while(|c| c.is_whitespace())
                        .collect::<String>()
                } else {
                    String::new()
                };
                let indent_width = indent_string.len();

                // 4. Collect all words in the paragraph
                let mut words = Vec::new();
                for line_idx in start_line..end_line {
                    if let Some(line) = buffer.line(line_idx) {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            words.extend(trimmed.split_whitespace().map(|s| s.to_string()));
                        }
                    }
                }

                // 5. Reflow text
                let mut formatted_lines = Vec::new();
                let mut current_line_text = String::new();
                let max_width: usize = window.width.max(20);
                let effective_width = max_width.saturating_sub(indent_width).saturating_sub(1);
                let mut current_width = 0;

                for word in words {
                    let word_len = word.len();
                    let fits = if current_width == 0 {
                        true
                    } else {
                        current_width + 1 + word_len <= effective_width
                    };

                    if fits {
                        if current_width > 0 {
                            current_line_text.push(' ');
                            current_width += 1;
                        }
                        current_line_text.push_str(&word);
                        current_width += word_len;
                    } else {
                        formatted_lines.push(format!("{}{}", indent_string, current_line_text));
                        current_line_text = word.to_string();
                        current_width = word_len;
                    }
                }

                if !current_line_text.is_empty() {
                    formatted_lines.push(format!("{}{}", indent_string, current_line_text));
                }

                // 6. Apply changes to buffer
                let start_byte = buffer.line_to_byte(start_line).unwrap_or_default();
                let end_byte = match buffer.line_to_byte(end_line) {
                    Some(b) => b,
                    None => buffer.len(),
                };

                // Delete old paragraph
                let delete_len = end_byte - start_byte;
                buffer.delete(start_byte, delete_len);

                // Insert new paragraph
                let mut new_paragraph = formatted_lines.join("\n");
                if end_line < total_lines || delete_len > 0 {
                    new_paragraph.push('\n');
                }
                buffer.insert(start_byte, &new_paragraph);

                // Restore cursor
                if let Some(window_mut) = app.windows.get_mut(&active_window_id) {
                    window_mut.cursor_y = start_line;
                    window_mut.cursor_x = indent_width;
                }
            }
        DispatchResult::Success
    }

pub fn delete_blank_lines(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id
            && let (Some(window), Some(buffer)) = (
                app.windows.get(&app.active_window),
                app.buffers.get_mut(&buffer_id),
            ) {
                let current_line = window.cursor_y;
                let is_current_blank = buffer.line(current_line).map(|l| l.trim().is_empty()).unwrap_or(true);

                let mut start_line = current_line;
                if is_current_blank {
                    while start_line > 0 {
                        if let Some(l) = buffer.line(start_line - 1)
                            && !l.trim().is_empty() {
                                break;
                            }
                        start_line -= 1;
                    }
                } else {
                    start_line = current_line + 1;
                }

                let mut end_line = start_line;
                while end_line < buffer.line_count() {
                    if let Some(l) = buffer.line(end_line)
                        && !l.trim().is_empty() {
                            break;
                        }
                    end_line += 1;
                }

                if end_line > start_line {
                    let start_byte = buffer.line_to_byte(start_line).unwrap_or_default();
                    let end_byte = match buffer.line_to_byte(end_line) {
                        Some(b) => b,
                        None => buffer.len(),
                    };

                    buffer.delete(start_byte, end_byte - start_byte);

                    if is_current_blank {
                        buffer.insert(start_byte, "\n");
                    }
                }
            }
        DispatchResult::Success
    }

pub fn goto_matching_fence(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = if let Some(window) = app.windows.get(&active_window_id) {
            window.buffer_id
        } else {
            return DispatchResult::Info("No active window".to_string());
        };

        if let Some(buffer) = app.buffers.get(&buffer_id)
            && let Some(window) = app.windows.get_mut(&active_window_id)
        {
                let mut start_pos = window.get_byte_offset(buffer).unwrap_or_default();
                let mut current_char = buffer.char_at(start_pos);

                // Not on a bracket: vim's `%` seeks the next bracket forward.
                if !matches!(current_char, Some('{' | '(' | '[' | '}' | ')' | ']')) {
                    let total = buffer.len();
                    let mut p = start_pos + 1;
                    while p < total {
                        if let Some(c) = buffer.char_at(p)
                            && matches!(c, '{' | '(' | '[' | '}' | ')' | ']')
                        {
                            break;
                        }
                        p += 1;
                    }
                    if p < total {
                        start_pos = p;
                        current_char = buffer.char_at(p);
                    }
                }

                if let Some(ch) = current_char {
                    let (target, dir) = match ch {
                        '{' => ('}', 1),
                        '(' => (')', 1),
                        '[' => (']', 1),
                        '}' => ('{', -1),
                        ')' => ('(', -1),
                        ']' => ('[', -1),
                        _ => return DispatchResult::Info("Not a fence character".to_string()),
                    };

                    let mut count = 1;
                    let mut pos = start_pos as isize;
                    let max_len = buffer.len() as isize;
                    let mut found = false;

                    loop {
                        pos += dir;
                        if pos < 0 || pos >= max_len {
                            break;
                        }

                        if let Some(c) = buffer.char_at(pos as usize) {
                            if c == ch {
                                count += 1;
                            } else if c == target {
                                count -= 1;
                            }
                        }

                        if count == 0 {
                            found = true;
                            break;
                        }
                    }

                    if found {
                        app.goto_byte(pos as usize);
                    } else {
                        return DispatchResult::Info("No matching fence found".to_string());
                    }
                }
            }
        DispatchResult::Success
    }

pub fn case_word_upper(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = match app.windows.get(&active_window_id).map(|w| w.buffer_id) {
            Some(id) => id,
            None => return DispatchResult::Success,
        };

        let range = {
            let window = match app.windows.get(&active_window_id) {
                Some(w) => w,
                None => return DispatchResult::Success,
            };
            let buffer = match app.buffers.get(&buffer_id) {
                Some(b) => b,
                None => return DispatchResult::Success,
            };
            get_word_range_at_cursor(window, buffer)
        };

        if let Some((start_byte, end_byte)) = range {
            let new_text = {
                let buffer = match app.buffers.get(&buffer_id) {
                    Some(b) => b,
                    None => return DispatchResult::Success,
                };
                buffer
                    .get_range_as_string(start_byte, end_byte - start_byte)
                    .to_uppercase()
            };

            if let Some(buffer_mut) = app.buffers.get_mut(&buffer_id) {
                buffer_mut.delete(start_byte, end_byte - start_byte);
                buffer_mut.insert(start_byte, &new_text);
            }

            if let Some(window_mut) = app.windows.get_mut(&active_window_id)
                && let Some(buffer) = app.buffers.get(&buffer_id) {
                    let (line, col) = byte_to_cursor_position(buffer, start_byte + new_text.len());
                    window_mut.cursor_y = line;
                    window_mut.cursor_x = col;
                }
        }
        DispatchResult::Success
    }

pub fn case_word_lower(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = match app.windows.get(&active_window_id).map(|w| w.buffer_id) {
            Some(id) => id,
            None => return DispatchResult::Success,
        };

        let range = {
            let window = match app.windows.get(&active_window_id) {
                Some(w) => w,
                None => return DispatchResult::Success,
            };
            let buffer = match app.buffers.get(&buffer_id) {
                Some(b) => b,
                None => return DispatchResult::Success,
            };
            get_word_range_at_cursor(window, buffer)
        };

        if let Some((start_byte, end_byte)) = range {
            let new_text = {
                let buffer = match app.buffers.get(&buffer_id) {
                    Some(b) => b,
                    None => return DispatchResult::Success,
                };
                buffer
                    .get_range_as_string(start_byte, end_byte - start_byte)
                    .to_lowercase()
            };

            if let Some(buffer_mut) = app.buffers.get_mut(&buffer_id) {
                buffer_mut.delete(start_byte, end_byte - start_byte);
                buffer_mut.insert(start_byte, &new_text);
            }

            if let Some(window_mut) = app.windows.get_mut(&active_window_id)
                && let Some(buffer) = app.buffers.get(&buffer_id) {
                    let (line, col) = byte_to_cursor_position(buffer, start_byte + new_text.len());
                    window_mut.cursor_y = line;
                    window_mut.cursor_x = col;
                }
        }
        DispatchResult::Success
    }

pub fn case_word_capitalize(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = match app.windows.get(&active_window_id).map(|w| w.buffer_id) {
            Some(id) => id,
            None => return DispatchResult::Success,
        };

        let range = {
            let window = match app.windows.get(&active_window_id) {
                Some(w) => w,
                None => return DispatchResult::Success,
            };
            let buffer = match app.buffers.get(&buffer_id) {
                Some(b) => b,
                None => return DispatchResult::Success,
            };
            get_word_range_at_cursor(window, buffer)
        };

        if let Some((start_byte, end_byte)) = range {
            let new_text = {
                let buffer = match app.buffers.get(&buffer_id) {
                    Some(b) => b,
                    None => return DispatchResult::Success,
                };
                let word_text = buffer.get_range_as_string(start_byte, end_byte - start_byte);
                let mut c = word_text.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                }
            };

            if let Some(buffer_mut) = app.buffers.get_mut(&buffer_id) {
                buffer_mut.delete(start_byte, end_byte - start_byte);
                buffer_mut.insert(start_byte, &new_text);
            }

            if let Some(window_mut) = app.windows.get_mut(&active_window_id)
                && let Some(buffer) = app.buffers.get(&buffer_id) {
                    let (line, col) = byte_to_cursor_position(buffer, start_byte + new_text.len());
                    window_mut.cursor_y = line;
                    window_mut.cursor_x = col;
                }
        }
        DispatchResult::Success
    }

pub fn shell_command(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Shell command: ".to_string(),
            action: InputAction::ShellCommand,
        }
    }

pub fn filter_buffer(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Filter buffer: ".to_string(),
            action: InputAction::FilterBuffer,
        }
    }

pub fn uppercase_region(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id
            && let Some(window) = app.windows.get(&active_window_id)
                && let Some(buffer) = app.buffers.get(&buffer_id) {
                    if let Some(mark) = window.mark {
                        let cursor_byte = window.get_byte_offset(buffer).unwrap_or_default();

                        let mut temp_window = window.clone();
                        temp_window.cursor_x = mark.0;
                        temp_window.cursor_y = mark.1;
                        let mark_byte = temp_window.get_byte_offset(buffer).unwrap_or_default();

                        let (start_byte, end_byte) = if cursor_byte < mark_byte {
                            (cursor_byte, mark_byte)
                        } else {
                            (mark_byte, cursor_byte)
                        };

                        if end_byte > start_byte {
                            let region_text =
                                buffer.get_range_as_string(start_byte, end_byte - start_byte);
                            let upper_text = region_text.to_uppercase();

                            if let Some(buffer_mut) = app.buffers.get_mut(&buffer_id) {
                                buffer_mut.delete(start_byte, end_byte - start_byte);
                                buffer_mut.insert(start_byte, &upper_text);
                            }

                            if let Some(window_mut) = app.windows.get_mut(&active_window_id)
                                && let Some(buffer) = app.buffers.get(&buffer_id) {
                                    let (line, col) = byte_to_cursor_position(
                                        buffer,
                                        start_byte + upper_text.len(),
                                    );
                                    window_mut.cursor_y = line;
                                    window_mut.cursor_x = col;
                                }
                        }
                    } else {
                        return DispatchResult::Info("No mark set".to_string());
                    }
                }
        DispatchResult::Success
    }

pub fn lowercase_region(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id
            && let Some(window) = app.windows.get(&active_window_id)
                && let Some(buffer) = app.buffers.get(&buffer_id) {
                    if let Some(mark) = window.mark {
                        let cursor_byte = window.get_byte_offset(buffer).unwrap_or_default();

                        let mut temp_window = window.clone();
                        temp_window.cursor_x = mark.0;
                        temp_window.cursor_y = mark.1;
                        let mark_byte = temp_window.get_byte_offset(buffer).unwrap_or_default();

                        let (start_byte, end_byte) = if cursor_byte < mark_byte {
                            (cursor_byte, mark_byte)
                        } else {
                            (mark_byte, cursor_byte)
                        };

                        if end_byte > start_byte {
                            let region_text =
                                buffer.get_range_as_string(start_byte, end_byte - start_byte);
                            let lower_text = region_text.to_lowercase();

                            if let Some(buffer_mut) = app.buffers.get_mut(&buffer_id) {
                                buffer_mut.delete(start_byte, end_byte - start_byte);
                                buffer_mut.insert(start_byte, &lower_text);
                            }

                            if let Some(window_mut) = app.windows.get_mut(&active_window_id)
                                && let Some(buffer) = app.buffers.get(&buffer_id) {
                                    let (line, col) = byte_to_cursor_position(
                                        buffer,
                                        start_byte + lower_text.len(),
                                    );
                                    window_mut.cursor_y = line;
                                    window_mut.cursor_x = col;
                                }
                        }
                    } else {
                        return DispatchResult::Info("No mark set".to_string());
                    }
                }
        DispatchResult::Success
    }

pub fn wrap_word(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;

        let should_loop = {
            if let Some(window) = app.windows.get(&active_window_id) {
                app.buffers.contains_key(&window.buffer_id)
            } else {
                false
            }
        };

        if !should_loop {
            return DispatchResult::Success;
        }

        let (found_break, break_pos, word_len) = {
            if let Some(window) = app.windows.get_mut(&active_window_id) {
                if let Some(buffer) = app.buffers.get_mut(&window.buffer_id) {
                    if !window.move_backward(buffer) {
                        return DispatchResult::Success;
                    }

                    let mut current_pos = window.get_byte_offset(buffer).unwrap_or_default();
                    let mut cnt = 0;
                    let mut found = false;
                    let mut hit_start = false;

                    loop {
                        let c = buffer.char_at(current_pos);
                        if let Some(ch) = c
                            && (ch == ' ' || ch == '\t') {
                                found = true;
                                break;
                            }

                        cnt += 1;

                        if !window.move_backward(buffer) {
                            if window.cursor_x == 0 {
                                hit_start = true;
                            }
                            break;
                        }
                        current_pos = window.get_byte_offset(buffer).unwrap_or_default();

                        if window.cursor_x == 0 {
                            let c0 = buffer.char_at(current_pos);
                            if let Some(ch0) = c0
                                && (ch0 == ' ' || ch0 == '\t') {
                                    found = true;
                                    break;
                                }
                            hit_start = true;
                            break;
                        }
                    }

                    (
                        found || hit_start,
                        if hit_start { None } else { Some(current_pos) },
                        cnt,
                    )
                } else {
                    return DispatchResult::Success;
                }
            } else {
                return DispatchResult::Success;
            }
        };

        if !found_break {
            return DispatchResult::Success;
        }

        if let Some(window) = app.windows.get_mut(&active_window_id)
            && let Some(buffer) = app.buffers.get_mut(&window.buffer_id) {
                if break_pos.is_none() {
                    // Hit start of line
                    window.end_of_line(buffer);
                    let insert_pos = window.get_byte_offset(buffer).unwrap_or_default();
                    buffer.insert(insert_pos, "\n");
                    window.move_down(buffer);
                    window.beginning_of_line(buffer);
                } else {
                    // Found space at break_pos - use if let to safely unwrap
                    if let Some(pos) = break_pos {
                        buffer.delete(pos, 1); // Delete 1 char (space/tab)
                        buffer.insert(pos, "\n");

                        // Move cursor to end of wrap
                        let new_cursor_byte = pos + 1 + word_len;
                        let (line, col) = byte_to_cursor_position(buffer, new_cursor_byte);
                        window.cursor_y = line;
                        window.cursor_x = col;
                    }
                }

                window.update_visual_cursor(buffer);
                window.ensure_cursor_visible(buffer);
            }

        DispatchResult::Success
    }
