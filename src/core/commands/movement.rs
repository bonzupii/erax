//! Cursor-movement commands.
use crate::core::app::EditorApp;
use crate::core::buffer::Buffer;
/// Cursor movement commands
use crate::core::dispatcher::DispatchResult;
use crate::core::window::Window;

/// Helper to update selection after cursor movement
fn update_selection(window: &mut Window, buffer: &Buffer) {
    // If a selection is active (even if empty/point), extend it to the new cursor position
    if window.selection_manager.primary.is_some()
        && let Some(byte_pos) = window.get_byte_offset(buffer) {
            window.selection_manager.extend_selection(byte_pos, buffer);
        }
}

/// Helper to execute a movement command with the standard boilerplate pattern.
/// Reduces duplication across all movement commands.
fn execute_movement<F>(app: &mut EditorApp, count: usize, movement_fn: F) -> DispatchResult
where
    F: Fn(&mut Window, &Buffer),
{
    let active_window_id = app.active_window;
    let buffer_id = match app.windows.get(&active_window_id) {
        Some(w) => w.buffer_id,
        None => return DispatchResult::Success,
    };

    if let Some(buffer) = app.buffers.get(&buffer_id)
        && let Some(window) = app.windows.get_mut(&active_window_id) {
            for _ in 0..count {
                // A step that makes no progress (end of buffer/line) would
                // repeat forever: huge counts (`999999999j`) terminate in
                // O(distance), not O(count).
                let before = window.get_byte_offset(buffer);
                movement_fn(window, buffer);
                update_selection(window, buffer);
                if window.get_byte_offset(buffer) == before {
                    break;
                }
            }
            window.ensure_cursor_visible(buffer);
        }
    DispatchResult::Success
}

/// Helper for single-execution movements (no count loop)
fn execute_movement_once<F>(app: &mut EditorApp, movement_fn: F) -> DispatchResult
where
    F: Fn(&mut Window, &Buffer),
{
    execute_movement(app, 1, movement_fn)
}

pub fn forward_character(app: &mut EditorApp, count: usize) -> DispatchResult {
        execute_movement(app, count, |window, buffer| {
            window.move_forward(buffer);
        })
    }

pub fn backward_character(app: &mut EditorApp, count: usize) -> DispatchResult {
        execute_movement(app, count, |window, buffer| {
            window.move_backward(buffer);
        })
    }

pub fn next_line(app: &mut EditorApp, count: usize) -> DispatchResult {
        execute_movement(app, count, |window, buffer| {
            window.move_down(buffer);
        })
    }

pub fn previous_line(app: &mut EditorApp, count: usize) -> DispatchResult {
        execute_movement(app, count, |window, buffer| {
            window.move_up(buffer);
        })
    }

pub fn beginning_of_line(app: &mut EditorApp, _count: usize) -> DispatchResult {
        execute_movement_once(app, |window, buffer| {
            window.beginning_of_line(buffer);
        })
    }

/// Smart Home: first non-whitespace, then column 0 (VS Code behavior).
pub fn smart_home(app: &mut EditorApp, _count: usize) -> DispatchResult {
        execute_movement_once(app, |window, buffer| {
            window.smart_home(buffer);
        })
    }

pub fn end_of_line(app: &mut EditorApp, _count: usize) -> DispatchResult {
        execute_movement_once(app, |window, buffer| {
            window.end_of_line(buffer);
        })
    }

pub fn beginning_of_file(app: &mut EditorApp, _count: usize) -> DispatchResult {
        execute_movement_once(app, |window, buffer| {
            window.beginning_of_buffer(buffer);
        })
    }

pub fn end_of_file(app: &mut EditorApp, _count: usize) -> DispatchResult {
        execute_movement_once(app, |window, buffer| {
            window.end_of_buffer(buffer);
        })
    }

pub fn forward_word(app: &mut EditorApp, count: usize) -> DispatchResult {
        execute_movement(app, count, |window, buffer| {
            window.forward_word(buffer);
        })
    }

pub fn backward_word(app: &mut EditorApp, count: usize) -> DispatchResult {
        execute_movement(app, count, |window, buffer| {
            window.backward_word(buffer);
        })
    }

pub fn forward_page(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id
            && let Some(buffer) = app.buffers.get(&buffer_id)
                && let Some(window) = app.windows.get_mut(&active_window_id) {
                    // Move cursor down by page_size lines
                    let page_size = window.height.saturating_sub(2).max(1);
                    for _ in 0..page_size {
                        window.move_down(buffer);
                    }
                    update_selection(window, buffer);
                    // Edge-triggered scroll will handle viewport adjustment
                    window.ensure_cursor_visible(buffer);
                }
        DispatchResult::Success
    }

