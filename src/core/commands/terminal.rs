//! Terminal-emulation commands.
use crate::core::app::EditorApp;
use crate::core::buffer::Buffer;
use crate::core::dispatcher::DispatchResult;
use crate::core::terminal_host::TerminalHost;
use std::path::PathBuf;

pub fn spawn_terminal(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let shell = crate::config::editor::SHELL;

        // Initial dimensions - will be updated by renderer/resize
        let cols = 80;
        let rows = 24;

        match TerminalHost::spawn(shell, cols, rows) {
            Ok(host) => {
                app.terminal_host = Some(host);

                // Create a *Terminal* buffer to display output
                let mut buffer = Buffer::new();
                buffer.filename = Some(PathBuf::from("*Terminal*"));
                buffer.buffer_kind = crate::core::buffer::BufferKind::Terminal;
                let buffer_id = app.add_buffer(buffer);

                // Switch current window to this buffer
                if let Some(window) = app.active_window_mut() {
                    window.buffer_id = buffer_id;
                    window.cursor_x = 0;
                    window.cursor_y = 0;
                }

                app.message = Some("Terminal spawned".to_string());
                DispatchResult::Success
            }
            Err(e) => {
                app.message = Some(format!("Failed to spawn terminal: {}", e));
                DispatchResult::Success
            }
        }
    }

/// Sends the active region's text to the built-in terminal, or the
/// current line when no region is active.
///
/// The text goes to the pty through the same [`TerminalHost::write`] path
/// focused terminal input takes, so the shell receives it as typed input.
/// The payload ends with exactly one newline, which executes the text in
/// the shell the way run-selection features do; multi-line regions pass
/// through unchanged and execute line by line.
///
/// When no terminal is open, or its child has exited, one is spawned
/// through [`spawn_terminal`] rather than failing: a send-to-terminal
/// command that errors when no terminal exists would be hostile to its
/// own workflow. Spawning switches the window to the `*Terminal*`
/// buffer, so the previous buffer and cursor are restored afterwards —
/// the command never moves the user out of the editor.
pub fn terminal_send_input(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let text = match text_to_send(app) {
            Some(text) => text,
            None => {
                app.message = Some("No active buffer".to_string());
                return DispatchResult::Success;
            }
        };
        if !ensure_terminal_host(app) {
            // spawn_terminal already reported the failure in app.message.
            return DispatchResult::Success;
        }
        let payload = format!("{}\n", text.trim_end_matches('\n'));
        if let Some(host) = app.terminal_host.as_mut() {
            host.write(payload.as_bytes());
            app.message = Some(format!("Sent {} bytes to terminal", payload.len()));
        }
        DispatchResult::Success
    }

/// The text [`terminal_send_input`] sends: the active region, or the
/// current line when no mark is set. `None` when there is no window or
/// buffer to read from.
fn text_to_send(app: &mut EditorApp) -> Option<String> {
        let window_id = app.active_window;
        let window = app.windows.get(&window_id)?;
        let buffer = app.buffers.get(&window.buffer_id)?;
        if let Some(mark_byte) = window.selection_manager.mark {
            let cursor_byte = window.get_byte_offset(buffer).unwrap_or(0);
            let (start, end) = if cursor_byte < mark_byte {
                (cursor_byte, mark_byte)
            } else {
                (mark_byte, cursor_byte)
            };
            Some(buffer.get_range_as_string(start, end - start))
        } else {
            buffer.line(window.cursor_y)
        }
    }

/// A live terminal host, spawning one through [`spawn_terminal`] when
/// none exists or the child has exited. The user's window state is
/// restored after the spawn, so the terminal stays one buffer switch
/// away instead of stealing the window. Returns false when no terminal
/// could be established.
fn ensure_terminal_host(app: &mut EditorApp) -> bool {
        if app
            .terminal_host
            .as_ref()
            .is_some_and(|host| host.is_alive())
        {
            return true;
        }
        app.terminal_host = None;
        let window_id = app.active_window;
        let saved = app
            .windows
            .get(&window_id)
            .map(|w| (w.buffer_id, w.cursor_x, w.cursor_y));
        spawn_terminal(app, 1);
        if let (Some((buffer_id, cursor_x, cursor_y)), Some(window)) =
            (saved, app.windows.get_mut(&window_id))
        {
            window.buffer_id = buffer_id;
            window.cursor_x = cursor_x;
            window.cursor_y = cursor_y;
        }
        app.terminal_host
            .as_ref()
            .is_some_and(|host| host.is_alive())
    }

pub fn split_spawn_terminal_vertical(app: &mut EditorApp, count: usize) -> DispatchResult {
        app.split_window_vertically();
        spawn_terminal(app, count)
    }

pub fn split_spawn_terminal_horizontal(app: &mut EditorApp, count: usize) -> DispatchResult {
        app.split_window_horizontally();
        spawn_terminal(app, count)
    }

#[cfg(test)]
mod tests {
    //! The terminal command tests drive a real pty child. Each test
    //! spawns its own shell, sends a unique probe line, and polls the
    //! host output with a bounded timeout; a missing probe fails with the
    //! accumulated output attached for diagnosis.
    use super::*;
    use crate::core::command::Command;
    use crate::core::dispatcher::dispatch;
    use std::time::{Duration, Instant};

    const PROBE: &str = "TERMINAL_SEND_INPUT_PROBE_9f3k";

    fn app_with_text(text: &str) -> EditorApp {
        let mut app = EditorApp::new();
        if let Some(buffer) = app.active_buffer_mut() {
            buffer.insert(0, text);
        }
        app
    }

