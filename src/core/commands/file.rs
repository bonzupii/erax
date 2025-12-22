use crate::core::app::EditorApp;
/// File and buffer operation commands
use crate::core::command::Command;
use crate::core::dispatcher::{DispatchResult, InputAction};

/// Save active buffer to file
#[derive(Clone)]
pub struct SaveBuffer;

impl Command for SaveBuffer {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        if let Some(window) = app.windows.get(&active_window_id) {
            let buffer_id = window.buffer_id;
            if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
                // If buffer has no filename, prompt for one (uEmacs behavior)
                if buffer.filename.is_none() {
                    return DispatchResult::NeedsInput {
                        prompt: "Save as: ".to_string(),
                        action: InputAction::SaveAs,
                    };
                }
                if buffer.check_external_modification() {
                    return DispatchResult::FileModified;
                }
                if let Err(e) = buffer.save() {
                    eprintln!("Error saving file: {}", e);
                }
            }
        }
        DispatchResult::Success
    }
}

/// Open a file (prompts for filename)
#[derive(Clone)]
pub struct FindFile;

impl Command for FindFile {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Find file: ".to_string(),
            action: InputAction::OpenFile,
        }
    }
}

/// Save buffer with new filename (prompts)
#[derive(Clone)]
pub struct WriteFile;

impl Command for WriteFile {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Write file: ".to_string(),
            action: InputAction::SaveAs,
        }
    }
}

/// Read file contents at cursor position (prompts)
#[derive(Clone)]
pub struct ReadFile;

impl Command for ReadFile {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Read file: ".to_string(),
            action: InputAction::ReadFile,
        }
    }
}

/// Kill active buffer
#[derive(Clone)]
pub struct KillBuffer;

impl Command for KillBuffer {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let active_window_id = app.active_window;
        if let Some(window) = app.windows.get(&active_window_id) {
            let buffer_id = window.buffer_id;
            if app.buffers.remove(&buffer_id).is_some() {
                // Also remove any windows viewing this buffer
                let windows_to_remove: Vec<_> = app
                    .windows
                    .iter()
                    .filter(|(_, w)| w.buffer_id == buffer_id)
                    .map(|(id, _)| *id)
                    .collect();

                for win_id in windows_to_remove {
                    app.windows.remove(&win_id);
                }

                // Switch to another buffer if available
                if let Some((&new_buffer_id, _)) = app.buffers.iter().next() {
                    for (_, window) in app.windows.iter_mut() {
                        if window.buffer_id == buffer_id {
                            window.buffer_id = new_buffer_id;
                        }
                    }
                }
            }
        }
        DispatchResult::Success
    }
}

/// Switch to different buffer (prompts)
#[derive(Clone)]
pub struct SwitchToBuffer;

impl Command for SwitchToBuffer {
    fn execute(&self, _app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Switch to buffer: ".to_string(),
            action: InputAction::SwitchToBuffer,
        }
    }
}

/// Print buffer to printer
#[derive(Clone)]
pub struct PrintBuffer;

impl Command for PrintBuffer {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        use crate::core::print::{CupsBackend, PrintBackend};

        let active_window_id = app.active_window;
        if let Some(window) = app.windows.get(&active_window_id) {
            let buffer_id = window.buffer_id;
            if let Some(buffer) = app.buffers.get(&buffer_id) {
                let content = buffer.to_string();
                let job_name = match buffer.filename.as_ref().map(|p| p.display().to_string()) {
                    Some(s) => s,
                    None => "untitled".to_string(),
                };

                let printer = CupsBackend::new();
                if let Err(e) = printer.print(&job_name, &content) {
                    eprintln!("Print error: {}", e);
                }
            }
        }
        DispatchResult::Success
    }
}
