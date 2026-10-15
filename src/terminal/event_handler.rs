//! Terminal event-handler module.
//!
//! This module processes terminal events (input, resize, mouse) and dispatches
//! them to the appropriate editor components.

use crate::core::app::EditorApp;
use crate::core::command::Command;
use crate::core::dispatcher::{DispatchResult, InputAction, dispatch, dispatch_insert};
use crate::core::layout;
use crate::core::mouse::{
    MouseButton as CoreMouseButton, MouseEvent as CoreMouseEvent, MouseHandler, ScrollDirection,
};
use crate::terminal::display::Display;
use crate::terminal::events::EditorEvent;
use crate::terminal::input_state::{InputMode, InputStateMachine};
use crate::terminal::keybinds::{KeyBindingManager, KeyOutcome};

/// Handle a dispatch result the same way for commands and inserted chars.
/// Returns Ok(true) when the editor should exit.
fn handle_dispatch_result(
    app: &mut EditorApp,
    display: &mut Display,
    result: DispatchResult,
) -> Result<bool, Box<dyn std::error::Error>> {
    match result {
        DispatchResult::Exit => Ok(true),
        DispatchResult::Success | DispatchResult::NotHandled => {
            display.dirty = true;
            Ok(false)
        }
        DispatchResult::Info(msg) => {
            // A command carrying its own message (list-buffers,
            // exit-and-save refusing on save errors, yank-pop): the
            // message is the result. Dropping it left bound keys that
            // ran fine but said nothing.
            app.message = Some(msg);
            display.dirty = true;
            Ok(false)
        }
        DispatchResult::Redraw => {
            display.dirty = true;
            Ok(false)
        }
        DispatchResult::NeedsInput { prompt, action } => {
            // Enter prompt mode via focus manager
            use crate::core::focus::{FocusState, focus_target_for_action};
            let target = focus_target_for_action(action);
            app.focus_manager.push(FocusState::new(target, &prompt));
            display.dirty = true;
            Ok(false)
        }
        DispatchResult::AwaitKey(action) => {
            use crate::core::focus::{FocusState, focus_target_for_await_key};
            let prompt = match &action {
                crate::core::dispatcher::InputAction::QuoteCharacter => "Quote next key: ",
                _ => "Describe Key: ",
            };
            let target = focus_target_for_await_key(action);
            app.focus_manager.push(FocusState::new(target, prompt));
            // One-keypress states draw no minibuffer; the message area
            // carries the prompt instead.
            app.message = Some(prompt.trim_end().to_string());
            display.dirty = true;
            Ok(false)
        }
        DispatchResult::FileModified(buffer_id) => {
            crate::core::focus::push_reload_question(app, buffer_id);
            display.dirty = true;
            Ok(false)
        }
    }
}

/// Dispatch a command from outside the event handler (e.g. an `await_longer`
/// leaf whose timeout expired). Returns Ok(true) when the editor should exit.
pub fn dispatch_command(
    app: &mut EditorApp,
    display: &mut Display,
    cmd: Command,
) -> Result<bool, Box<dyn std::error::Error>> {
    { let r = dispatch(app, cmd, 1); handle_dispatch_result(app, display, r) }
}

