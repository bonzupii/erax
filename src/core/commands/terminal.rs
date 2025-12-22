use crate::core::app::EditorApp;
use crate::core::buffer::Buffer;
use crate::core::command::Command;
use crate::core::dispatcher::DispatchResult;
use crate::core::terminal_host::TerminalHost;
use std::path::PathBuf;

/// Spawn a new terminal in the current window
#[derive(Clone)]
pub struct SpawnTerminal;

impl Command for SpawnTerminal {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
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
}

/// Forward input to terminal (placeholder)
#[derive(Clone)]
pub struct TerminalSendInput;

impl Command for TerminalSendInput {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        app.message = Some("Forwarding input to terminal (placeholder)".to_string());
        DispatchResult::Success
    }
}

/// Split window vertically and spawn terminal
#[derive(Clone)]
pub struct SplitSpawnTerminalVertical;

impl Command for SplitSpawnTerminalVertical {
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult {
        app.split_window_vertically();
        SpawnTerminal.execute(app, count)
    }
}

/// Split window horizontally and spawn terminal
#[derive(Clone)]
pub struct SplitSpawnTerminalHorizontal;

impl Command for SplitSpawnTerminalHorizontal {
    fn execute(&self, app: &mut EditorApp, count: usize) -> DispatchResult {
        app.split_window_horizontally();
        SpawnTerminal.execute(app, count)
    }
}
