//! Expand-selection command.
//!
//! Expands the selection to the next larger syntactic unit.
//! Uses the unified Lexer to find token boundaries.

use crate::core::app::EditorApp;
use crate::core::dispatcher::DispatchResult;
use crate::core::lexer::{LanguageConfig, Lexer, TokenKind};

pub fn mark_word(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let result: Option<(usize, usize)> = {
            let window = app.active_window_ref();
            let buffer = app.active_buffer();

            if let (Some(w), Some(b)) = (window, buffer) {
                let cursor_byte = w.get_byte_offset(b).unwrap_or_default();
                let content = b.content_string();
                expand_to_word(&content, cursor_byte)
            } else {
                None
            }
        };

        if let Some((start, end)) = result {
            // Set mark at word start, move cursor to word end
            app.goto_byte(start);
            if let Some(window) = app.active_window_mut() {
                window.mark = Some((window.cursor_x, window.cursor_y));
            }
            app.goto_byte(end);
            DispatchResult::Info("Word marked".to_string())
        } else {
            DispatchResult::Info("No word at cursor".to_string())
        }
    }

pub fn mark_line(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let result: Option<(usize, usize)> = {
            let window = app.active_window_ref();
            let buffer = app.active_buffer();
            if let (Some(w), Some(b)) = (window, buffer) {
                expand_to_line(&b.content_string(), w.cursor_y)
            } else {
                None
            }
        };
        if let Some((start, end)) = result {
            app.goto_byte(start);
            if let Some(window) = app.active_window_mut() {
                window.mark = Some((window.cursor_x, window.cursor_y));
            }
            app.goto_byte(end);
            DispatchResult::Info("Line marked".to_string())
        } else {
            DispatchResult::Info("Could not mark line".to_string())
        }
    }

pub fn mark_paragraph(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let result: Option<(usize, usize)> = {
            let window = app.active_window_ref();
            let buffer = app.active_buffer();
            if let (Some(w), Some(b)) = (window, buffer) {
                expand_to_paragraph(&b.content_string(), w.cursor_y)
            } else {
                None
            }
        };
        if let Some((start, end)) = result {
            app.goto_byte(start);
            if let Some(window) = app.active_window_mut() {
                window.mark = Some((window.cursor_x, window.cursor_y));
            }
            app.goto_byte(end);
            DispatchResult::Info("Paragraph marked".to_string())
        } else {
            DispatchResult::Info("Could not mark paragraph".to_string())
        }
    }

/// Mark the whole buffer: point at start, mark at end.
///
/// The shared primitive for "select all" (Ctrl+A in Standard mode). All input
/// models use this one implementation.
pub fn mark_whole_buffer(app: &mut EditorApp, _count: usize) -> DispatchResult {
    app.goto_byte(0);
    if let Some(window) = app.active_window_mut() {
        window.mark = Some((window.cursor_x, window.cursor_y));
    }
    if let Some(buffer) = app.active_buffer() {
        let end = buffer.len();
        app.goto_byte(end);
    }
    DispatchResult::Info("Whole buffer marked".to_string())
}

/// Expand selection to word boundaries using lexer
///
/// Returns the (start_byte, end_byte) of the word containing cursor_pos,
/// or None if cursor is not in a word.
pub fn expand_to_word(buffer_content: &str, cursor_pos: usize) -> Option<(usize, usize)> {
    let config = LanguageConfig::generic();
    let lexer = Lexer::new(buffer_content, &config);

    // Find the token containing the cursor position
    for token in lexer {
        let token_end = token.start + token.text.len();
        if cursor_pos >= token.start && cursor_pos <= token_end {
            // Check if this is a word-like token
            if matches!(
                token.kind,
                TokenKind::Identifier | TokenKind::Keyword | TokenKind::Type
            ) {
                return Some((token.start, token_end));
            }
        }
    }

    None
}

