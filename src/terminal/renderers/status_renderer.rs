//! Status-line rendering for TUI mode.
use crate::core::buffer::Buffer;
use crate::core::layout::Rect;
use crate::core::window::Window;
use crate::terminal::display::{BarOwner, BarState, Cell, CellRole, ScreenBuffer};
use crate::terminal::prompt::PromptState;
use crate::terminal::theme::Theme;
use unicode_segmentation::UnicodeSegmentation;

/// Everything the status renderer needs for one window's status line.
pub struct StatusCtx<'a> {
    pub window: &'a Window,
    pub buffer: &'a Buffer,
    pub rect: &'a Rect,
    pub theme: &'a Theme,
    pub is_active: bool,
    pub prompt_state: Option<&'a PromptState>,
    /// Screen-model bar state (N-273/D6): ownership arbitration result, mode
    /// segment, pending keys, command line. Composed from backend state by
    /// the screen model; the renderer does no mode logic.
    pub bar: &'a BarState,
}

/// Renders the status line for a window
pub struct StatusRenderer;

impl StatusRenderer {
    /// Render status line to the screen buffer.
    ///
    /// Layout: `{left}{mode segment}{middle}{padding}{right}`, where the
    /// middle is whichever bar owner won the arbitration (prompt, vim `:`
    /// command line, message, or the model's echo/mode line).
    ///
    /// Returns the screen coordinates of the `:` command-line cursor when the
    /// modal command line owns the bar, so the caller can place the hardware
    /// cursor there; `None` otherwise.
    pub fn render(screen_buffer: &mut ScreenBuffer, ctx: &StatusCtx<'_>) -> Option<(u16, u16)> {
        let StatusCtx {
            window,
            buffer,
            rect,
            theme,
            is_active,
            prompt_state,
            bar,
        } = ctx;
        let status_y = (rect.y + rect.height.saturating_sub(1)) as u16;
        let bg_color = if *is_active {
            theme.status_bg
        } else {
            theme.status_inactive_bg
        };
        let fg_color = if *is_active {
            theme.status_fg
        } else {
            theme.status_inactive_fg
        };

        let filename = match buffer.filename.as_ref().map(|p| p.display().to_string()) {
            Some(s) => s,
            None => "[No Name]".to_string(),
        };

        let modified = if buffer.modified { "[+]" } else { "" };
        let left = format!(" {} {} ", filename, modified);

        // Right: position readout. The Standard model appends the active
        // selection length, VS Code-style: `Ln X, Col Y (Z selected)`.
        let mut pos_info = format!("Ln {}, Col {}", window.cursor_y + 1, window.cursor_x + 1);
        if let Some(n) = bar.selection_len {
            pos_info.push_str(&format!(" ({} selected)", n));
        }
        let right = format!(" {} ", pos_info);

        // Middle: whoever owns the bar. Inactive windows show no bar content
        // (unchanged from the classic renderer).
        let mut cmd_cursor: Option<(u16, u16)> = None;
        let (seg, middle, middle_role) = if *is_active {
            match bar.owner {
                BarOwner::Prompt => match prompt_state {
                    Some(ps) => (
                        String::new(),
                        format!("{}{}", ps.prompt, ps.input),
                        CellRole::Prompt,
                    ),
                    // Arbitration said Prompt but the prompt vanished; render
                    // nothing rather than stale content.
                    None => (String::new(), String::new(), CellRole::StatusBar),
                },
                BarOwner::CommandLine => {
                    let text = format!(":{}", bar.cmdline_text);
                    // Command-line cursor in grapheme units — the same units
                    // the cell loop below uses.
                    let cursor_graphemes = bar
                        .cmdline_text
                        .get(..bar.cmdline_cursor.min(bar.cmdline_text.len()))
                        .map(|s| s.graphemes(true).count())
                        .unwrap_or(0);
                    let cursor_i = left.graphemes(true).count() + 1 + cursor_graphemes;
                    let screen_x = rect.x + cursor_i;
                    if screen_x < rect.x + rect.width {
                        cmd_cursor = Some((screen_x as u16, status_y));
                    }
                    (String::new(), text, CellRole::Prompt)
                }
                // A message covers the mode segment while it is up; the
                // segment returns when the message clears (it is never
                // permanently clobbered).
                BarOwner::Message => (String::new(), bar.message.clone(), CellRole::MessageLine),
                BarOwner::ModeLine => {
                    let seg = bar
                        .mode_segment
                        .map(|s| format!("{} ", s))
                        .unwrap_or_default();
                    let echo = if !bar.pending.is_empty() {
                        bar.pending.clone()
                    } else {
                        bar.key_sequence.clone()
                    };
                    (seg, echo, CellRole::MessageLine)
                }
            }
        } else {
            (String::new(), String::new(), CellRole::StatusBar)
        };

        let used_len = left.len() + seg.len() + middle.len() + right.len();
        let padding = rect.width.saturating_sub(used_len);
        let full_status = format!(
            "{}{}{}{}{}",
            left,
            seg,
            middle,
            " ".repeat(padding),
            right
        );

        // Render status line, stamping the echo region's semantic role (N-273).
        // The mode segment itself is status chrome (StatusBar role).
        // Grapheme-granular: a non-ASCII filename must not desync the
        // role boundaries or the cell loop.
        let seg_end = left.graphemes(true).count() + seg.graphemes(true).count();
        let middle_end = seg_end + middle.graphemes(true).count();
        for (i, grapheme) in full_status.graphemes(true).take(rect.width).enumerate() {
            let screen_x = (rect.x + i) as u16;
            let role = if i >= seg_end && i < middle_end {
                middle_role
            } else {
                CellRole::StatusBar
            };
            screen_buffer.set(
                screen_x,
                status_y,
                Cell::with_text(grapheme, fg_color, bg_color, role),
            );
        }

        cmd_cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::keys::{InputModel, ModalMode};
    use crate::core::id::{BufferId, WindowId};
    use crate::terminal::display::{
        bar_owner_for, compose_bar_state, cursor_style_for, mode_segment_for,
    };
    use crate::terminal::display::{BarState, Display};

    /// Render one status line and read it back as a string.
    fn render_bar(
        display: &mut Display,
        window: &Window,
        buffer: &Buffer,
        rect: &Rect,
        bar: &BarState,
        prompt: Option<&PromptState>,
    ) -> (String, Option<(u16, u16)>) {
        let theme = display.theme;
        let ctx = StatusCtx {
            window,
            buffer,
            rect,
            theme: &theme,
            is_active: true,
            prompt_state: prompt,
            bar,
        };
        let cursor = StatusRenderer::render(&mut display.back_buffer, &ctx);
        let y = (rect.y + rect.height - 1) as u16;
        let line: String = (0..rect.width as u16)
            .map(|x| display.back_buffer.get(x, y).map(|c| c.text()).unwrap_or(" "))
            .collect();
        (line, cursor)
    }

    fn window_and_buffer(text: &str) -> (Window, Buffer) {
        let window = Window::new(WindowId(0), BufferId(0), 4);
        let buffer = Buffer::from_string(text);
        (window, buffer)
    }

    /// Drive the modal engine through `keys` (bypassing the compile-time
    /// input-model gate, like the engine's own tests) and compose the bar
    /// state for the Modal model.
    fn modal_bar(
        engine: &crate::core::modal::ModalEngine,
        message: &str,
    ) -> BarState {
        compose_bar_state(
            InputModel::Modal,
            engine,
            None,
            message,
            "",
            None,
            None,
        )
    }

    fn press(engine: &mut crate::core::modal::ModalEngine, app: &mut crate::core::app::EditorApp, key: crate::core::input::KeyPress) {
        let _ = engine.handle_key_inner(app, &key);
    }

    fn kp(c: char) -> crate::core::input::KeyPress {
        crate::core::input::KeyPress::plain(crate::core::input::Key::Char(c))
    }

    fn esc() -> crate::core::input::KeyPress {
        crate::core::input::KeyPress::plain(crate::core::input::Key::Esc)
    }

    #[test]
    fn golden_modal_normal_shows_mode_segment() {
        let mut display = Display::new(48, 10, None, None);
        let (window, buffer) = window_and_buffer("hello\n");
        let rect = Rect::new(0, 0, 48, 10);
        let engine = crate::core::modal::ModalEngine::new();
        let bar = modal_bar(&engine, "");
        let (line, cursor) = render_bar(&mut display, &window, &buffer, &rect, &bar, None);
        assert_eq!(cursor, None);
        assert!(
            line.starts_with(" [No Name]  -- NORMAL -- "),
            "unexpected bar: {line:?}"
        );
        assert!(line.ends_with(" Ln 1, Col 1 "), "unexpected bar: {line:?}");
    }

    #[test]
    fn golden_modal_insert_and_visual_segments() {
        let mut display = Display::new(48, 10, None, None);
        let (window, buffer) = window_and_buffer("hello\n");
        let rect = Rect::new(0, 0, 48, 10);
        let mut app = crate::core::app::EditorApp::new();
        let mut engine = crate::core::modal::ModalEngine::new();

        press(&mut engine, &mut app, kp('i'));
        let bar = modal_bar(&engine, "");
        let (line, _) = render_bar(&mut display, &window, &buffer, &rect, &bar, None);
        assert!(line.contains("-- INSERT --"), "unexpected bar: {line:?}");

        press(&mut engine, &mut app, esc());
        press(&mut engine, &mut app, kp('v'));
        let bar = modal_bar(&engine, "");
        let (line, _) = render_bar(&mut display, &window, &buffer, &rect, &bar, None);
        assert!(line.contains("-- VISUAL --"), "unexpected bar: {line:?}");

        press(&mut engine, &mut app, esc());
        press(&mut engine, &mut app, kp('V'));
        let bar = modal_bar(&engine, "");
        let (line, _) = render_bar(&mut display, &window, &buffer, &rect, &bar, None);
        assert!(line.contains("-- VISUAL LINE --"), "unexpected bar: {line:?}");
    }

    #[test]
    fn golden_operator_pending_shows_count_and_operator() {
        let mut display = Display::new(48, 10, None, None);
        let (window, buffer) = window_and_buffer("hello world\n");
        let rect = Rect::new(0, 0, 48, 10);
        let mut app = crate::core::app::EditorApp::new();
        let mut engine = crate::core::modal::ModalEngine::new();

        press(&mut engine, &mut app, kp('d'));
        press(&mut engine, &mut app, kp('3'));
        assert_eq!(engine.mode(), ModalMode::OperatorPending);
        let bar = modal_bar(&engine, "");
        let (line, _) = render_bar(&mut display, &window, &buffer, &rect, &bar, None);
        // Mode segment stays (OperatorPending is a Normal sub-state) and the
        // pending count+operator follows it in a consistent position.
        assert!(
            line.starts_with(" [No Name]  -- NORMAL -- 3d"),
            "unexpected bar: {line:?}"
        );

        // Register prefix: `"a2c`.
        use crate::core::input::{Key, KeyPress};
        let esc = KeyPress::plain(Key::Esc);
        engine.handle_key_inner(&mut app, &esc);
        for c in ['"', 'a', '2', 'c'] {
            press(&mut engine, &mut app, kp(c));
        }
        let bar = modal_bar(&engine, "");
        let (line, _) = render_bar(&mut display, &window, &buffer, &rect, &bar, None);
        assert!(
            line.contains("-- NORMAL -- \"a2c"),
            "unexpected bar: {line:?}"
        );
    }

    #[test]
    fn golden_command_line_owns_bar_with_cursor() {
        let mut display = Display::new(48, 10, None, None);
        let (window, buffer) = window_and_buffer("hello\n");
        let rect = Rect::new(0, 0, 48, 10);
        let mut app = crate::core::app::EditorApp::new();
        let mut engine = crate::core::modal::ModalEngine::new();

        press(&mut engine, &mut app, kp(':'));
        press(&mut engine, &mut app, kp('w'));
        let bar = modal_bar(&engine, "");
        assert_eq!(bar.owner, crate::terminal::display::BarOwner::CommandLine);
        let (line, cursor) = render_bar(&mut display, &window, &buffer, &rect, &bar, None);
        assert!(
            line.starts_with(" [No Name]  :w"),
            "unexpected bar: {line:?}"
        );
        // No mode segment while the command line owns the bar.
        assert!(!line.contains("--"), "unexpected bar: {line:?}");
        // Hardware cursor lands after `:w` on the status row.
        let left_len = " [No Name]  ".chars().count();
        assert_eq!(cursor, Some(((left_len + 2) as u16, 9)));
        // The command-line cells carry the Prompt role (an input area).
        let cell = display.back_buffer.get((left_len + 1) as u16, 9).unwrap();
        assert_eq!(cell.role, CellRole::Prompt);
    }

    #[test]
    fn golden_message_covers_then_mode_segment_returns() {
        let mut display = Display::new(48, 10, None, None);
        let (window, buffer) = window_and_buffer("hello\n");
        let rect = Rect::new(0, 0, 48, 10);
        let engine = crate::core::modal::ModalEngine::new();

        // A message shows in the bar without the mode segment…
        let bar = modal_bar(&engine, "Not an editor command: zzz");
        assert_eq!(bar.owner, crate::terminal::display::BarOwner::Message);
        let (line, _) = render_bar(&mut display, &window, &buffer, &rect, &bar, None);
        assert!(line.contains("Not an editor command: zzz"), "unexpected bar: {line:?}");
        assert!(!line.contains("-- NORMAL --"), "unexpected bar: {line:?}");

        // …and the mode segment returns once the message clears: it was
        // covered, never clobbered.
        let bar = modal_bar(&engine, "");
        let (line, _) = render_bar(&mut display, &window, &buffer, &rect, &bar, None);
        assert!(line.contains("-- NORMAL --"), "unexpected bar: {line:?}");
    }

    #[test]
    fn golden_standard_selection_readout_no_mode_segment() {
        use crate::core::app::EditorApp;
        let mut display = Display::new(48, 10, None, None);
        let rect = Rect::new(0, 0, 48, 10);
        let mut app = EditorApp::new();
        app.active_buffer_mut().unwrap().insert(0, "hello\n");
        {
            let w = app.active_window_mut().unwrap();
            w.cursor_x = 5;
            w.cursor_y = 0;
            w.mark = Some((2, 0));
        }
        let window = app.active_window_ref().unwrap();
        let buffer = app.active_buffer().unwrap();
        let bar = compose_bar_state(
            InputModel::Standard,
            &app.modal_engine,
            None,
            "",
            "",
            Some(window),
            Some(buffer),
        );
        assert_eq!(bar.selection_len, Some(3));
        assert_eq!(bar.mode_segment, None);
        let (line, _) = render_bar(&mut display, window, buffer, &rect, &bar, None);
        assert!(
            line.ends_with(" Ln 1, Col 6 (3 selected) "),
            "unexpected bar: {line:?}"
        );
        assert!(!line.contains("--"), "unexpected bar: {line:?}");

        // No selection: plain readout.
        {
            let w = app.active_window_mut().unwrap();
            w.mark = None;
        }
        let window = app.active_window_ref().unwrap();
        let buffer = app.active_buffer().unwrap();
        let bar = compose_bar_state(
            InputModel::Standard,
            &app.modal_engine,
            None,
            "",
            "",
            Some(window),
            Some(buffer),
        );
        assert_eq!(bar.selection_len, None);
        let (line, _) = render_bar(&mut display, window, buffer, &rect, &bar, None);
        assert!(line.ends_with(" Ln 1, Col 6 "), "unexpected bar: {line:?}");
    }

    #[test]
    fn golden_keytrie_echo_area_unchanged() {
        let mut display = Display::new(48, 10, None, None);
        let (window, buffer) = window_and_buffer("hello\n");
        let rect = Rect::new(0, 0, 48, 10);
        let engine = crate::core::modal::ModalEngine::new();
        // Classic behavior: pending chord in the echo area, no mode segment.
        let bar = compose_bar_state(InputModel::KeyTrie, &engine, None, "", "C-x ", None, None);
        let (line, _) = render_bar(&mut display, &window, &buffer, &rect, &bar, None);
        assert!(line.contains("C-x"), "unexpected bar: {line:?}");
        assert!(!line.contains("--"), "unexpected bar: {line:?}");
        // Cursor style unchanged: steady block.
        assert_eq!(
            cursor_style_for(InputModel::KeyTrie, &engine, false),
            crate::config::display::CursorStyle::SteadyBlock
        );
    }

    #[test]
    fn golden_prompt_owner_keeps_minibuffer_machinery() {
        let mut display = Display::new(48, 10, None, None);
        let (window, buffer) = window_and_buffer("hello\n");
        let rect = Rect::new(0, 0, 48, 10);
        let engine = crate::core::modal::ModalEngine::new();
        let mut prompt = PromptState::new("Find: ".to_string());
        prompt.input = "hel".to_string();
        // A minibuffer prompt outranks everything, in every input model.
        for model in [InputModel::KeyTrie, InputModel::Modal, InputModel::Standard] {
            let bar = compose_bar_state(model, &engine, Some(&prompt), "", "", None, None);
            assert_eq!(bar.owner, crate::terminal::display::BarOwner::Prompt);
            let (line, _) = render_bar(&mut display, &window, &buffer, &rect, &bar, Some(&prompt));
            assert!(line.contains("Find: hel"), "unexpected bar: {line:?}");
        }
        assert_eq!(
            bar_owner_for(InputModel::Modal, &engine, true, true),
            crate::terminal::display::BarOwner::Prompt
        );
    }

    #[test]
    fn cursor_style_per_model_and_state() {
        use crate::config::display::CursorStyle;
        let mut app = crate::core::app::EditorApp::new();
        let mut engine = crate::core::modal::ModalEngine::new();
        // Modal: Normal = block.
        assert_eq!(
            cursor_style_for(InputModel::Modal, &engine, false),
            CursorStyle::SteadyBlock
        );
        // Insert = bar…
        press(&mut engine, &mut app, kp('i'));
        assert_eq!(
            cursor_style_for(InputModel::Modal, &engine, false),
            CursorStyle::SteadyBar
        );
        // …Replace (overwrite on) = underline, with the matching segment.
        assert_eq!(
            cursor_style_for(InputModel::Modal, &engine, true),
            CursorStyle::SteadyUnderline
        );
        assert_eq!(
            mode_segment_for(InputModel::Modal, &engine, true),
            Some("-- REPLACE --")
        );
        use crate::core::input::{Key, KeyPress};
        engine.handle_key_inner(&mut app, &KeyPress::plain(Key::Esc));
        // Visual = block (hollow ok).
        press(&mut engine, &mut app, kp('v'));
        assert_eq!(
            cursor_style_for(InputModel::Modal, &engine, false),
            CursorStyle::HollowBlock
        );
        // Standard = bar, KeyTrie = block.
        assert_eq!(
            cursor_style_for(InputModel::Standard, &engine, false),
            CursorStyle::SteadyBar
        );
        assert_eq!(
            cursor_style_for(InputModel::KeyTrie, &engine, false),
            CursorStyle::SteadyBlock
        );
    }
}