/// Processes a single editor event and returns true if exit is requested.
pub fn process_terminal_event(
    app: &mut EditorApp,
    display: &mut Display,
    keybind_manager: &mut KeyBindingManager,
    event: EditorEvent,
) -> Result<bool, Box<dyn std::error::Error>> {
    // Sync state machine with current app state
    let mut state_machine = InputStateMachine::new();
    let buffer_kind = app.active_buffer().map(|b| b.buffer_kind());
    state_machine.sync_from_app(
        &app.focus_manager,
        display.is_menu_open(),
        display.menu_bar.active_menu.unwrap_or_default(),
        buffer_kind,
    );

    match event {
        EditorEvent::Input(key) => {
            // Route based on current input mode
            match state_machine.mode() {
                InputMode::Focus { .. } => {
                    return handle_focus_input(app, display, keybind_manager, &key);
                }
                InputMode::Menu { .. } => {
                    return handle_menu_input(app, display, &key);
                }
                InputMode::SpecialBuffer { kind } => {
                    if handle_special_buffer_input(app, display, &key, *kind)? {
                        return Ok(false);
                    }
                    // Fall through to normal handling if not consumed
                }
                InputMode::Normal => {
                    // N-268: the vim modal engine owns key handling when the
                    // compile-time input model is Modal. Insert-mode special
                    // keys and ctrl/alt chords fall through to the trie.
                    if crate::config::keys::INPUT_MODEL == crate::config::keys::InputModel::Modal
                    {
                        match crate::terminal::modal::handle_modal_key(app, display, &key) {
                            crate::core::modal::ModalOutcome::Consumed => {
                                display.dirty = true;
                                return Ok(false);
                            }
                            crate::core::modal::ModalOutcome::ExitRequested => {
                                return Ok(true);
                            }
                            crate::core::modal::ModalOutcome::PassThrough => {}
                        }
                    }
                    // Continue to keybinding processing below
                }
            }

            // Normal mode: process through the keybinding manager.
            // The outcome is typed (N-055): a command, a character to insert,
            // a pending prefix, or a dead end. Dead ends are reported, never
            // silently swallowed (N-059).
            match keybind_manager.process_key(key) {
                KeyOutcome::Command(Command::ToggleMenu) => {
                    // N-043: the menu toggle is bound in the typed config;
                    // the key is not hardcoded here. It operates on display
                    // state, so it is handled before dispatch.
                    display.toggle_menu_focus();
                    display.dirty = true;
                    return Ok(false);
                }
                KeyOutcome::Command(cmd) => {
                    if { let r = dispatch(app, cmd, 1); handle_dispatch_result(app, display, r) }? {
                        return Ok(true);
                    }
                }
                KeyOutcome::InsertChar(c) => {
                    if { let r = dispatch_insert(app, c, 1); handle_dispatch_result(app, display, r) }? {
                        return Ok(true);
                    }
                }
                KeyOutcome::Prefix => {
                    display.set_key_sequence(keybind_manager.current_sequence());
                }
                KeyOutcome::AwaitingLonger { .. } => {
                    // Leaf that is also a prefix: show the sequence and wait.
                    // The event loop polls with the sequence timeout; if it
                    // expires, `take_awaited()` yields the leaf command.
                    display.set_key_sequence(keybind_manager.current_sequence());
                }
                KeyOutcome::DeadEnd { leaf } => {
                    // AwaitLonger leaf that got a non-matching key: run it.
                    if let Some(cmd) = leaf
                        && { let r = dispatch(app, cmd, 1); handle_dispatch_result(app, display, r) }? {
                            return Ok(true);
                        }
                    // Otherwise the key is genuinely unbound: say so instead
                    // of swallowing it (N-059).
                    display.set_message(format!("{} is undefined", key));
                    display.set_key_sequence(String::new());
                    display.dirty = true;
                    return Ok(false);
                }
            }
        }
        EditorEvent::Resize(cols, rows) => {
            display.update_size(cols, rows);

            // Account for menu bar height
            let menu_height = display.menu_bar_height();
            let editor_height = (rows as usize).saturating_sub(menu_height);
            let rect = layout::Rect::new(0, menu_height, cols as usize, editor_height);

            let window_rects = app.layout.collect_windows(rect);
            for (window_id, win_rect) in window_rects {
                if let Some(window) = app.windows.get_mut(&window_id) {
                    window.set_dimensions(win_rect.width, win_rect.height.saturating_sub(1));
                }
            }
            display.dirty = true;
            // Note: GUI mode doesn't use stdout, it renders via GridRenderer
            // TUI mode will call render separately in main loop
        }
        EditorEvent::Mouse(event) => {
            let mouse_x = event.column as usize;
            let mouse_y = event.row as usize;

            // Handle hover/movement for menu
            if let crate::core::input::MouseEventKind::Moved = event.kind {
                display.menu_handle_move(mouse_x, mouse_y);
                if display.dirty {
                    // Force render if menu state changed
                    // Since this is TUI loop, we can return false to continue loop but with dirty flag set.
                    // The loop checks display.dirty (?) - process_terminal_event returns Result<bool>.
                    // If false, caller loop checks dirty.
                }
                // Moved events usually don't need further processing for windows (yet)
                return Ok(false);
            }

            // Handle menu bar clicks first (only for click events, not drags/scrolls)
            if let crate::core::input::MouseEventKind::Down(_) = event.kind {
                if let Some(cmd) = display.menu_handle_click(mouse_x, mouse_y) {
                    if { let r = dispatch(app, cmd, 1); handle_dispatch_result(app, display, r) }? {
                        return Ok(true);
                    }
                    return Ok(false);
                }
                // If menu is open and click was on menu bar but no command, skip editor handling
                if mouse_y == 0 && display.show_menu_bar {
                    display.dirty = true;
                    return Ok(false);
                }
            }

            // Find window under mouse
            let (cols, rows) = display.terminal_size;
            // Account for menu bar height (same as resize handler)
            let menu_height = display.menu_bar_height();
            let editor_height = (rows as usize).saturating_sub(menu_height);
            let root_rect = layout::Rect::new(0, menu_height, cols as usize, editor_height);
            let windows = app.layout.collect_windows(root_rect);

            for (window_id, rect) in windows {
                if mouse_x >= rect.x
                    && mouse_x < rect.x + rect.width
                    && mouse_y >= rect.y
                    && mouse_y < rect.y + rect.height
                {
                    // Found the window
                    app.active_window = window_id;

                    if let Some(window) = app.windows.get_mut(&window_id)
                        && let Some(buffer) = app.buffers.get_mut(&window.buffer_id) {
                            // Calculate gutter width using input_router
                            let gutter_width = crate::core::input_router::gutter_width(
                                buffer.line_count(),
                                display.show_line_numbers,
                            );

                            // Check if click is on vertical scrollbar (rightmost column)
                            let scrollbar_x = rect.x + rect.width.saturating_sub(1);
                            let text_height = rect.height.saturating_sub(1); // Exclude status line

                            if mouse_x == scrollbar_x
                                && mouse_y >= rect.y
                                && mouse_y < rect.y + text_height
                            {
                                // Vertical scrollbar click with arrows
                                let rel_y = mouse_y.saturating_sub(rect.y);
                                let line_count = buffer.line_count().max(1);

                                if rel_y == 0 {
                                    // Up arrow - scroll up (N-046: typed config step).
                                    let target = window.scroll_offset.saturating_sub(
                                        crate::config::display::SCROLLBAR_ARROW_STEP_LINES,
                                    );
                                    window.scroll_to(target, buffer);
                                } else if rel_y >= text_height.saturating_sub(1) {
                                    // Down arrow - scroll down (N-046: typed config step).
                                    let target = window.scroll_offset.saturating_add(
                                        crate::config::display::SCROLLBAR_ARROW_STEP_LINES,
                                    );
                                    window.scroll_to(target, buffer);
                                } else {
                                    // Track area - jump to proportional position
                                    let track_height = text_height.saturating_sub(2); // Minus arrows
                                    let track_pos = rel_y.saturating_sub(1); // Minus up arrow
                                    let target_line = (track_pos * line_count)
                                        .checked_div(track_height)
                                        .unwrap_or(0)
                                        .min(line_count.saturating_sub(text_height));
                                    window.scroll_to(target_line, buffer);
                                }
                                display.dirty = true;
                                continue;
                            }

                            // Check if click is on horizontal scrollbar (second-to-last row)
                            let hscroll_y = rect.y + rect.height.saturating_sub(2);
                            let hscroll_start_x = rect.x + gutter_width;
                            let text_width = rect.width.saturating_sub(gutter_width + 1);
                            let hscroll_width = text_width.saturating_sub(1);

                            // Only handle if horizontal scroll is active
                            let needs_hscroll =
                                window.cached_content_width > text_width || window.scroll_x > 0;

                            if needs_hscroll
                                && mouse_y == hscroll_y
                                && mouse_x >= hscroll_start_x
                                && mouse_x < hscroll_start_x + hscroll_width
                            {
                                // Horizontal scrollbar click with arrows
                                let rel_x = mouse_x.saturating_sub(hscroll_start_x);
                                let content_width = window
                                    .cached_content_width
                                    .max(window.scroll_x + text_width);

                                if rel_x == 0 {
                                    // Left arrow - scroll left (N-046: typed config step).
                                    window.scroll_x = window.scroll_x.saturating_sub(
                                        crate::config::display::SCROLLBAR_ARROW_STEP_COLS,
                                    );
                                } else if rel_x >= hscroll_width.saturating_sub(1) {
                                    // Right arrow - scroll right (N-046: typed config step).
                                    let max_scroll = content_width.saturating_sub(text_width);
                                    window.scroll_x = (window.scroll_x
                                        + crate::config::display::SCROLLBAR_ARROW_STEP_COLS)
                                        .min(max_scroll);
                                } else {
                                    // Track area - jump to proportional position
                                    let track_width = hscroll_width.saturating_sub(2); // Minus arrows
                                    let track_pos = rel_x.saturating_sub(1); // Minus left arrow
                                    let max_scroll = content_width.saturating_sub(text_width);
                                    let target_x = (track_pos * content_width)
                                        .checked_div(track_width)
                                        .unwrap_or(0)
                                        .min(max_scroll);
                                    window.scroll_x = target_x;
                                }
                                display.dirty = true;
                                continue;
                            }

                            // Calculate relative coordinates within the text area
                            // Note: rect.y already accounts for menu bar (root_rect starts at menu_height)
                            let rel_x = mouse_x.saturating_sub(rect.x).saturating_sub(gutter_width);
                            let rel_y = mouse_y.saturating_sub(rect.y);

                            // Ignore clicks on status line
                            if rel_y >= rect.height.saturating_sub(1) {
                                display.dirty = true;
                                continue;
                            }

                            // Convert input mouse button to core mouse button
                            let convert_btn = |b: crate::core::input::MouseButton| match b {
                                crate::core::input::MouseButton::Left => CoreMouseButton::Left,
                                crate::core::input::MouseButton::Right => CoreMouseButton::Right,
                                crate::core::input::MouseButton::Middle => CoreMouseButton::Middle,
                            };

                            // Convert event to core mouse event
                            let core_event = match event.kind {
                                crate::core::input::MouseEventKind::Down(btn) => {
                                    match event.click_count {
                                        2 => Some(CoreMouseEvent::DoubleClick(rel_x, rel_y)),
                                        3 => Some(CoreMouseEvent::TripleClick(rel_x, rel_y)),
                                        _ => Some(CoreMouseEvent::Click(
                                            rel_x,
                                            rel_y,
                                            convert_btn(btn),
                                        )),
                                    }
                                }
                                crate::core::input::MouseEventKind::Drag(btn) => {
                                    // Start coordinates are not tracked here but MouseHandler ignores them
                                    Some(CoreMouseEvent::Drag(0, 0, rel_x, rel_y, convert_btn(btn)))
                                }
                                crate::core::input::MouseEventKind::ScrollUp => {
                                    Some(CoreMouseEvent::Scroll(3, ScrollDirection::Up))
                                }
                                crate::core::input::MouseEventKind::ScrollDown => {
                                    Some(CoreMouseEvent::Scroll(3, ScrollDirection::Down))
                                }
                                crate::core::input::MouseEventKind::ScrollLeft => {
                                    Some(CoreMouseEvent::Scroll(3, ScrollDirection::Left))
                                }
                                crate::core::input::MouseEventKind::ScrollRight => {
                                    Some(CoreMouseEvent::Scroll(3, ScrollDirection::Right))
                                }
                                _ => None,
                            };

                            if let Some(evt) = core_event {
                                // Handle middle-click paste (X11 style)
                                // We detect and handle it here, dispatching yank after positioning
                                let is_middle_click = matches!(
                                    &evt,
                                    CoreMouseEvent::Click(_, _, CoreMouseButton::Middle)
                                );

                                if is_middle_click
                                    && let CoreMouseEvent::Click(x, y, _) = &evt {
                                        // Position cursor at click location
                                        let mouse_handler = MouseHandler::new();
                                        let pos_event =
                                            CoreMouseEvent::Click(*x, *y, CoreMouseButton::Left);
                                        mouse_handler.handle_event(&pos_event, window, buffer);
                                    }
                                // Exit borrow scope before dispatching
                                if is_middle_click {
                                    crate::core::dispatcher::dispatch(app, Command::Yank, 1);
                                    display.dirty = true;
                                    continue;
                                }

                                let mouse_handler = MouseHandler::new();
                                if mouse_handler.handle_event(&evt, window, buffer) {
                                    display.dirty = true;

                                    // Sync window.mark from selection_manager for rendering compatibility
                                    if let Some(sel) = window.selection_manager.get_selection() {
                                        // The mark is the anchor of the selection
                                        let anchor_byte = sel.anchor;
                                        let line = buffer.byte_to_line(anchor_byte);
                                        if let Some(line_start) = buffer.line_to_byte(line) {
                                            let col_byte = anchor_byte.saturating_sub(line_start);
                                            if let Some(line_text) = buffer.line(line) {
                                                let col = crate::core::utf8::byte_to_grapheme_col(
                                                    &line_text, col_byte,
                                                );
                                                window.mark = Some((col, line));
                                            } else {
                                                window.mark = Some((0, line));
                                            }
                                        }
                                    } else {
                                        window.mark = None;
                                    }
                                }
                            }
                        }
                    break;
                }
            }
        }
        EditorEvent::None => {}
    }
    Ok(false)
}

