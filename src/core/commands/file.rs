//! File commands (open, save, revert).
use crate::core::app::EditorApp;
/// File and buffer operation commands
use crate::core::dispatcher::{DispatchResult, InputAction};

pub fn save_buffer(app: &mut EditorApp, _count: usize) -> DispatchResult {
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

pub fn find_file(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Find file: ".to_string(),
            action: InputAction::OpenFile,
        }
    }

pub fn write_file(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Write file: ".to_string(),
            action: InputAction::SaveAs,
        }
    }

pub fn read_file(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Read file: ".to_string(),
            action: InputAction::ReadFile,
        }
    }

pub fn delete_buffer(app: &mut EditorApp, _count: usize) -> DispatchResult {
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
                    for window in app.windows.values_mut() {
                        if window.buffer_id == buffer_id {
                            window.buffer_id = new_buffer_id;
                        }
                    }
                }
            }
        }
        DispatchResult::Success
    }

pub fn select_buffer(_app: &mut EditorApp, _count: usize) -> DispatchResult {
        DispatchResult::NeedsInput {
            prompt: "Switch to buffer: ".to_string(),
            action: InputAction::SwitchToBuffer,
        }
    }

pub fn print_buffer(app: &mut EditorApp, _count: usize) -> DispatchResult {
        use crate::core::print::{print_active_buffer, PrintersBackend};

        // Same cross-platform print path as the `print` command (N-193/N-267):
        // one backend, one typed error model, bytes straight from the rope.
        let backend = PrintersBackend::new();
        print_active_buffer(
            app,
            |buffer| match buffer.filename.as_ref().map(|p| p.display().to_string()) {
                Some(s) => s,
                None => "untitled".to_string(),
            },
            &backend,
        )
    }
