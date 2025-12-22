use crate::core::app::EditorApp;
use crate::core::command::Command;
use crate::core::dispatcher::DispatchResult;
use std::io::Write;
use std::process::{Command as ShellCommand, Stdio};

/// Print buffer content using lpr
#[derive(Clone)]
pub struct PrintCommand;

impl Command for PrintCommand {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        // Check if lpr exists
        let lpr_exists = ShellCommand::new("which")
            .arg("lpr")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !lpr_exists {
            app.message = Some("No printer driver available".to_string());
            return DispatchResult::Success;
        }

        let active_window_id = app.active_window;
        if let Some(window) = app.windows.get(&active_window_id) {
            let buffer_id = window.buffer_id;
            if let Some(buffer) = app.buffers.get(&buffer_id) {
                let content = buffer.to_string();

                let child = ShellCommand::new("lpr").stdin(Stdio::piped()).spawn();

                match child {
                    Ok(mut child) => {
                        if let Some(mut stdin) = child.stdin.take() {
                            if let Err(e) = stdin.write_all(content.as_bytes()) {
                                app.message = Some(format!("Print failed: {}", e));
                                return DispatchResult::Success;
                            }
                        }
                        match child.wait() {
                            Ok(status) if status.success() => {
                                app.message = Some("Buffer sent to printer".to_string());
                            }
                            Ok(status) => {
                                app.message = Some(format!("Print failed with status: {}", status));
                            }
                            Err(e) => {
                                app.message = Some(format!("Print failed: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        app.message = Some(format!("Failed to start lpr: {}", e));
                    }
                }
            }
        }

        DispatchResult::Success
    }
}