/// Handle input when focus state is active (minibuffer, prompts, etc.)
fn handle_focus_input(
    app: &mut EditorApp,
    display: &mut Display,
    keybind_manager: &mut KeyBindingManager,
    key: &crate::core::input::KeyPress,
) -> Result<bool, Box<dyn std::error::Error>> {
    let focus_result = if let Some(focus) = app.focus_manager.current_state_mut() {
        if focus.target == crate::core::focus::FocusTarget::QuoteCharacter {
            // Quoted insert: the next keypress bypasses all keybindings and
            // lands in the buffer literally. Control chords become their
            // control codes; Enter and Tab become newline and tab.
            use crate::core::input::Key;
            if key.key == Key::Esc {
                // Cancel the quoted insert without touching the buffer.
                app.focus_manager.pop();
                display.dirty = true;
                return Ok(false);
            }
            let literal = match key.key {
                Key::Char(c) if key.ctrl => {
                    let lower = c.to_ascii_lowercase() as u8;
                    if lower.is_ascii_lowercase() {
                        Some((lower - b'a' + 1) as char)
                    } else {
                        None
                    }
                }
                Key::Char(c) => Some(c),
                Key::Enter => Some('\n'),
                Key::Tab => Some('\t'),
                _ => None,
            };
            app.focus_manager.pop();
            match literal {
                Some(c) => {
                    let r = crate::core::dispatcher::dispatch_insert(app, c, 1);
                    if handle_dispatch_result(app, display, r)? {
                        return Ok(true);
                    }
                }
                None => {
                    app.message = Some("Cannot quote that key".to_string());
                }
            }
            display.dirty = true;
            return Ok(false);
        } else if focus.target == crate::core::focus::FocusTarget::DescribeKey {
            // Process key using keybind manager to see if it's bound.
            // This properly handles prefixes and multi-key sequences.
            match keybind_manager.process_key(*key) {
                KeyOutcome::Command(cmd) => {
                    let seq = keybind_manager.current_sequence();
                    app.focus_manager.pop();
                    app.message = Some(format!("{} runs command: {}", seq, cmd.name()));
                }
                KeyOutcome::InsertChar(c) => {
                    let seq = keybind_manager.current_sequence();
                    app.focus_manager.pop();
                    app.message = Some(format!("{} inserts '{}'", seq, c));
                }
                KeyOutcome::DeadEnd { .. } => {
                    let seq = keybind_manager.current_sequence();
                    app.focus_manager.pop();
                    app.message = Some(format!("{} is undefined", seq));
                }
                KeyOutcome::Prefix | KeyOutcome::AwaitingLonger { .. } => {
                    // Incomplete sequence (prefix): stay in DescribeKey mode.
                    focus.prompt =
                        format!("Describe Key: {}", keybind_manager.current_sequence());
                }
            }
            // Consumed needed input
            display.dirty = true;
            return Ok(false);
        } else if focus.target.uses_minibuffer() {
            Some((focus.target, focus.handle_key(key)))
        } else {
            None
        }
    } else {
        None
    };

    if let Some((target, result)) = focus_result {
        match result {
            crate::core::focus::FocusResult::Continue => {
                display.dirty = true;
                return Ok(false);
            }
            crate::core::focus::FocusResult::Confirmed(input) => {
                if target == crate::core::focus::FocusTarget::ConfirmReload {
                    // The payload is set by the only constructor for this
                    // target, so the question cannot exist without its
                    // buffer; the key mapping ("y" reloads, anything else
                    // declines) is the single rule in
                    // `FocusState::confirm_reload_key`.
                    let buffer_id = app
                        .focus_manager
                        .current_state()
                        .and_then(|s| s.reload_buffer_id())
                        .expect("ConfirmReload focus always carries its buffer");
                    app.focus_manager.pop();
                    crate::core::commands::file::answer_reload_question(
                        app,
                        buffer_id,
                        input == "y",
                    );
                    display.dirty = true;
                    return Ok(false);
                }
                let action = match target {
                    crate::core::focus::FocusTarget::Calculator => InputAction::Calculator,
                    crate::core::focus::FocusTarget::Minibuffer => InputAction::ExecuteNamedCommand,
                    crate::core::focus::FocusTarget::GoToLine => InputAction::GotoLine,
                    crate::core::focus::FocusTarget::ISearch => InputAction::SearchForward,
                    crate::core::focus::FocusTarget::FindReplace => InputAction::QueryReplace,
                    _ => InputAction::ExecuteNamedCommand,
                };

                app.focus_manager.pop();

                // The prompt handler reports exit through Ok(true)
                // (M-x exit-erax, palette quit, vim `:` ex commands that
                // dispatch Exit). Checking only Err discarded it, so those
                // commands ran, requested exit, and the editor stayed open.
                match crate::core::prompt::handle_prompt_action(app, action, input) {
                    Ok(should_exit) => {
                        display.dirty = true;
                        return Ok(should_exit);
                    }
                    Err(e) => {
                        app.message = Some(format!("Error: {}", e));
                    }
                }

                display.dirty = true;
                return Ok(false);
            }
            crate::core::focus::FocusResult::Cancelled => {
                app.focus_manager.pop();
                display.dirty = true;
                return Ok(false);
            }
            _ => {
                display.dirty = true;
                return Ok(false);
            }
        }
    }
    Ok(false)
}

