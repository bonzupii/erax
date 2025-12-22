//! Shared prompt action handling for all editor modes (TUI/GUI)

use crate::core::app::EditorApp;
use crate::core::buffer::Buffer;
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
                match Buffer::from_file(&path) {
                    Ok(buffer) => {
                        let buffer_id = app.alloc_buffer_id();
                        app.buffers.insert(buffer_id, buffer);
                        if let Some(window) = app.windows.get_mut(&app.active_window) {
                            window.buffer_id = buffer_id;
                            window.cursor_x = 0;
                            window.cursor_y = 0;
                            window.scroll_offset = 0;
                        }
                        app.message = Some(format!("Opened {}", input));
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
                if let Some(bid) = bid {
                    if let Some(buffer) = app.buffers.get_mut(&bid) {
                        buffer.filename = Some(path);
                        if let Err(e) = buffer.save() {
                            app.message = Some(format!("Error: {}", e));
                        } else {
                            app.message = Some(format!("Wrote {}", input));
                        }
                    }
                }
            }
        }
        InputAction::SearchForward => {
            if !input.is_empty() {
                if let Some(window) = app.windows.get_mut(&app.active_window) {
                    let bid = window.buffer_id;
                    if let Some(buffer) = app.buffers.get(&bid) {
                        let start_pos = match window.get_byte_offset(buffer) {
                            Some(p) => p + 1,
                            None => 1,
                        };
                        if let Some(pos) = buffer.find_forward(&input, start_pos) {
                            let line = buffer.byte_to_line(pos);
                            let line_start = match buffer.line_to_byte(line) {
                                Some(b) => b,
                                None => 0,
                            };
                            let col = crate::core::utf8::grapheme_count(
                                &buffer.to_string()[line_start..pos],
                            );
                            window.cursor_y = line;
                            window.cursor_x = col;
                            window.update_visual_cursor(buffer);
                            window.ensure_cursor_visible(buffer);
                        }
                        app.message = Some(format!("Search: {}", input));
                    }
                }
            }
        }
        InputAction::SearchBackward => {
            if !input.is_empty() {
                if let Some(window) = app.windows.get_mut(&app.active_window) {
                    let bid = window.buffer_id;
                    if let Some(buffer) = app.buffers.get(&bid) {
                        let cursor_pos = match window.get_byte_offset(buffer) {
                            Some(p) => p,
                            None => 0,
                        };
                        if let Some(found_pos) = buffer.find_backward(&input, cursor_pos) {
                            let line = buffer.byte_to_line(found_pos);
                            let line_start = match buffer.line_to_byte(line) {
                                Some(b) => b,
                                None => 0,
                            };
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
        }
        InputAction::GotoLine => {
            if let Ok(line) = input.parse::<usize>() {
                if line > 0 {
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
            }
        }
        InputAction::QueryReplace => {
            if let Some((old, new)) = input.split_once('|') {
                if !old.is_empty() {
                    let bid = app.windows.get(&app.active_window).map(|w| w.buffer_id);
                    if let Some(bid) = bid {
                        if let Some(buffer) = app.buffers.get_mut(&bid) {
                            let count = buffer.replace_all(old, new);
                            buffer.modified = true;
                            app.message = Some(format!("Replaced {} occurrences", count));
                        }
                    }
                }
            } else {
                app.message = Some("Format: old|new (use | as separator)".to_string());
            }
        }
        InputAction::SwitchToBuffer => {
            let mut found = false;
            let mut target_id = None;
            let mut target_name = String::new();

            for (&buffer_id, buffer) in app.buffers.iter() {
                if let Some(filename) = &buffer.filename {
                    let name = filename
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string());
                    let name = match name {
                        Some(n) => n,
                        None => String::new(),
                    };
                    if name == input || filename.to_string_lossy().contains(&input) {
                        target_id = Some(buffer_id);
                        target_name = name;
                        found = true;
                        break;
                    }
                }
            }
            if found {
                if let Some(window) = app.windows.get_mut(&app.active_window) {
                    if let Some(bid) = target_id {
                        window.buffer_id = bid;
                        window.cursor_x = 0;
                        window.cursor_y = 0;
                        window.scroll_offset = 0;
                        app.message = Some(format!("Switched to {}", target_name));
                    }
                }
            } else {
                app.message = Some(format!("Buffer not found: {}", input));
            }
        }
        InputAction::RenameSymbol => {
            app.message = Some("Rename functionality (LSP) removed.".to_string());
        }
        InputAction::ReadFile => {
            if !input.is_empty() {
                let path = std::path::PathBuf::from(&input);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Some(window) = app.windows.get_mut(&app.active_window) {
                        let bid = window.buffer_id;
                        if let Some(buffer) = app.buffers.get_mut(&bid) {
                            if let Some(pos) = window.get_byte_offset(buffer) {
                                buffer.insert(pos, &content);
                                buffer.modified = true;
                            }
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
                let output = std::process::Command::new("sh")
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
                    }
                }
            }
        }
        InputAction::FilterBuffer => {
            app.message = Some(format!("Filter: {}", input));
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
                    }
                }
            }
        }
        InputAction::SedPreview => {
            if !input.is_empty() {
                if let Err(e) = crate::core::commands::diff::start_sed_diff(app, &input) {
                    app.message = Some(format!("Sed Error: {}", e));
                }
            }
        }
        InputAction::ExecuteNamedCommand => {
            if !input.is_empty() {
                let result = dispatch(app, Some(&input), None, 1);
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
                    DispatchResult::Exit => {
                        return Ok(true);
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(false)
}