    /// The buffer id the active window showed before the command ran.
    fn active_buffer_name(app: &EditorApp) -> Option<String> {
        let window = app.windows.get(&app.active_window)?;
        let buffer = app.buffers.get(&window.buffer_id)?;
        buffer
            .filename
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
    }

    /// Poll the terminal host until `needle` appears in its output.
    /// Returns everything the child wrote; fails loudly on timeout.
    fn wait_for_output(app: &mut EditorApp, needle: &str, timeout: Duration) -> String {
        let start = Instant::now();
        let mut accumulated = String::new();
        while start.elapsed() < timeout {
            if let Some(host) = app.terminal_host.as_mut() {
                if let Some(chunk) = host.read() {
                    accumulated.push_str(&chunk);
                }
                if accumulated.contains(needle) {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            accumulated.contains(needle),
            "shell never produced {needle:?}; output was: {accumulated:?}"
        );
        accumulated
    }

    #[test]
    fn terminal_send_input_runs_current_line_and_preserves_focus() {
        let mut app = app_with_text(&format!("echo {PROBE}\nsecond line\n"));
        let before = active_buffer_name(&app);
        let result = dispatch(&mut app, Command::TerminalSendInput, 1);
        assert_eq!(result, DispatchResult::Success);
        // A terminal was auto-spawned, and the editor never left the
        // user's buffer: the window still shows what it showed before.
        assert!(app.terminal_host.is_some());
        assert_eq!(active_buffer_name(&app), before);
        wait_for_output(&mut app, PROBE, Duration::from_secs(10));
    }

    #[test]
    fn terminal_send_input_sends_region_without_trailing_newline() {
        let mut app = app_with_text(&format!("echo {PROBE}\n"));
        // Mark at the start of the line, cursor at its end: the region is
        // the line text with no trailing newline, which the command must
        // still execute by appending one.
        let window_id = app.active_window;
        if let Some(window) = app.windows.get_mut(&window_id) {
            window.selection_manager.set_mark(0);
            let line_len = format!("echo {PROBE}").len();
            window.cursor_x = line_len;
        }
        let result = dispatch(&mut app, Command::TerminalSendInput, 1);
        assert_eq!(result, DispatchResult::Success);
        wait_for_output(&mut app, PROBE, Duration::from_secs(10));
    }

    #[test]
    fn terminal_send_input_runs_multi_line_region() {
        let marker_a = format!("{PROBE}_A");
        let marker_b = format!("{PROBE}_B");
        let mut app = app_with_text(&format!("echo {marker_a}\necho {marker_b}\n"));
        // Region spanning both lines: mark at (0,0), cursor at the end
        // of line 1.
        let window_id = app.active_window;
        if let Some(window) = app.windows.get_mut(&window_id) {
            window.selection_manager.set_mark(0);
            window.cursor_y = 1;
            window.cursor_x = format!("echo {marker_b}").chars().count();
        }
        let result = dispatch(&mut app, Command::TerminalSendInput, 1);
        assert_eq!(result, DispatchResult::Success);
        let output = wait_for_output(&mut app, &marker_b, Duration::from_secs(10));
        assert!(
            output.contains(&marker_a),
            "first line of the region never ran; output was: {output:?}"
        );
    }

    #[test]
    fn terminal_send_input_reuses_live_terminal() {
        let mut app = app_with_text("echo one\n");
        dispatch(&mut app, Command::TerminalSendInput, 1);
        let terminal_buffers: Vec<_> = app
            .buffers
            .iter()
            .filter(|(_, b)| {
                b.filename
                    .as_ref()
                    .is_some_and(|p| p.to_string_lossy() == "*Terminal*")
            })
            .collect();
        assert_eq!(terminal_buffers.len(), 1);
        if let Some(buffer) = app.active_buffer_mut() {
            buffer.insert(0, "echo two\n");
        }
        dispatch(&mut app, Command::TerminalSendInput, 1);
        let terminal_buffers: Vec<_> = app
            .buffers
            .iter()
            .filter(|(_, b)| {
                b.filename
                    .as_ref()
                    .is_some_and(|p| p.to_string_lossy() == "*Terminal*")
            })
            .collect();
        assert_eq!(
            terminal_buffers.len(),
            1,
            "a live terminal must be reused, not re-spawned"
        );
    }

    #[test]
    fn terminal_read_reports_non_utf8_output_as_activity() {
        // Binary child output used to make read() return None — no
        // chunk decoded as UTF-8 — even though the VTE parser advanced,
        // so the TUI/web loops never marked the frame dirty and the
        // output never drew.
        let mut app = app_with_text("echo ready\n");
        dispatch(&mut app, Command::TerminalSendInput, 1);
        wait_for_output(&mut app, "ready", Duration::from_secs(10));
        // Drain the channel so only the binary write below remains.
        if let Some(host) = app.terminal_host.as_mut() {
            while host.read().is_some() {}
            // Silence the line echo: the shell would otherwise echo
            // the printf command itself as valid UTF-8.
            host.write(b"stty -echo\n");
        }
        wait_for_output(&mut app, "stty -echo", Duration::from_secs(10));
        if let Some(host) = app.terminal_host.as_mut() {
            while host.read().is_some() {}
            // Two bytes that are not valid UTF-8, no newline echo.
            host.write(b"printf '\\377\\376'\n");
        }
        let start = Instant::now();
        let mut saw_activity = false;
        while start.elapsed() < Duration::from_secs(10) {
            if let Some(host) = app.terminal_host.as_mut()
                && host.read().is_some()
            {
                saw_activity = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            saw_activity,
            "binary child output must count as terminal activity"
        );
        // The shell is left with echo off; restore it for hygiene.
        if let Some(host) = app.terminal_host.as_mut() {
            host.write(b"stty echo\n");
        }
    }
}
