//! Terminal-emulation commands.
use crate::core::app::EditorApp;
use crate::core::buffer::Buffer;
use crate::core::dispatcher::DispatchResult;
use crate::core::terminal_host::TerminalHost;
use std::path::PathBuf;

pub fn spawn_terminal(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let shell = "sh";

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

pub fn terminal_send_input(app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.message = Some("Forwarding input to terminal (placeholder)".to_string());
        DispatchResult::Success
    }

pub fn split_spawn_terminal_vertical(app: &mut EditorApp, count: usize) -> DispatchResult {
        app.split_window_vertically();
        spawn_terminal(app, count)
    }

pub fn split_spawn_terminal_horizontal(app: &mut EditorApp, count: usize) -> DispatchResult {
        app.split_window_horizontally();
        spawn_terminal(app, count)
    }
