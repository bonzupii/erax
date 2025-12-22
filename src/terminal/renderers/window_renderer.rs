use crate::core::buffer::{Buffer, BufferKind};
use crate::core::layout::Rect;
use crate::core::lexer::{Token, TokenKind};
use crate::core::spell::SpellChecker;
use crate::core::syntax::{SyntaxHighlighter, TokenType};
use crate::core::terminal_host::TerminalHost;
use crate::core::window::Window;
use crate::sed::diff::DiffState;
use crate::terminal::display::{Cell, Color, ScreenBuffer};
use crate::terminal::renderers::DirtyTracker;
use crate::terminal::theme::Theme;

/// Renders a window's content to the screen buffer
pub struct WindowRenderer;

impl WindowRenderer {
    /// Draw a window to the back buffer
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        buffer: &mut Buffer,
        window: &Window,
        rect: &Rect,
        screen_buffer: &mut ScreenBuffer,
        theme: &Theme,
        syntax_highlighter: &mut SyntaxHighlighter,
        spell_checker: &SpellChecker,
        dirty_tracker: &DirtyTracker,
        is_active: bool,
        show_line_numbers: bool,
        diff_state: Option<&DiffState>,
        terminal_host: Option<&TerminalHost>,
    ) {
        // Ensure the syntax state cache is the right size
        if buffer.syntax_state_cache.len() < buffer.line_count() {
            buffer.syntax_state_cache.resize(
                buffer.line_count(),
                crate::core::syntax::SyntaxLexerState::Normal,
            );
        }

        let text_height = rect.height.saturating_sub(1);
        let kind = buffer.buffer_kind();
        let is_original_diff = kind == BufferKind::DiffOriginal;
        let is_modified_diff = kind == BufferKind::DiffModified;

        // Special handling for Terminal buffers - render from TerminalHost grid
        if kind == BufferKind::Terminal {
            if let Some(host) = terminal_host {
                let fg = theme.fg().clone().into();
                let bg = theme.bg().clone().into();
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
                return;
            }
        }

        // Calculate selection range if mark is set
        let selection = window.mark.map(|mark| {
            let p1 = (window.cursor_y, window.cursor_x);
            let p2 = (mark.1, mark.0);
            if p1 <= p2 { (p1, p2) } else { (p2, p1) }
        });

        // Calculate gutter width if line numbers are enabled
        let gutter_width: usize = if show_line_numbers {
            let max_line = window.scroll_offset + text_height;
            let max_line_num = buffer.line_count().min(max_line).max(1);
            format!("{}", max_line_num).len() + 1
        } else {
            0
        };

        // Initialize references for rendering loop using split borrow
        let (rope, diagnostics_vec, syntax_cache, filename) = buffer.split_for_render();
        let mut line_iter = rope.lines_at(window.scroll_offset);
        let len_lines = rope.len_lines();

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
                Self::mix_colors(theme.bg().clone().into(), error_color, 0.2)
            } else if has_warning {
                let warning_color = Self::diagnostic_severity_color(
                    theme,
                    crate::core::diagnostics::DiagnosticSeverity::Warning,
                );
                Self::mix_colors(theme.bg().clone().into(), warning_color, 0.2)
            } else {
                theme.bg().clone().into()
            };

            // Diff highlighting
            if let Some(state) = diff_state {
                for hunk in &state.hunks {
                    if is_original_diff
                        && buffer_line_idx >= hunk.start_line
                        && buffer_line_idx <= hunk.end_line
                    {
                        line_bg = Self::mix_colors(line_bg, Color::Red, 0.2);
                    } else if is_modified_diff
                        && buffer_line_idx >= hunk.start_line
                        && buffer_line_idx
                            <= hunk.start_line + hunk.new_lines.len().saturating_sub(1)
                    {
                        line_bg = Self::mix_colors(line_bg, Color::Green, 0.2);
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
                            Cell::new(
                                ch,
                                theme.gutter_fg().clone().into(),
                                theme.gutter_bg().clone().into(),
                            ),
                        );
                    }
                }
            }

            // Draw diagnostic gutter icon
            if (has_error || has_warning) && gutter_width > 0 {
                let icon = if has_error { '!' } else { 'W' };
                let icon_fg = if has_error {
                    theme.error()
                } else {
                    theme.warning()
                };
                let gx = rect.x as u16 + (gutter_width as u16).saturating_sub(1);
                if gx < (rect.x + rect.width) as u16 {
                    screen_buffer.set(
                        gx,
                        screen_y,
                        Cell::new(
                            icon,
                            icon_fg.clone().into(),
                            theme.gutter_bg().clone().into(),
                        ),
                    );
                }
            }

            let text_start_x = rect.x + gutter_width;
            let text_width = rect.width.saturating_sub(gutter_width);
            let line_slice = line_iter.next();

            if let Some(slice) = line_slice {
                use std::borrow::Cow;
                let line_content: Cow<'_, str> = slice.into();
                let line_content: &str = &line_content;

                let selection_range = if let Some(((start_y, start_x), (end_y, end_x))) = selection
                {
                    if buffer_line_idx >= start_y && buffer_line_idx <= end_y {
                        let start_byte = if buffer_line_idx == start_y {
                            match crate::core::utf8::grapheme_byte_index(line_content, start_x) {
                                Some(b) => b,
                                None => 0,
                            }
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

                let extension = match filename
                    .as_ref()
                    .and_then(|p| p.extension())
                    .and_then(|e| e.to_str())
                {
                    Some(e) => e,
                    None => "",
                };

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
                    line_bg = theme.current_line_bg().clone().into();
                }

                let scroll_x = window.scroll_x;
                let mut visual_x: usize = 0;
                let mut screen_x_offset: u16 = 0;
                let mut byte_offset = 0;
                let chars: Vec<char> = line_content.chars().collect();

                let get_token_color = |offset: usize, theme: &Theme| -> Color {
                    for span in &highlight_spans {
                        if offset >= span.start && offset < span.end {
                            return Self::token_type_to_color(span.token_type, theme);
                        }
                    }
                    theme.fg().clone().into()
                };

                for ch in &chars {
                    let char_width = if *ch == '\t' {
                        let tab_width = window.tab_width;
                        tab_width - (visual_x % tab_width)
                    } else {
                        crate::core::utf8::char_width(*ch)
                    };

                    if char_width == 0 {
                        byte_offset += ch.len_utf8();
                        continue;
                    }

                    let char_visible_start = visual_x.saturating_sub(scroll_x);
                    let char_visible_end = (visual_x + char_width).saturating_sub(scroll_x);

                    if visual_x + char_width > scroll_x && visual_x < scroll_x + text_width {
                        let is_selected = selection_range
                            .as_ref()
                            .map_or(false, |r| r.contains(&byte_offset));

                        let (token_color, bg_color) = if is_selected {
                            (
                                theme.selection_fg().clone().into(),
                                theme.selection_bg().clone().into(),
                            )
                        } else {
                            let mut final_bg = line_bg;
                            if misspelled_ranges.iter().any(|r| r.contains(&byte_offset)) {
                                final_bg = Self::mix_colors(
                                    final_bg,
                                    theme.spell_tint().clone().into(),
                                    0.15,
                                );
                            }
                            (get_token_color(byte_offset, theme), final_bg)
                        };

                        if *ch == '\t' {
                            for i in 0..char_width {
                                let abs_visual_x = visual_x + i;
                                if abs_visual_x >= scroll_x && abs_visual_x < scroll_x + text_width
                                {
                                    let x = text_start_x as u16 + (abs_visual_x - scroll_x) as u16;
                                    screen_buffer.set(
                                        x,
                                        screen_y,
                                        Cell::new(' ', token_color, bg_color),
                                    );
                                }
                            }
                        } else {
                            let x = text_start_x as u16 + char_visible_start as u16;
                            if x < (text_start_x + text_width) as u16 {
                                screen_buffer.set(
                                    x,
                                    screen_y,
                                    Cell::new(*ch, token_color, bg_color),
                                );

                                if char_width > 1 {
                                    for i in 1..char_width {
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

                    visual_x += char_width;
                    byte_offset += ch.len_utf8();

                    if visual_x >= scroll_x + text_width {
                        break;
                    }
                }

                let remaining_start = text_start_x as u16 + screen_x_offset;
                for x in remaining_start..(text_start_x + text_width) as u16 {
                    screen_buffer.set(
                        x,
                        screen_y,
                        Cell::new(' ', theme.fg().clone().into(), line_bg),
                    );
                }
            } else {
                let screen_x = text_start_x as u16;
                screen_buffer.set(
                    screen_x,
                    screen_y,
                    Cell::new(
                        '~',
                        Color::Rgb {
                            r: 100,
                            g: 100,
                            b: 200,
                        },
                        line_bg,
                    ),
                );
                for x in (screen_x + 1)..(text_start_x + text_width) as u16 {
                    screen_buffer.set(
                        x,
                        screen_y,
                        Cell::new(' ', theme.fg().clone().into(), line_bg),
                    );
                }
            }
        }

        // Render vertical scrollbar
        let track_bg = theme.bg().clone().into();
        let track_fg: Color = theme.scrollbar_track().clone().into();
        let thumb_fg: Color = theme.scrollbar_thumb().clone().into();

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
        let visible_start = window.scroll_offset;
        let visible_end =
            (window.scroll_offset + rect.height.saturating_sub(1)).min(buffer.line_count());
        let mut max_line_width: usize = 0;
        for line_idx in visible_start..visible_end {
            if let Some(line) = buffer.line(line_idx) {
                let width = crate::core::utf8::visual_width(&line, window.tab_width);
                max_line_width = max_line_width.max(width);
            }
        }

        let text_width = rect.width.saturating_sub(gutter_width + 1);
        crate::terminal::scrollbar::render_horizontal(
            screen_buffer,
            rect,
            gutter_width,
            max_line_width.max(window.scroll_x + text_width),
            text_width,
            window.scroll_x,
            track_fg,
            thumb_fg,
            track_bg,
        );
    }

    fn diagnostic_severity_color(
        theme: &Theme,
        severity: crate::core::diagnostics::DiagnosticSeverity,
    ) -> Color {
        use crate::core::diagnostics::DiagnosticSeverity;
        match severity {
            DiagnosticSeverity::Error => theme.red().clone().into(),
            DiagnosticSeverity::Warning => theme.orange().clone().into(),
            DiagnosticSeverity::Info => theme.info().clone().into(),
            DiagnosticSeverity::Note => theme.blue().clone().into(),
        }
    }

    fn token_type_to_color(token_type: TokenType, theme: &Theme) -> Color {
        match token_type {
            TokenType::Keyword => theme.keyword().clone().into(),
            TokenType::Type => theme.type_name().clone().into(),
            TokenType::String => theme.string().clone().into(),
            TokenType::Char => theme.char().clone().into(),
            TokenType::Number => theme.number().clone().into(),
            TokenType::Comment => theme.comment().clone().into(),
            TokenType::Preprocessor => theme.preprocessor().clone().into(),
            TokenType::Function => theme.function().clone().into(),
            TokenType::Operator => theme.operator().clone().into(),
            TokenType::Punctuation => theme.punctuation().clone().into(),
            TokenType::Normal => theme.normal().clone().into(),
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
