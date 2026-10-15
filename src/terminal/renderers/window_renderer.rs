//! Window-content rendering for TUI mode.
use crate::core::buffer::{Buffer, BufferKind};
use crate::core::layout::Rect;
use crate::core::lexer::{Token, TokenKind};
use crate::core::spell::SpellChecker;
use crate::core::syntax::{SyntaxHighlighter, TokenType};
use crate::core::terminal_host::TerminalHost;
use crate::core::window::Window;
use crate::core::commands::diff::DiffNavState;
use crate::terminal::display::{Cell, CellRole, Color, ScreenBuffer};
use crate::terminal::renderers::DirtyTracker;
use crate::terminal::theme::Theme;
use unicode_segmentation::UnicodeSegmentation;

/// Renders a window's content to the screen buffer
pub struct WindowRenderer;

/// Parameters for [`WindowRenderer::render`]: one struct instead of twelve
/// arguments.
pub struct WindowRenderParams<'a> {
    pub buffer: &'a mut Buffer,
    pub window: &'a Window,
    pub rect: &'a Rect,
    pub screen_buffer: &'a mut ScreenBuffer,
    pub theme: &'a Theme,
    pub syntax_highlighter: &'a mut SyntaxHighlighter,
    pub spell_checker: &'a SpellChecker,
    pub dirty_tracker: &'a DirtyTracker,
    pub is_active: bool,
    pub show_line_numbers: bool,
    pub diff_state: Option<&'a DiffNavState>,
    pub terminal_host: Option<&'a TerminalHost>,
}

