//! Shared prompt-action handling for all editor modes.

use crate::core::app::EditorApp;
use crate::core::buffer::Buffer;
use crate::core::command::Command;
use crate::core::dispatcher::{DispatchResult, InputAction, dispatch};

/// Handle a completed prompt action at the core level
/// Returns true if exit is requested
pub fn handle_prompt_action(
    app: &mut EditorApp,
    action: InputAction,
    input: String,
) -> Result<bool, Box<dyn std::error::Error>> {
    match action {
        InputAction::OpenFile => {
            if !input.is_empty() {
                let path = std::path::PathBuf::from(&input);
                // Byte-safe open, shared with startup and diagnostics: the
                // loader reports encoding replacements via `app.message`
                // itself, so only set the plain confirmation when it stayed
                // quiet.
                match app.load_file(&path) {
                    Ok(buffer_id) => {
                        if let Some(window) = app.windows.get_mut(&app.active_window) {
                            window.buffer_id = buffer_id;
                            window.cursor_x = 0;
                            window.cursor_y = 0;
                            window.scroll_offset = 0;
                        }
                        if app.message.is_none() {
                            app.message = Some(format!("Opened {}", input));
                        }
                    }
                    Err(e) => {
                        app.message = Some(format!("Error: {}", e));
                    }
                }
            }
        }
        InputAction::SaveAs => {
            if !input.is_empty() {
                let path = std::path::PathBuf::from(&input);
                let bid = app.windows.get(&app.active_window).map(|w| w.buffer_id);
                if let Some(bid) = bid
                    && let Some(buffer) = app.buffers.get_mut(&bid) {
                        buffer.filename = Some(path);
                        if let Err(e) = buffer.save() {
                            app.message = Some(format!("Error: {}", e));
                        } else {
                            app.message = Some(format!("Wrote {}", input));
                        }
                    }
            }
        }
        InputAction::SearchForward => {
            if !input.is_empty()
                && let Some(window) = app.windows.get_mut(&app.active_window) {
                    let bid = window.buffer_id;
                    if let Some(buffer) = app.buffers.get(&bid) {
                        let start_pos = match window.get_byte_offset(buffer) {
                            Some(p) => p + 1,
                            None => 1,
                        };
                        if let Some(pos) = buffer.find_forward(&input, start_pos) {
                            let line = buffer.byte_to_line(pos);
                            let line_start = buffer.line_to_byte(line).unwrap_or_default();
                            // Boundary-safe: the lossy range string is
                            // never sliced by raw buffer offsets (a raw
                            // offset can split the replacement chars).
                            let slice =
                                buffer.get_range_as_string(line_start, pos - line_start);
                            let col = crate::core::utf8::grapheme_count(&slice);
                            window.cursor_y = line;
                            window.cursor_x = col;
                            window.update_visual_cursor(buffer);
                            window.ensure_cursor_visible(buffer);
                        }
                        app.message = Some(format!("Search: {}", input));
                    }
                }
        }
        InputAction::SearchBackward => {
            if !input.is_empty()
                && let Some(window) = app.windows.get_mut(&app.active_window) {
                    let bid = window.buffer_id;
                    if let Some(buffer) = app.buffers.get(&bid) {
                        let cursor_pos = window.get_byte_offset(buffer).unwrap_or_default();
                        if let Some(found_pos) = buffer.find_backward(&input, cursor_pos) {
                            let line = buffer.byte_to_line(found_pos);
                            let line_start = buffer.line_to_byte(line).unwrap_or_default();
                            let slice =
                                buffer.get_range_as_string(line_start, found_pos - line_start);
                            let col = crate::core::utf8::grapheme_count(&slice);
                            window.cursor_y = line;
                            window.cursor_x = col;
                            window.update_visual_cursor(buffer);
                            window.ensure_cursor_visible(buffer);
                        }
                        app.message = Some(format!("Search backward: {}", input));
                    }
                }
        }
        InputAction::GotoLine => {
            match input.parse::<usize>() {
                Ok(line) if line > 0 => {
                    if let Some(window) = app.windows.get_mut(&app.active_window) {
                        let bid = window.buffer_id;
                        if let Some(buffer) = app.buffers.get(&bid) {
                            let target_line = (line - 1).min(buffer.line_count().saturating_sub(1));
                            window.cursor_y = target_line;
                            window.cursor_x = 0;
                            window.update_visual_cursor(buffer);
                            window.ensure_cursor_visible(buffer);
                        }
                    }
                }
                _ => {
                    // A rejected number is user input, not a programmer
                    // error: say so in the message line rather than
                    // silently ignoring the prompt.
                    app.message = Some(format!("Invalid line number: {}", input));
                }
            }
        }
        InputAction::GotoByte => match input.parse::<usize>() {
            Ok(byte_offset) => {
                app.goto_byte(byte_offset);
            }
            _ => {
                app.message = Some(format!("Invalid byte offset: {}", input));
            }
        },
        InputAction::QueryReplace => {
            if let Some((old, new)) = input.split_once('|') {
                if old.is_empty() {
                    app.message = Some("Empty search pattern: nothing replaced".to_string());
                } else if let Some(bid) =
                    app.windows.get(&app.active_window).map(|w| w.buffer_id)
                    && let Some(buffer) = app.buffers.get_mut(&bid) {
                        let count = buffer.replace_all(old, new);
                        buffer.modified = true;
                        app.message = Some(format!("Replaced {} occurrences", count));
                    }
            } else {
                app.message = Some("Format: old|new (use | as separator)".to_string());
            }
        }
        InputAction::SwitchToBuffer => {
            // Match against display names so unnamed buffers (*scratch*)
            // are reachable. Collect all matches and pick the
            // alphabetically first for deterministic multi-match
            // behavior (HashMap iteration order is not stable).
            let mut matches: Vec<(String, crate::core::id::BufferId)> = app
                .buffers
                .iter()
                .map(|(&id, b)| (b.display_name(), id))
                .filter(|(name, _)| name == &input || name.contains(&input))
                .collect();
            matches.sort_by(|a, b| a.0.cmp(&b.0));
            match matches.into_iter().next() {
                Some((target_name, bid)) => {
                    if let Some(window) = app.windows.get_mut(&app.active_window) {
                        window.buffer_id = bid;
                        window.cursor_x = 0;
                        window.cursor_y = 0;
                        window.scroll_offset = 0;
                        window.scroll_x = 0;
                        window.selection_manager.clear_mark();
                        app.message = Some(format!("Switched to {}", target_name));
                    }
                }
                None => {
                    app.message = Some(format!("Buffer not found: {}", input));
                }
            }
        }
        InputAction::RenameSymbol => {
            app.message = Some("Rename functionality not implemented.".to_string());
        }
        InputAction::ReadFile => {
            if !input.is_empty() {
                let path = std::path::PathBuf::from(&input);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Some(window) = app.windows.get_mut(&app.active_window) {
                        let bid = window.buffer_id;
                        if let Some(buffer) = app.buffers.get_mut(&bid)
                            && let Some(pos) = window.get_byte_offset(buffer) {
                                buffer.insert(pos, &content);
                                buffer.modified = true;
                            }
                    }
                    app.message = Some(format!("Read {}", input));
                } else {
                    app.message = Some(format!("Cannot read {}", input));
                }
            }
        }
        InputAction::ShellCommand => {
            if !input.is_empty() {
                // Execute command
                let output = std::process::Command::new(crate::config::editor::SHELL)
                    .arg("-c")
                    .arg(&input)
                    .output();

                match output {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let full_output = format!("{}{}", stdout, stderr);

                        // Find or create output buffer
                        let mut buffer_id = None;
                        for (&id, buf) in app.buffers.iter() {
                            if buf.buffer_kind() == crate::core::buffer::BufferKind::ShellOutput {
                                buffer_id = Some(id);
                                break;
                            }
                        }

                        let bid = if let Some(id) = buffer_id {
                            id
                        } else {
                            let mut new_buf = Buffer::new();
                            new_buf.filename =
                                Some(std::path::PathBuf::from("*Shell Command Output*"));
                            new_buf.buffer_kind = crate::core::buffer::BufferKind::ShellOutput;
                            app.add_buffer(new_buf)
                        };

                        if let Some(buffer) = app.buffers.get_mut(&bid) {
                            buffer.clear_diagnostics();
                            // Clear buffer content
                            let len = buffer.len();
                            buffer.delete(0, len);
                            buffer.insert(0, &full_output);

                            // Parse diagnostics
                            let mut parser = crate::core::diagnostics::DiagnosticParser::new();
                            for line in full_output.lines() {
                                parser.parse_line(line);
                            }
                            let diags = parser.finish();
                            for diag in diags {
                                buffer.add_diagnostic(diag);
                            }

                            app.message = Some(format!(
                                "Command finished. {} diagnostics found.",
                                buffer.diagnostics.len()
                            ));
                        }

                        // Switch to this buffer in active window
                        if let Some(window) = app.windows.get_mut(&app.active_window) {
                            window.buffer_id = bid;
                            window.cursor_x = 0;
                            window.cursor_y = 0;
                        }
                    }
                    Err(e) => {
                        app.message = Some(format!("Error: {}", e));
                        app.cue_bus.emit(crate::core::cue::Cue::Error);
                    }
                }
            }
        }
        InputAction::FilterBuffer => {
            if !input.is_empty() {
                // Pipe the buffer through the shell command: stdin in,
                // stdout replaces the buffer. A failed filter leaves the
                // buffer untouched.
                let original = match app.active_buffer() {
                    Some(buffer) => buffer.content_string(),
                    None => {
                        app.message = Some("No active buffer".to_string());
                        return Ok(false);
                    }
                };
                let child = std::process::Command::new(crate::config::editor::SHELL)
                    .arg("-c")
                    .arg(&input)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn();
                match child {
                    Ok(mut child) => {
                        use std::io::Write;
                        let stdin = child.stdin.take();
                        // Stdin goes on its own thread: with a buffer
                        // larger than the pipe, a sequential
                        // write-then-wait deadlocks (the child blocks on
                        // a full stdout pipe while we block on a full
                        // stdin pipe).
                        let writer = std::thread::spawn(move || {
                            if let Some(mut stdin) = stdin {
                                stdin.write_all(original.as_bytes()).is_ok()
                            } else {
                                false
                            }
                        });
                        match child.wait_with_output() {
                            Ok(output)
                                if writer.join().unwrap_or(false)
                                    && output.status.success() => {
                                let filtered =
                                    String::from_utf8_lossy(&output.stdout).into_owned();
                                if let Some(buffer) = app.active_buffer_mut() {
                                    let len = buffer.len();
                                    buffer.delete(0, len);
                                    buffer.insert(0, &filtered);
                                    buffer.modified = true;
                                    app.message =
                                        Some(format!("Filtered through: {input}"));
                                }
                            }
                            Ok(output) => {
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                app.message = Some(format!(
                                    "Filter failed (exit {}): {}",
                                    output.status,
                                    stderr.trim()
                                ));
                                app.cue_bus.emit(crate::core::cue::Cue::Error);
                            }
                            Err(e) => {
                                app.message = Some(format!("Error: {e}"));
                                app.cue_bus.emit(crate::core::cue::Cue::Error);
                            }
                        }
                    }
                    Err(e) => {
                        app.message = Some(format!("Error: {e}"));
                        app.cue_bus.emit(crate::core::cue::Cue::Error);
                    }
                }
            }
        }
        InputAction::Calculator => {
            if !input.is_empty() {
                match app.calculator.eval(&input) {
                    Ok(result) => {
                        let formatted = crate::core::calculator::Calculator::format_result(result);
                        app.message = Some(format!("Result: {}", formatted));
                    }
                    Err(e) => {
                        app.message = Some(format!("Calc Error: {}", e));
                        app.cue_bus.emit(crate::core::cue::Cue::Error);
                    }
                }
            }
        }
        InputAction::SedPreview => {
            if !input.is_empty()
                && let Err(e) = crate::core::commands::diff::start_sed_diff(app, &input) {
                    app.message = Some(format!("Sed Error: {}", e));
                    app.cue_bus.emit(crate::core::cue::Cue::Error);
                }
        }
        InputAction::ExecuteNamedCommand => {
            if !input.is_empty() {
                let result = match Command::from_name(&input) {
                    Some(cmd) => dispatch(app, cmd, 1),
                    None => DispatchResult::NotHandled,
                };
                match result {
                    DispatchResult::Success => {}
                    DispatchResult::NotHandled => {
                        app.message = Some(format!("[No match: {}]", input));
                    }
                    DispatchResult::Info(msg) => {
                        app.message = Some(msg);
                    }
                    DispatchResult::NeedsInput { prompt, action: _ } => {
                        app.focus_manager.push(crate::core::focus::FocusState::new(
                            crate::core::focus::FocusTarget::Minibuffer,
                            &prompt,
                        ));
                    }
                    DispatchResult::AwaitKey(action) => {
                        let target = crate::core::focus::focus_target_for_await_key(action);
                        app.focus_manager.push(crate::core::focus::FocusState::new(
                            target,
                            "Describe Key: ",
                        ));
                    }
                    DispatchResult::FileModified(buffer_id) => {
                        crate::core::focus::push_reload_question(app, buffer_id);
                    }
                    DispatchResult::Exit => {
                        return Ok(true);
                    }
                    // A full redraw is subsumed: the prompt's caller
                    // repaints unconditionally on dismissal.
                    DispatchResult::Redraw => {}
                }
            }
        }
        InputAction::DescribeKey => {
            // Not used via prompt
        }
        InputAction::QuoteCharacter => {
            // Not used via prompt; the event loop consumes the keypress
        }
    }
    // Prompt actions mutate buffers outside dispatch() (query-replace,
    // filter-buffer, switch-buffer): the reconciliation seam lives here
    // so every caller — TUI, modal ex commands, web — upholds the same
    // postcondition: the active window is valid for its buffer.
    app.reconcile_active_window();
    Ok(false)
}
