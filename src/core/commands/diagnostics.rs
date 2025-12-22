//! Diagnostics Pane Commands
//!
//! This module implements the diagnostics pane (^X d), which shows all
//! errors and warnings across all open buffers.

use crate::core::app::EditorApp;
use crate::core::buffer::Buffer;
use crate::core::command::Command;
use crate::core::diagnostics::DiagnosticSeverity;
use crate::core::dispatcher::DispatchResult;
use crate::core::id::BufferId;
use std::path::PathBuf;

const DIAGNOSTICS_BUFFER_NAME: &str = "*Diagnostics*";

/// Toggle the diagnostics pane
#[derive(Clone, Debug)]
pub struct ToggleDiagnosticsPane;

impl Command for ToggleDiagnosticsPane {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        use crate::core::buffer::BufferKind;
        // 1. Check if diagnostics window exists
        let diag_window_id = {
            let mut found = None;
            for (win_id, window) in &app.windows {
                if let Some(buffer) = app.buffers.get(&window.buffer_id) {
                    if buffer.buffer_kind() == BufferKind::Diagnostics {
                        found = Some(*win_id);
                        break;
                    }
                }
            }
            found
        };

        if let Some(win_id) = diag_window_id {
            // If it exists, close it
            // If it's the only window, we can't close it, but usually it's a split
            if app.windows.len() > 1 {
                let current_active = app.active_window;
                app.active_window = win_id;
                app.delete_window();
                // If we closed the diagnostics window, try to return to previous window
                if app.windows.contains_key(&current_active) {
                    app.active_window = current_active;
                }
            } else {
                // Just switch to a scratch buffer if it's the only window?
                // Or just do nothing. uEmacs typically doesn't close the last window.
            }
            return DispatchResult::Success;
        }

        // 2. Window doesn't exist, create/find the buffer
        let buffer_id = get_or_create_diagnostics_buffer(app);

        // 3. Populate the buffer with all diagnostics
        populate_diagnostics(app, buffer_id);

        // 4. Split window and show diagnostics
        // We prefer splitting vertically (top/bottom) for diagnostics
        app.split_window_vertically(); // This splits and focuses the new window
        if let Some(window) = app.windows.get_mut(&app.active_window) {
            window.buffer_id = buffer_id;
            window.cursor_x = 0;
            window.cursor_y = 0;
        }

        DispatchResult::Success
    }
}

/// Jump to the diagnostic at cursor
#[derive(Clone, Debug)]
pub struct DiagnosticsJump;

impl Command for DiagnosticsJump {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        use crate::core::buffer::BufferKind;
        let (file, line) = {
            let window = match app.active_window_ref() {
                Some(w) => w,
                None => return DispatchResult::Success,
            };
            let buffer = match app.buffers.get(&window.buffer_id) {
                Some(b) => b,
                None => return DispatchResult::Success,
            };

            if buffer.buffer_kind() != BufferKind::Diagnostics {
                // If not in diagnostics buffer, do default Enter behavior: insert newline
                // But we can't easily call another command from here without registry
                // So we'll let the dispatcher handle it by not matching if we use a smart dispatcher.
                // For now, let's just do nothing if not in diagnostics.
                return DispatchResult::NotHandled;
            }

            // Parse current line: '! main.rs:6:12   Error: cannot find value'
            let line_text = match buffer.line(window.cursor_y) {
                Some(l) => l,
                None => return DispatchResult::Success,
            };

            parse_diagnostic_line(&line_text)
        };

        if let Some((path, line_num)) = file.zip(line) {
            // Close diagnostics pane
            let diag_window_id = {
                let mut found = None;
                for (win_id, window) in &app.windows {
                    if let Some(buffer) = app.buffers.get(&window.buffer_id) {
                        if buffer.buffer_kind() == BufferKind::Diagnostics {
                            found = Some(*win_id);
                            break;
                        }
                    }
                }
                found
            };

            if let Some(win_id) = diag_window_id {
                app.active_window = win_id;
                app.delete_window();
            }

            // Try to find if it's already open
            let mut found_buffer_id = None;
            for (id, buf) in &app.buffers {
                if let Some(fname) = &buf.filename {
                    // Try to match canonical paths if possible, but fallback to ends_with
                    if fname == &path || fname.ends_with(&path) || path.ends_with(fname) {
                        found_buffer_id = Some(*id);
                        break;
                    }
                }
            }

            let buffer_id = match found_buffer_id {
                Some(id) => id,
                None => match app.load_file(&path) {
                    Ok(id) => id,
                    Err(_) => return DispatchResult::Success,
                },
            };

            // Switch to buffer and jump to line
            if let Some(window) = app.windows.get_mut(&app.active_window) {
                window.buffer_id = buffer_id;
                window.cursor_y = line_num.saturating_sub(1);
                window.cursor_x = 0;
                if let Some(buffer) = app.buffers.get(&buffer_id) {
                    window.update_visual_cursor(buffer);
                    window.ensure_cursor_visible(buffer);
                }
            }
        }