pub fn backward_page(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id
            && let Some(buffer) = app.buffers.get(&buffer_id)
                && let Some(window) = app.windows.get_mut(&active_window_id) {
                    // Move cursor up by page_size lines
                    let page_size = window.height.saturating_sub(2).max(1);
                    for _ in 0..page_size {
                        window.move_up(buffer);
                    }
                    update_selection(window, buffer);
                    // Edge-triggered scroll will handle viewport adjustment
                    window.ensure_cursor_visible(buffer);
                }
        DispatchResult::Success
    }

pub fn forward_paragraph(app: &mut EditorApp, count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            for _ in 0..count {
                // Huge counts (`999999999}`) terminate when an iteration
                // makes no progress (already at the last paragraph).
                let before = app
                    .windows
                    .get(&active_window_id)
                    .map(|w| (w.cursor_x, w.cursor_y));
                if let Some(window) = app.windows.get_mut(&active_window_id)
                    && let Some(buffer) = app.buffers.get(&buffer_id) {
                        // uEmacs paragraph movement: move to next paragraph boundary
                        // A paragraph is defined as text separated by blank lines
                        let current_line = window.cursor_y;
                        let total_lines = buffer.line_count();

                        // Find next paragraph boundary
                        let mut target_line = current_line + 1;
                        while target_line < total_lines {
                            if let Some(line) = buffer.line(target_line)
                                && line.trim().is_empty() {
                                    // Found blank line, move to next non-blank line
                                    let mut next_paragraph = target_line + 1;
                                    while next_paragraph < total_lines {
                                        if let Some(next_line) = buffer.line(next_paragraph)
                                            && !next_line.trim().is_empty() {
                                                break;
                                            }
                                        next_paragraph += 1;
                                    }

                                    if next_paragraph < total_lines {
                                        window.cursor_y = next_paragraph;
                                        window.cursor_x = 0;
                                        window.update_visual_cursor(buffer);
                                        break;
                                    } else {
                                        // End of buffer
                                        window.cursor_y = total_lines.saturating_sub(1);
                                        window.cursor_x = 0;
                                        window.update_visual_cursor(buffer);
                                        break;
                                    }
                                }
                            target_line += 1;
                        }

                        // If we didn't find a blank line, go to end of buffer
                        if target_line >= total_lines {
                            window.cursor_y = total_lines.saturating_sub(1);
                            window.cursor_x = 0;
                            window.update_visual_cursor(buffer);
                        }
                        update_selection(window, buffer);
                    }
                if app
                    .windows
                    .get(&active_window_id)
                    .map(|w| (w.cursor_x, w.cursor_y))
                    == before
                {
                    break;
                }
            }
        }
        DispatchResult::Success
    }

pub fn backward_paragraph(app: &mut EditorApp, count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        let buffer_id = app.windows.get(&active_window_id).map(|w| w.buffer_id);

        if let Some(buffer_id) = buffer_id {
            for _ in 0..count {
                // Huge counts (`999999999{`) terminate when an iteration
                // makes no progress (already at the first paragraph).
                let before = app
                    .windows
                    .get(&active_window_id)
                    .map(|w| (w.cursor_x, w.cursor_y));
                if let Some(window) = app.windows.get_mut(&active_window_id)
                    && let Some(buffer) = app.buffers.get(&buffer_id) {
                        let mut y = window.cursor_y;

                        // 1. If at start of paragraph (or in blank lines), move back to end of previous paragraph
                        // Check if we need to jump back because we are at the top of a paragraph
                        let mut at_start_of_para = false;
                        if y > 0
                            && let Some(line) = buffer.line(y)
                                && !line.trim().is_empty()
                                    && let Some(prev) = buffer.line(y - 1)
                                        && prev.trim().is_empty() {
                                            at_start_of_para = true;
                                        }

                        if at_start_of_para {
                            y = y.saturating_sub(1);
                        }

                        // 2. Skip over any blank lines (separator) backwards
                        while y > 0 {
                            if let Some(line) = buffer.line(y)
                                && !line.trim().is_empty() {
                                    break;
                                }
                            y -= 1;
                        }

                        // 3. Scan backward until we find a blank line or SOB (Start of Buffer)
                        // The line AFTER the blank line is the start of the paragraph
                        while y > 0 {
                            if let Some(prev) = buffer.line(y - 1)
                                && prev.trim().is_empty() {
                                    break;
                                }
                            y -= 1;
                        }

                        window.cursor_y = y;
                        window.cursor_x = 0;
                        window.update_visual_cursor(buffer);
                        update_selection(window, buffer);
                        window.ensure_cursor_visible(buffer);
                    }
                if app
                    .windows
                    .get(&active_window_id)
                    .map(|w| (w.cursor_x, w.cursor_y))
                    == before
                {
                    break;
                }
            }
        }
        DispatchResult::Success
    }