impl WindowRenderer {
    /// Draw a window to the back buffer
    /// Returns the maximum content width of visible lines (for caching)
    pub fn render(p: WindowRenderParams<'_>) -> usize {
        let WindowRenderParams {
            buffer,
            window,
            rect,
            screen_buffer,
            theme,
            syntax_highlighter,
            spell_checker,
            dirty_tracker,
            is_active,
            show_line_numbers,
            diff_state,
            terminal_host,
        } = p;
        // Ensure the syntax state cache is the right size
        if buffer.syntax_state_cache.len() < buffer.line_count() {
            buffer.syntax_state_cache.resize(
                buffer.line_count(),
                crate::core::syntax::SyntaxLexerState::Normal,
            );
        }

        // Pre-calculate gutter width to determine text area width
        let pre_text_height = rect.height.saturating_sub(1);
        let gutter_width_estimate: usize = if show_line_numbers {
            let max_line = window.scroll_offset + pre_text_height;
            let max_line_num = buffer.line_count().min(max_line).max(1);
            format!("{}", max_line_num).len() + 1
        } else {
            0
        };

        // Calculate text width for horizontal scrollbar decision
        let text_width_estimate = rect.width.saturating_sub(gutter_width_estimate + 1);

        // Determine if horizontal scrollbar will be shown
        // (based on cached_content_width or if we're scrolled horizontally)
        let needs_hscroll =
            window.cached_content_width > text_width_estimate || window.scroll_x > 0;

        // Reserve extra row for horizontal scrollbar if needed
        let text_height = if needs_hscroll {
            rect.height.saturating_sub(2) // 1 for status, 1 for hscroll
        } else {
            rect.height.saturating_sub(1) // 1 for status only
        };

        let kind = buffer.buffer_kind();
        let is_original_diff = kind == BufferKind::DiffOriginal;
        let is_modified_diff = kind == BufferKind::DiffModified;

        // Special handling for Terminal buffers - render from TerminalHost grid
        if kind == BufferKind::Terminal
            && let Some(host) = terminal_host {
                let fg = theme.fg;
                let bg = theme.bg;
                let grid = host.grid();

                for y in 0..text_height.min(host.rows as usize) {
                    let screen_y = (rect.y + y) as u16;
                    // Optimization: check dirty tracker (though terminal usually needs full redraw)
                    // if !dirty_tracker.is_row_dirty(screen_y as usize) { continue; }

                    if y < grid.len() {
                        for x in 0..rect.width.min(host.cols as usize) {
                            let screen_x = (rect.x + x) as u16;
                            if x < grid[y].len() {
                                let ch = grid[y][x];
                                screen_buffer.set(screen_x, screen_y, Cell::new(ch, fg, bg));
                            }
                        }
                    }
                }
                // Status line is handled by StatusRenderer (caller responsibility)
                return 0; // Terminal buffers don't need horizontal scrollbar caching
            }

        // Calculate selection range if mark is set
        let selection = window.mark.map(|mark| {
            let p1 = (window.cursor_y, window.cursor_x);
            let p2 = (mark.1, mark.0);
            if p1 <= p2 { (p1, p2) } else { (p2, p1) }
        });

        // Calculate gutter width if line numbers are enabled (now accurate with text_height)
        let gutter_width: usize = if show_line_numbers {
            let max_line = window.scroll_offset + text_height;
            let max_line_num = buffer.line_count().min(max_line).max(1);
            format!("{}", max_line_num).len() + 1
        } else {
            0
        };

        // Line count first: split_for_render holds a borrow of the buffer.
        // (On a pager failure line_count records the error on the buffer and
        // returns 0; the line iterator below then simply yields nothing.)
        let len_lines = buffer.line_count();

        // Initialize references for rendering loop using split borrow
        let (text, diagnostics_vec, syntax_cache, filename) = buffer.split_for_render();
        // Lines are byte-exact in the tree; the lossy view is only for
        // display (invalid UTF-8 becomes U+FFFD without touching content).
        // Unreadable lines (mapped files, pager failure) are skipped — never
        // a panic in the render loop. (The line_count() probe above records
        // the error on the buffer in the overwhelmingly common case, where a
        // failed content read also fails the page-metrics read.)
        let mut line_iter = text.lines_at(window.scroll_offset);

        for y in 0..text_height {
            let buffer_line_idx = window.scroll_offset + y;
            let screen_y = (rect.y + y) as u16;

            // Optimization: Skip rendering if row is not dirty
            // Note: caller handles copying from front buffer if skipped
            if !dirty_tracker.is_row_dirty(screen_y as usize) {
                let _ = line_iter.next();
                continue;
            }

            // Check for diagnostics on this line
            let line_diagnostics: Vec<&crate::core::diagnostics::Diagnostic> = diagnostics_vec
                .iter()
                .filter(|d| d.line == buffer_line_idx + 1)
                .collect();

            let has_error = line_diagnostics.iter().any(|d| d.is_error());
            let has_warning = line_diagnostics.iter().any(|d| d.is_warning());

            // Use diagnostic severity colors from Theme
            let mut line_bg = if has_error {
                let error_color = Self::diagnostic_severity_color(
                    theme,
                    crate::core::diagnostics::DiagnosticSeverity::Error,
                );
                Self::mix_colors(theme.bg, error_color, 0.2)
            } else if has_warning {
                let warning_color = Self::diagnostic_severity_color(
                    theme,
                    crate::core::diagnostics::DiagnosticSeverity::Warning,
                );
                Self::mix_colors(theme.bg, warning_color, 0.2)
            } else {
                theme.bg
            };

            // Diff highlighting
            if let Some(state) = diff_state {
                for (old_start, old_end, new_start, new_end, _) in &state.hunks {
                    // Spans are 1-based, end exclusive; buffer_line_idx is 0-based.
                    if is_original_diff
                        && buffer_line_idx + 1 >= *old_start
                        && buffer_line_idx + 1 < *old_end
                    {
                        line_bg = Self::mix_colors(line_bg, theme.diff_removed_bg, 0.2);
                    } else if is_modified_diff
                        && buffer_line_idx + 1 >= *new_start
                        && buffer_line_idx + 1 < *new_end
                    {
                        line_bg = Self::mix_colors(line_bg, theme.diff_added_bg, 0.2);
                    }
                }
            }

            // Draw line number in gutter
            if show_line_numbers && gutter_width > 0 {
                let line_num_str = if buffer_line_idx < len_lines {
                    format!("{:>width$}│", buffer_line_idx + 1, width = gutter_width - 1)
                } else {
                    format!("{}│", " ".repeat(gutter_width - 1))
                };
                for (i, ch) in line_num_str.chars().enumerate() {
                    let gx = rect.x as u16 + i as u16;
                    if gx < (rect.x + rect.width) as u16 {
                        screen_buffer.set(
                            gx,
                            screen_y,
                            Cell::with_role(
                                ch,
                                theme.gutter_fg,
                                theme.gutter_bg,
                                CellRole::Gutter,
                            ),
                        );
                    }
                }
            }

            // Draw diagnostic gutter icon
            if (has_error || has_warning) && gutter_width > 0 {
                let icon = if has_error { '!' } else { 'W' };
                let icon_fg = if has_error {
                    theme.error
                } else {
                    theme.warning
                };
                let gx = rect.x as u16 + (gutter_width as u16).saturating_sub(1);
                if gx < (rect.x + rect.width) as u16 {
                    screen_buffer.set(
                        gx,
                        screen_y,
                        Cell::new(
                            icon,
                            icon_fg,
                            theme.gutter_bg,
                        ),
                    );
                }
            }

            let text_start_x = rect.x + gutter_width;
            let text_width = rect.width.saturating_sub(gutter_width);
            let line_slice = line_iter.next();

            // `owned_line` extends the lifetime of mapped-region lines (which
            // the pager materializes per read) to the end of this iteration;
            // owned-region lines borrow straight from the tree — zero-copy,
            // exactly like the old ropey `RopeSlice` path.
            let owned_line: Vec<u8>;
            let line_bytes: Option<&[u8]> = match line_slice {
                Some(Ok(crate::core::btext::LineRef::Borrowed(b))) => Some(b),
                Some(Ok(crate::core::btext::LineRef::Owned(v))) => {
                    owned_line = v;
                    Some(&owned_line)
                }
                _ => None,
            };

            if let Some(line_bytes) = line_bytes {
                use std::borrow::Cow;
                let line_content: Cow<'_, str> = String::from_utf8_lossy(line_bytes);
                let line_content: &str = &line_content;

                let selection_range = if let Some(((start_y, start_x), (end_y, end_x))) = selection
                {
                    if buffer_line_idx >= start_y && buffer_line_idx <= end_y {
                        let start_byte = if buffer_line_idx == start_y {
                            crate::core::utf8::grapheme_byte_index(line_content, start_x).unwrap_or_default()
                        } else {
                            0
                        };

                        let end_byte = if buffer_line_idx == end_y {
                            match crate::core::utf8::grapheme_byte_index(line_content, end_x) {
                                Some(b) => b,
                                None => line_content.len(),
                            }
                        } else {
                            line_content.len()
                        };

                        if start_y == end_y && start_x == end_x {
                            None
                        } else {
                            Some(start_byte..end_byte)
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                let extension = filename
                    .as_ref()
                    .and_then(|p| p.extension())
                    .and_then(|e| e.to_str())
                    .unwrap_or_default();

                let current_state = match syntax_cache.get(buffer_line_idx).copied() {
                    Some(s) => s,
                    None => crate::core::syntax::SyntaxLexerState::Normal,
                };

                let (highlight_spans, next_state) = syntax_highlighter.highlight_line_with_state(
                    extension,
                    line_content,
                    current_state,
                );

                let mut misspelled_ranges = Vec::new();
                for span in &highlight_spans {
                    let kind = match span.token_type {
                        TokenType::Comment => TokenKind::Comment,
                        TokenType::String => TokenKind::String,
                        _ => continue,
                    };

                    if let Some(token_text) = line_content.get(span.start..span.end) {
                        let token = Token::new(kind, token_text, span.start);
                        let misspellings = spell_checker.check_token(&token);
                        for m in misspellings {
                            misspelled_ranges.push(m.start..m.end);
                        }
                    }
                }

                let next_line_idx = buffer_line_idx + 1;
                if next_line_idx < len_lines {
                    if next_line_idx < syntax_cache.len() {
                        syntax_cache[next_line_idx] = next_state;
                    } else {
                        syntax_cache.resize(
                            next_line_idx + 1,
                            crate::core::syntax::SyntaxLexerState::Normal,
                        );
                        syntax_cache[next_line_idx] = next_state;
                    }
                }

                if is_active && buffer_line_idx == window.cursor_y {
                    line_bg = theme.current_line_bg;
                }

                let scroll_x = window.scroll_x;
                let mut visual_x: usize = 0;
                let mut screen_x_offset: u16 = 0;
                let mut byte_offset = 0;

                let get_token_color = |offset: usize, theme: &Theme| -> Color {
                    for span in &highlight_spans {
                        if offset >= span.start && offset < span.end {
                            return Self::token_type_to_color(span.token_type, theme);
                        }
                    }
                    theme.fg
                };

                // One cell per grapheme cluster: combining marks ride on
                // their base character inside the same cell, so the emitted
                // bytes match what `cat` would print and the width model
                // (`layout_grapheme_width`) agrees with the cells.
                for grapheme in line_content.graphemes(true) {
                    let cluster_width = if grapheme == "\t" {
                        let tab_width = window.tab_width;
                        tab_width - (visual_x % tab_width)
                    } else {
                        let w = crate::core::utf8::layout_grapheme_width(grapheme);
                        if w == 0 {
                            // Control characters never reach the terminal.
                            byte_offset += grapheme.len();
                            continue;
                        }
                        w
                    };

                    let char_visible_start = visual_x.saturating_sub(scroll_x);
                    let char_visible_end = (visual_x + cluster_width).saturating_sub(scroll_x);

                    if visual_x + cluster_width > scroll_x && visual_x < scroll_x + text_width {
                        let is_selected = selection_range
                            .as_ref()
                            .is_some_and(|r| r.contains(&byte_offset));

                        let (token_color, bg_color) = if is_selected {
                            (
                                theme.selection_fg,
                                theme.selection_bg,
                            )
                        } else {
                            let mut final_bg = line_bg;
                            if misspelled_ranges.iter().any(|r| r.contains(&byte_offset)) {
                                final_bg = Self::mix_colors(
                                    final_bg,
                                    theme.spell_tint,
                                    0.15,
                                );
                            }
                            (get_token_color(byte_offset, theme), final_bg)
                        };

                        if grapheme == "\t" {
                            for i in 0..cluster_width {
                                let abs_visual_x = visual_x + i;
                                if abs_visual_x >= scroll_x && abs_visual_x < scroll_x + text_width
                                {
                                    let x = text_start_x as u16 + (abs_visual_x - scroll_x) as u16;
                                    let tab_role = if is_selected {
                                        CellRole::Selection
                                    } else {
                                        CellRole::EditorText
                                    };
                                    screen_buffer.set(
                                        x,
                                        screen_y,
                                        Cell::with_role(' ', token_color, bg_color, tab_role),
                                    );
                                }
                            }
                        } else {
                            let x = text_start_x as u16 + char_visible_start as u16;
                            let text_role = if is_selected {
                                CellRole::Selection
                            } else {
                                CellRole::EditorText
                            };
                            if x < (text_start_x + text_width) as u16 {
                                screen_buffer.set(
                                    x,
                                    screen_y,
                                    Cell::with_text(grapheme, token_color, bg_color, text_role),
                                );

                                if cluster_width > 1 {
                                    for i in 1..cluster_width {
                                        let wide_x = x + i as u16;
                                        if wide_x < (text_start_x + text_width) as u16 {
                                            screen_buffer.set(wide_x, screen_y, Cell::hidden());
                                            // The implementation of Cell::hidden wasn't public.
                                            // For now Cell::empty with hidden flag logic.
                                            // Wait, I saw Cell definition, it has hidden.
                                            // I should make it public or use a constructor.
                                        }
                                    }
                                }
                            }
                        }
                        screen_x_offset = char_visible_end as u16;
                    }

                    visual_x += cluster_width;
                    byte_offset += grapheme.len();

                    if visual_x >= scroll_x + text_width {
                        break;
                    }
                }

                let remaining_start = text_start_x as u16 + screen_x_offset;
                for x in remaining_start..(text_start_x + text_width) as u16 {
                    screen_buffer.set(
                        x,
                        screen_y,
                        Cell::new(' ', theme.fg, line_bg),
                    );
                }
            } else {
                let screen_x = text_start_x as u16;
                screen_buffer.set(
                    screen_x,
                    screen_y,
                    Cell::new('~', theme.gutter_fg, line_bg),
                );
                for x in (screen_x + 1)..(text_start_x + text_width) as u16 {
                    screen_buffer.set(
                        x,
                        screen_y,
                        Cell::new(' ', theme.fg, line_bg),
                    );
                }
            }
        }

        // Render vertical scrollbar
        let track_bg = theme.bg;
        let track_fg: Color = theme.scrollbar_track;
        let thumb_fg: Color = theme.scrollbar_thumb;

        crate::terminal::scrollbar::render_vertical(
            screen_buffer,
            rect,
            buffer.line_count(),
            window.scroll_offset,
            track_fg,
            thumb_fg,
            track_bg,
        );

        // Render horizontal scrollbar
        // Update cached content width with max of visible lines (grows but doesn't shrink rapidly)
        let visible_start = window.scroll_offset;
        let visible_end =
            (window.scroll_offset + rect.height.saturating_sub(1)).min(buffer.line_count());
        let mut visible_max_width: usize = 0;
        for line_idx in visible_start..visible_end {
            if let Some(line) = buffer.line(line_idx) {
                let width = crate::core::utf8::visual_width(&line, window.tab_width);
                visible_max_width = visible_max_width.max(width);
            }
        }
        // Keep the maximum width ever seen (only grows, not shrinks, for stable scrollbar)
        let content_width = window.cached_content_width.max(visible_max_width);
        // Note: can't mutate window here, caller should update cached_content_width after render

        let text_width = rect.width.saturating_sub(gutter_width + 1);
        crate::terminal::scrollbar::render_horizontal(
            screen_buffer,
            rect,
            &crate::terminal::scrollbar::HorizontalScrollbarParams {
                gutter_width,
                content_width: content_width.max(window.scroll_x + text_width),
                text_width,
                scroll_x: window.scroll_x,
                track_fg,
                thumb_fg,
                track_bg,
            },
        );

        // Return the visible max width for caching by caller
        visible_max_width
    }

    fn diagnostic_severity_color(
        theme: &Theme,
        severity: crate::core::diagnostics::DiagnosticSeverity,
    ) -> Color {
        use crate::core::diagnostics::DiagnosticSeverity;
        match severity {
            DiagnosticSeverity::Error => theme.error,
            DiagnosticSeverity::Warning => theme.warning,
            DiagnosticSeverity::Info => theme.info,
            DiagnosticSeverity::Note => theme.info,
        }
    }

    fn token_type_to_color(token_type: TokenType, theme: &Theme) -> Color {
        match token_type {
            TokenType::Keyword => theme.keyword,
            TokenType::Type => theme.type_name,
            TokenType::String => theme.string,
            TokenType::Char => theme.char_lit,
            TokenType::Number => theme.number,
            TokenType::Comment => theme.comment,
            TokenType::Preprocessor => theme.preprocessor,
            TokenType::Function => theme.function,
            TokenType::Operator => theme.operator,
            TokenType::Punctuation => theme.punctuation,
            TokenType::Normal => theme.fg,
        }
    }

    fn mix_colors(base: Color, tint: Color, alpha: f32) -> Color {
        let b = base.to_rgba_f32();
        let t = tint.to_rgba_f32();

        let r = (b[0] * (1.0 - alpha) + t[0] * alpha) * 255.0;
        let g = (b[1] * (1.0 - alpha) + t[1] * alpha) * 255.0;
        let b_val = (b[2] * (1.0 - alpha) + t[2] * alpha) * 255.0;

        Color::Rgb {
            r: r as u8,
            g: g as u8,
            b: b_val as u8,
        }
    }
}