/// Expand selection to line boundaries
///
/// Returns the (start_byte, end_byte) for the entire line containing cursor.
pub fn expand_to_line(buffer_content: &str, line_num: usize) -> Option<(usize, usize)> {
    let mut line_start = 0;
    let mut current_line = 0;

    for (i, ch) in buffer_content.char_indices() {
        if current_line == line_num {
            // Found the start of our line
            let line_end = match buffer_content[i..]
                .find('\n')
                .map(|pos| i + pos + 1) // Include the newline
            {
                Some(end) => end,
                None => buffer_content.len(),
            };
            return Some((line_start, line_end));
        }

        if ch == '\n' {
            current_line += 1;
            line_start = i + 1;
        }
    }

    // Handle last line without newline
    if current_line == line_num && line_start <= buffer_content.len() {
        return Some((line_start, buffer_content.len()));
    }

    None
}

/// Expand selection to paragraph boundaries
///
/// A paragraph is a block of non-blank lines.
/// Returns (start_byte, end_byte) for the paragraph containing line_num.
pub fn expand_to_paragraph(buffer_content: &str, line_num: usize) -> Option<(usize, usize)> {
    let lines: Vec<&str> = buffer_content.lines().collect();

    if line_num >= lines.len() {
        return None;
    }

    // Find paragraph start (go backwards until blank line or start)
    let mut para_start_line = line_num;
    while para_start_line > 0 && !lines[para_start_line - 1].trim().is_empty() {
        para_start_line -= 1;
    }

    // Find paragraph end (go forwards until blank line or end)
    let mut para_end_line = line_num;
    while para_end_line + 1 < lines.len() && !lines[para_end_line + 1].trim().is_empty() {
        para_end_line += 1;
    }

    // Convert line numbers to byte offsets
    let start_byte: usize = lines[..para_start_line]
        .iter()
        .map(|l| l.len() + 1) // +1 for newline
        .sum();

    let end_byte: usize = lines[..=para_end_line].iter().map(|l| l.len() + 1).sum();

    Some((start_byte, end_byte.min(buffer_content.len())))
}

/// Represents an expansion level for progressive expansion
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExpansionLevel {
    /// No selection
    None,
    /// Word selected
    Word,
    /// Line selected
    Line,
    /// Paragraph selected
    Paragraph,
    /// Entire buffer selected
    All,
}

impl ExpansionLevel {
    /// Get the next expansion level
    pub fn next(self) -> Self {
        match self {
            ExpansionLevel::None => ExpansionLevel::Word,
            ExpansionLevel::Word => ExpansionLevel::Line,
            ExpansionLevel::Line => ExpansionLevel::Paragraph,
            ExpansionLevel::Paragraph => ExpansionLevel::All,
            ExpansionLevel::All => ExpansionLevel::All,
        }
    }
}

/// Progressive expand selection command (word -> line -> paragraph -> all).
///
/// Stateless (N-236): the current level is derived from the existing selection
/// by a pure function — if the selection already covers the word at the
/// cursor, the level is Word; if it covers the line, Line; and so on. The
/// command then advances one level. No state is stored in the command.
pub fn expand_selection(app: &mut EditorApp, _count: usize) -> DispatchResult {
    // Derive the current level from the selection, then advance.
    let next_level = {
        let window = app.active_window_ref();
        let buffer = app.active_buffer();
        match (window, buffer) {
            (Some(w), Some(b)) => current_expansion_level(w, b).next(),
            _ => return DispatchResult::NotHandled,
        }
    };

    // Apply the level (shared helper with mark_word/mark_line/mark_paragraph).
    apply_expansion_level(app, next_level)
}

/// Derive the current expansion level from the window's selection (N-236).
///
/// Compares the selection's byte range against the word/line/paragraph/buffer
/// bounds at the cursor: the level is the largest unit the selection already
/// covers exactly.
fn current_expansion_level(
    window: &crate::core::window::Window,
    buffer: &crate::core::buffer::Buffer,
) -> ExpansionLevel {
    let selection = match window.selection_manager.get_selection() {
        Some(s) => s,
        None => return ExpansionLevel::None,
    };
    let (sel_start, sel_end) = (selection.start().min(selection.end()), selection.start().max(selection.end()));
    let content = buffer.content_string();
    let cursor = window.get_byte_offset(buffer).unwrap_or(0);
    let len = content.len();

    if sel_start == 0 && sel_end == len {
        return ExpansionLevel::All;
    }
    if let Some((start, end)) = expand_to_paragraph(&content, window.cursor_y)
        && sel_start == start && sel_end == end {
            return ExpansionLevel::Paragraph;
        }
    if let Some((start, end)) = expand_to_line(&content, window.cursor_y)
        && sel_start == start && sel_end == end {
            return ExpansionLevel::Line;
        }
    if let Some((start, end)) = expand_to_word(&content, cursor)
        && sel_start == start && sel_end == end {
            return ExpansionLevel::Word;
        }
    ExpansionLevel::None
}