        DispatchResult::Success
    }
}

/// Helper: Get or create diagnostics buffer
fn get_or_create_diagnostics_buffer(app: &mut EditorApp) -> BufferId {
    use crate::core::buffer::BufferKind;
    for (id, buffer) in &app.buffers {
        if buffer.buffer_kind() == BufferKind::Diagnostics {
            return *id;
        }
    }

    let mut buffer = Buffer::new();
    buffer.filename = Some(PathBuf::from(DIAGNOSTICS_BUFFER_NAME));
    buffer.buffer_kind = BufferKind::Diagnostics;
    app.add_buffer(buffer)
}

/// Helper: Populate diagnostics buffer with all diagnostics from all buffers
fn populate_diagnostics(app: &mut EditorApp, diag_buffer_id: BufferId) {
    let mut all_diagnostics = Vec::new();

    for buffer in app.buffers.values() {
        if buffer.display_name() == DIAGNOSTICS_BUFFER_NAME {
            continue;
        }

        let filename = buffer.display_name();
        for diag in &buffer.diagnostics {
            all_diagnostics.push((filename.clone(), diag.clone()));
        }
    }

    // Sort by severity (Error first), then file, then line
    all_diagnostics.sort_by(|a, b| {
        let sev_a = match a.1.severity {
            DiagnosticSeverity::Error => 0,
            DiagnosticSeverity::Warning => 1,
            DiagnosticSeverity::Note => 2,
            DiagnosticSeverity::Info => 3,
        };
        let sev_b = match b.1.severity {
            DiagnosticSeverity::Error => 0,
            DiagnosticSeverity::Warning => 1,
            DiagnosticSeverity::Note => 2,
            DiagnosticSeverity::Info => 3,
        };
        sev_a
            .cmp(&sev_b)
            .then_with(|| a.0.cmp(&b.0))
            .then_with(|| a.1.line.cmp(&b.1.line))
    });

    let mut content = String::new();
    if all_diagnostics.is_empty() {
        content.push_str("No diagnostics found.\n");
    } else {
        for (filename, diag) in all_diagnostics {
            let sev_icon = match diag.severity {
                DiagnosticSeverity::Error => "!",
                DiagnosticSeverity::Warning => "W",
                DiagnosticSeverity::Note => "N",
                DiagnosticSeverity::Info => "I",
            };

            let loc = format!(
                "{}:{}:{}",
                filename,
                diag.line,
                match diag.column {
                    Some(c) => c,
                    None => 1,
                }
            );
            content.push_str(&format!(
                "{} {:<16}   {}: {}\n",
                sev_icon,
                loc,
                match diag.severity {
                    DiagnosticSeverity::Error => "Error",
                    DiagnosticSeverity::Warning => "Warning",
                    DiagnosticSeverity::Note => "Note",
                    DiagnosticSeverity::Info => "Info",
                },
                diag.message
            ));
        }
    }

    if let Some(diag_buffer) = app.buffers.get_mut(&diag_buffer_id) {
        let len = diag_buffer.len();
        diag_buffer.delete(0, len);
        diag_buffer.insert(0, &content);
        diag_buffer.modified = false; // It's a special buffer
    }
}

/// Helper: Parse a line from the diagnostics buffer back into a file and line number
fn parse_diagnostic_line(line: &str) -> (Option<PathBuf>, Option<usize>) {
    // Format: '! main.rs:6:12   Error: cannot find value'
    if line.len() < 3 {
        return (None, None);
    }

    let rest = &line[2..]; // skip icon and space
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.is_empty() {
        return (None, None);
    }

    let loc_part = parts[0];
    let loc_parts: Vec<&str> = loc_part.split(':').collect();
    if loc_parts.len() >= 2 {
        let path = PathBuf::from(loc_parts[0]);
        let line_num = loc_parts[1].parse::<usize>().ok();
        return (Some(path), line_num);
    }

    (None, None)
}