/// Byte offsets where sentences start: buffer start, plus the first
/// non-blank character after every sentence end.
///
/// A sentence ends at `.`, `!` or `?` — optionally followed by closing
/// `)`, `]`, `"` or `'` characters (which belong to the sentence) — when
/// the next character after that is end-of-line, a space, a tab, or end
/// of buffer. This is vim's rule, wart for wart (`e.g.` ends a sentence).
fn sentence_starts(buffer: &Buffer) -> Vec<usize> {
    let total = buffer.len();
    let text = buffer.get_range_as_string(0, total);
    let chars: Vec<char> = text.chars().collect();
    // Byte offset of each char, plus a sentinel for the end.
    let mut bytes = Vec::with_capacity(chars.len() + 1);
    let mut b = 0;
    for c in &chars {
        bytes.push(b);
        b += c.len_utf8();
    }
    bytes.push(b);

    let mut starts = vec![0];
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '.' || c == '!' || c == '?' {
            let mut j = i + 1;
            while j < chars.len() && matches!(chars[j], ')' | ']' | '"' | '\'') {
                j += 1;
            }
            let terminates = j == chars.len()
                || chars[j] == '\n'
                || chars[j] == ' '
                || chars[j] == '\t';
            if terminates {
                let mut k = j;
                while k < chars.len()
                    && (chars[k] == ' ' || chars[k] == '\t' || chars[k] == '\n')
                {
                    k += 1;
                }
                if k < chars.len() {
                    starts.push(bytes[k]);
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    starts
}

/// Place the window cursor on a buffer byte offset (helper for motions
/// that compute byte targets directly).
fn place_window_at_byte(window: &mut Window, buffer: &Buffer, byte: usize) {
    let (li, col) = buffer.byte_to_position(byte);
    window.cursor_y = li;
    window.cursor_x = col;
    window.update_visual_cursor(buffer);
    update_selection(window, buffer);
    window.ensure_cursor_visible(buffer);
}

/// `)`: [count] sentences forward — to the start of the next sentence.
/// Exclusive; at the last sentence moves to end of buffer (vim).
pub fn forward_sentence(app: &mut EditorApp, count: usize) -> DispatchResult {
    let active_window_id = app.active_window;
    let buffer_id = match app.windows.get(&active_window_id).map(|w| w.buffer_id) {
        Some(id) => id,
        None => return DispatchResult::Success,
    };
    // Movement never mutates the buffer: the sentence starts are
    // loop-invariant, computed once instead of per count step.
    let starts = match app.buffers.get(&buffer_id) {
        Some(b) => sentence_starts(b),
        None => return DispatchResult::Success,
    };
    for _ in 0..count.max(1) {
        let pos = match app
            .windows
            .get(&active_window_id)
            .zip(app.buffers.get(&buffer_id))
        {
            Some((w, b)) => w.get_byte_offset(b).unwrap_or(0),
            None => return DispatchResult::Success,
        };
        // No next sentence: vim's `)` rests on the last character (not
        // past the end, not on a trailing newline).
        let fallback = {
            let b = app.buffers.get(&buffer_id);
            let len = b.map(|b| b.len()).unwrap_or(0);
            if len > 0
                && b.map(|b| b.get_range_as_string(len - 1, 1) == "\n").unwrap_or(false)
            {
                len.saturating_sub(2)
            } else {
                len.saturating_sub(1)
            }
        };
        let target = starts
            .iter()
            .find(|s| **s > pos)
            .copied()
            .unwrap_or(fallback);
        // Past the last sentence there is nowhere to go: without this a
        // huge count (`999999999)`) would spin instead of stopping.
        if target == pos {
            break;
        }
        if let Some(window) = app.windows.get_mut(&active_window_id)
            && let Some(buffer) = app.buffers.get(&buffer_id)
        {
            place_window_at_byte(window, buffer, target);
        }
    }
    DispatchResult::Success
}

/// `(`: [count] sentences backward — to the start of the current sentence,
/// or the previous one when already at a sentence start. Exclusive.
pub fn backward_sentence(app: &mut EditorApp, count: usize) -> DispatchResult {
    let active_window_id = app.active_window;
    let buffer_id = match app.windows.get(&active_window_id).map(|w| w.buffer_id) {
        Some(id) => id,
        None => return DispatchResult::Success,
    };
    // Movement never mutates the buffer: the sentence starts are
    // loop-invariant, computed once instead of per count step.
    let starts = match app.buffers.get(&buffer_id) {
        Some(b) => sentence_starts(b),
        None => return DispatchResult::Success,
    };
    for _ in 0..count.max(1) {
        let pos = match app
            .windows
            .get(&active_window_id)
            .zip(app.buffers.get(&buffer_id))
        {
            Some((w, b)) => w.get_byte_offset(b).unwrap_or(0),
            None => return DispatchResult::Success,
        };
        // No earlier sentence: stay (vim beeps), and stop — further steps
        // cannot move either.
        let Some(target) = starts.iter().rev().find(|s| **s < pos).copied() else {
            break;
        };
        if let Some(window) = app.windows.get_mut(&active_window_id)
            && let Some(buffer) = app.buffers.get(&buffer_id)
        {
            place_window_at_byte(window, buffer, target);
        }
    }
    DispatchResult::Success
}