/// Apply an expansion level: select the word/line/paragraph/buffer at the cursor.
fn apply_expansion_level(app: &mut EditorApp, level: ExpansionLevel) -> DispatchResult {
    match level {
        ExpansionLevel::None => DispatchResult::Success,
        ExpansionLevel::Word | ExpansionLevel::Line | ExpansionLevel::Paragraph => {
            let result: Option<(usize, usize)> = {
                let w = app.active_window_ref();
                let b = app.active_buffer();
                if let (Some(w), Some(b)) = (w, b) {
                    let content = b.content_string();
                    match level {
                        ExpansionLevel::Word => {
                            let cursor = w.get_byte_offset(b).unwrap_or(0);
                            expand_to_word(&content, cursor)
                        }
                        ExpansionLevel::Line => expand_to_line(&content, w.cursor_y),
                        _ => expand_to_paragraph(&content, w.cursor_y),
                    }
                } else {
                    None
                }
            };
            if let Some((start, end)) = result {
                app.goto_byte(start);
                if let Some(w) = app.active_window_mut() {
                    w.mark = Some((w.cursor_x, w.cursor_y));
                }
                app.goto_byte(end);
            }
            DispatchResult::Success
        }
        ExpansionLevel::All => {
            // Select entire buffer
            if let Some(buffer) = app.active_buffer() {
                let len = buffer.len();
                app.goto_byte(0);
                if let Some(w) = app.active_window_mut() {
                    w.mark = Some((w.cursor_x, w.cursor_y));
                }
                app.goto_byte(len);
            }
            DispatchResult::Success
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_to_word() {
        let content = "fn main() { let foo = 42; }";

        // Cursor in "main"
        let result = expand_to_word(content, 5);
        assert_eq!(result, Some((3, 7))); // "main"

        // Cursor in "foo"
        let result = expand_to_word(content, 17);
        assert_eq!(result, Some((16, 19))); // "foo"
    }

    #[test]
    fn test_expand_to_line() {
        let content = "line one\nline two\nline three";

        // Line 0
        let result = expand_to_line(content, 0);
        assert_eq!(result, Some((0, 9))); // "line one\n"

        // Line 1
        let result = expand_to_line(content, 1);
        assert_eq!(result, Some((9, 18))); // "line two\n"

        // Line 2 (no trailing newline)
        let result = expand_to_line(content, 2);
        assert_eq!(result, Some((18, 28))); // "line three"
    }

    #[test]
    fn test_expand_to_paragraph() {
        let content = "paragraph one\nstill para one\n\nparagraph two\n";

        // In paragraph 1
        let result = expand_to_paragraph(content, 0);
        assert!(result.is_some());
        if let Some((start, end)) = result {
            assert_eq!(start, 0);
            assert!(end > 0);
        }

        // In paragraph 2
        let result = expand_to_paragraph(content, 3);
        assert!(result.is_some());
    }

    #[test]
    fn test_expansion_level_progression() {
        assert_eq!(ExpansionLevel::None.next(), ExpansionLevel::Word);
        assert_eq!(ExpansionLevel::Word.next(), ExpansionLevel::Line);
        assert_eq!(ExpansionLevel::Line.next(), ExpansionLevel::Paragraph);
        assert_eq!(ExpansionLevel::Paragraph.next(), ExpansionLevel::All);
        assert_eq!(ExpansionLevel::All.next(), ExpansionLevel::All);
    }
}