/// Handle input when menu bar is active
fn handle_menu_input(
    app: &mut EditorApp,
    display: &mut Display,
    key: &crate::core::input::KeyPress,
) -> Result<bool, Box<dyn std::error::Error>> {
    use crate::core::input::Key;
    match &key.key {
        Key::Esc => {
            display.close_menu();
        }
        Key::Up => {
            display.menu_handle_up();
        }
        Key::Down => {
            display.menu_handle_down();
        }
        Key::Left => {
            display.menu_handle_left();
        }
        Key::Right => {
            display.menu_handle_right();
        }
        Key::Enter => {
            if let Some(cmd) = display.menu_handle_enter()
                && { let r = dispatch(app, cmd, 1); handle_dispatch_result(app, display, r) }? {
                    return Ok(true);
                }
        }
        _ => {
            // Close menu on any other key and let normal handling proceed
            display.close_menu();
        }
    }
    Ok(false)
}

/// Handle input for special buffer types (Diagnostics, Diff, Terminal).
///
/// Diagnostics and diff views read their single-key maps from the config
/// tables: which view is active is the engine's decision, but the key to
/// command pairs are data. Keys are matched exactly, so a modified key in
/// a special view falls through to the model tables. Terminal buffers
/// forward keys to the pty instead; that forwarding policy is engine
/// behavior, not a mapping, and stays here.
fn handle_special_buffer_input(
    app: &mut EditorApp,
    display: &mut Display,
    key: &crate::core::input::KeyPress,
    kind: crate::core::buffer::BufferKind,
) -> Result<bool, Box<dyn std::error::Error>> {
    use crate::config::keys::{DIAGNOSTICS_KEYS, DIFF_KEYS};
    use crate::core::buffer::BufferKind;
    use crate::core::input::Key;

    let table: &[(crate::core::input::KeyPress, Command)] = match kind {
        BufferKind::Diagnostics => DIAGNOSTICS_KEYS,
        BufferKind::DiffOriginal | BufferKind::DiffModified => DIFF_KEYS,
        _ => &[],
    };
    if let Some((_, command)) = table.iter().find(|(k, _)| k == key) {
        dispatch(app, *command, 1);
        display.dirty = true;
        return Ok(true);
    }
    if kind == BufferKind::Terminal {
            // Forward keys to terminal (except Ctrl-X which is the prefix).
            let is_ctrl_x = key.ctrl && key.key == Key::Char('x');
            if !is_ctrl_x
                && let Some(ref mut host) = app.terminal_host {
                    match key.key {
                        Key::Char(c) if key.ctrl => {
                            let lower = c.to_ascii_lowercase() as u8;
                            if lower.is_ascii_lowercase() {
                                host.write(&[lower - b'a' + 1]);
                            }
                        }
                        Key::Char(c) => {
                            let mut buf = [0u8; 4];
                            let encoded = c.encode_utf8(&mut buf);
                            host.write(encoded.as_bytes());
                        }
                        Key::Enter => {
                            host.write(b"\n");
                        }
                        Key::Backspace => {
                            host.write(&[0x7f]);
                        }
                        Key::Tab => {
                            host.write(b"\t");
                        }
                        _ => {}
                    }
                    display.dirty = true;
                    return Ok(true);
                }
    }
    Ok(false)
}
