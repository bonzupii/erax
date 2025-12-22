//! Terminal event handler module
//!
//! This module processes terminal events (input, resize, mouse) and dispatches
//! them to the appropriate editor components.

use crate::core::app::EditorApp;
use crate::core::dispatcher::{DispatchResult, InputAction, dispatch};
use crate::core::input::Key;
use crate::core::layout;
use crate::core::mouse::{
    MouseButton as CoreMouseButton, MouseEvent as CoreMouseEvent, MouseHandler, ScrollDirection,
};
use crate::terminal::display::Display;
use crate::terminal::events::EditorEvent;
use crate::terminal::input_state::{InputMode, InputStateMachine};
use crate::terminal::keybinds::KeyBindingManager;

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
        match display.menu_bar.active_menu {
            Some(m) => m,
            None => 0,
        },
        buffer_kind,
    );

    match event {
        EditorEvent::Input(key) => {
            // Route based on current input mode
            match state_machine.mode() {
                InputMode::Focus { .. } => {
                    return handle_focus_input(app, display, &key);
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
                    // Continue to keybinding processing below
                }
            }

            // F10 toggles menu from normal mode
            if matches!(&key.key, Key::F(10)) {
                display.toggle_menu_focus();
                return Ok(false);
            }
            // Normal mode: process through keybinding manager
            // Process key through keybinding manager
            let (command_name_opt, char_to_insert_opt, is_complete) =
                keybind_manager.process_key(&key);

            // Display partial key sequences in status bar
            let current_seq = keybind_manager.current_sequence();
            if !current_seq.is_empty() {
                display.set_key_sequence(current_seq);
            } else {
                display.set_key_sequence(String::new());
            }

            // If we have a complete command or char to insert, dispatch it
            if is_complete {
                let result = dispatch(app, command_name_opt.as_deref(), char_to_insert_opt, 1);

                // Handle different dispatch results
                match result {
                    DispatchResult::Exit => return Ok(true),
                    DispatchResult::Success => {
                        display.dirty = true;
                    }
                    DispatchResult::NotHandled => {
                        display.dirty = true;
                    }
                    DispatchResult::NeedsInput { prompt, action } => {
                        // Enter prompt mode via focus manager
                        use crate::core::focus::{FocusState, FocusTarget};
                        let target = match action {
                            InputAction::Calculator => FocusTarget::Calculator,
                            InputAction::GotoLine => FocusTarget::GoToLine,
                            InputAction::SearchForward => FocusTarget::ISearch,
                            InputAction::QueryReplace => FocusTarget::FindReplace,
                            _ => FocusTarget::Minibuffer,
                        };
                        app.focus_manager.push(FocusState::new(target, &prompt));
                        display.dirty = true;
                    }
                    DispatchResult::FileModified => {
                        display.set_message("File changed on disk. Reload? (y/n)".into());
                        display.dirty = true;
                    }
                    _ => {}
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
                    let result = dispatch(app, Some(cmd), None, 1);
                    if let DispatchResult::Exit = result {
                        return Ok(true);
                    }
                    display.dirty = true;
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
            let root_rect = layout::Rect::new(0, 0, cols as usize, rows as usize);
            let windows = app.layout.collect_windows(root_rect);

            for (window_id, rect) in windows {
                if mouse_x >= rect.x
                    && mouse_x < rect.x + rect.width
                    && mouse_y >= rect.y
                    && mouse_y < rect.y + rect.height
                {
                    // Found the window
                    app.active_window = window_id;

                    if let Some(window) = app.windows.get_mut(&window_id) {
                        if let Some(buffer) = app.buffers.get_mut(&window.buffer_id) {
                            // Calculate gutter width using input_router
                            let gutter_width = crate::core::input_router::gutter_width(
                                buffer.line_count(),
                                display.show_line_numbers,
                            );

                            // Account for menu bar row if shown
                            let menu_bar_offset = if display.show_menu_bar { 1 } else { 0 };

                            // Calculate relative coordinates within the text area
                            let rel_x = mouse_x.saturating_sub(rect.x).saturating_sub(gutter_width);
                            let rel_y = mouse_y
                                .saturating_sub(rect.y)
                                .saturating_sub(menu_bar_offset);

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

                                if is_middle_click {
                                    if let CoreMouseEvent::Click(x, y, _) = &evt {
                                        // Position cursor at click location
                                        let mouse_handler = MouseHandler::new();
                                        let pos_event =
                                            CoreMouseEvent::Click(*x, *y, CoreMouseButton::Left);
                                        mouse_handler.handle_event(&pos_event, window, buffer);
                                    }
                                }
                                // Exit borrow scope before dispatching
                                if is_middle_click {
                                    crate::core::dispatcher::dispatch(app, Some("yank"), None, 1);
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
    key: &crate::core::input::InputEvent,
) -> Result<bool, Box<dyn std::error::Error>> {
    let focus_result = if let Some(focus) = app.focus_manager.current_state_mut() {
        if focus.target.uses_minibuffer() {
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
                let action = match target {
                    crate::core::focus::FocusTarget::Calculator => InputAction::Calculator,
                    crate::core::focus::FocusTarget::Minibuffer => InputAction::ExecuteNamedCommand,
                    crate::core::focus::FocusTarget::GoToLine => InputAction::GotoLine,
                    crate::core::focus::FocusTarget::ISearch => InputAction::SearchForward,
                    crate::core::focus::FocusTarget::FindReplace => InputAction::QueryReplace,
                    _ => InputAction::ExecuteNamedCommand,
                };

                app.focus_manager.pop();

                if let Err(e) = crate::core::prompt::handle_prompt_action(app, action, input) {
                    app.message = Some(format!("Error: {}", e));
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
    key: &crate::core::input::InputEvent,
) -> Result<bool, Box<dyn std::error::Error>> {
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
            if let Some(cmd) = display.menu_handle_enter() {
                let result = dispatch(app, Some(cmd), None, 1);
                if let DispatchResult::Exit = result {
                    return Ok(true);
                }
                display.dirty = true;
            }
        }
        _ => {
            // Close menu on any other key and let normal handling proceed
            display.close_menu();
        }
    }
    Ok(false)
}

/// Handle input for special buffer types (Diagnostics, Diff, Terminal)
fn handle_special_buffer_input(
    app: &mut EditorApp,
    display: &mut Display,
    key: &crate::core::input::InputEvent,
    kind: crate::core::buffer::BufferKind,
) -> Result<bool, Box<dyn std::error::Error>> {
    use crate::core::buffer::BufferKind;

    match kind {
        BufferKind::Diagnostics => match key.key {
            Key::Char('j') | Key::Down => {
                dispatch(app, Some("next-line"), None, 1);
                display.dirty = true;
                return Ok(true);
            }
            Key::Char('k') | Key::Up => {
                dispatch(app, Some("previous-line"), None, 1);
                display.dirty = true;
                return Ok(true);
            }
            Key::Enter => {
                dispatch(app, Some("diagnostics-jump"), None, 1);
                display.dirty = true;
                return Ok(true);
            }
            Key::Char('q') | Key::Esc => {
                dispatch(app, Some("delete-window"), None, 1);
                display.dirty = true;
                return Ok(true);
            }
            _ => {}
        },
        BufferKind::DiffOriginal | BufferKind::DiffModified => match key.key {
            Key::Char('j') => {
                dispatch(app, Some("diff-next-hunk"), None, 1);
                display.dirty = true;
                return Ok(true);
            }
            Key::Char('k') => {
                dispatch(app, Some("diff-previous-hunk"), None, 1);
                display.dirty = true;
                return Ok(true);
            }
            Key::Char('y') => {
                dispatch(app, Some("diff-accept-hunk"), None, 1);
                display.dirty = true;
                return Ok(true);
            }
            Key::Char('n') => {
                dispatch(app, Some("diff-next-hunk"), None, 1);
                display.dirty = true;
                return Ok(true);
            }
            Key::Char('q') => {
                dispatch(app, Some("diff-quit"), None, 1);
                display.dirty = true;
                return Ok(true);
            }
            _ => {}
        },
        BufferKind::Terminal => {
            // Forward keys to terminal (except Ctrl-X which is prefix)
            if key.key != Key::Ctrl('x') {
                if let Some(ref mut host) = app.terminal_host {
                    match key.key {
                        Key::Char(c) => {
                            host.write(&[c as u8]);
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
                        Key::Ctrl(c) => {
                            let code = (c as u8).wrapping_sub(b'a').wrapping_add(1);
                            host.write(&[code]);
                        }
                        _ => {}
                    }
                    display.dirty = true;
                    return Ok(true);
                }
            }
        }
        _ => {}
    }
    Ok(false)
}
