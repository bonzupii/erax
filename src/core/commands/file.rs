//! File commands (open, save, reload).
use crate::core::app::EditorApp;
/// File and buffer operation commands
use crate::core::dispatcher::{DispatchResult, InputAction};
use crate::core::id::BufferId;

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
                    return DispatchResult::FileModified(buffer_id);
                }
                if let Err(e) = buffer.save() {
                    eprintln!("Error saving file: {}", e);
                }
            }
        }
        DispatchResult::Success
    }

/// Answer a pending reload-confirm question for this buffer.
///
/// `y`: reload the buffer from disk through the normal open path
/// (encoding detection, pager/mmap backend selection — identical
/// semantics to a fresh open), then complete the save that raised the
/// question, which is a no-op write once the buffer matches disk.
/// Declining (`n`, Enter, Esc, C-g): record the divergence so this exact
/// on-disk state is never asked about again, and refuse the save —
/// overwriting someone else's changes takes a second, deliberate save
/// after the warning. If the buffer holds unsaved edits, the question
/// text names that consequence, so `y` never destroys work the prompt
/// did not warn about.
pub fn answer_reload_question(app: &mut EditorApp, buffer_id: BufferId, reload: bool) {
    let path = match app
        .buffers
        .get(&buffer_id)
        .and_then(|b| b.filename.clone())
    {
        Some(path) => path,
        None => {
            app.message = Some("Buffer is no longer open; nothing reloaded.".to_string());
            return;
        }
    };
    if !reload {
        if let Some(buffer) = app.buffers.get_mut(&buffer_id) {
            buffer.acknowledged_mtime = buffer.current_disk_mtime();
        }
        app.message = Some(format!(
            "Not saved: {} changed on disk (save again to overwrite it, or reload to pick up the changes)",
            path.display()
        ));
        return;
    }
    let loaded = match crate::core::encoding::load_file_with_encoding(&path) {
        Ok(loaded) => loaded,
        Err(e) => {
            app.message = Some(format!("Reload failed: {e}"));
            return;
        }
    };
    let encoding_note = if loaded.had_errors
        || loaded.detected.encoding != crate::core::encoding::Encoding::Utf8
    {
        Some(format!(
            "{}: reloaded as {}{}",
            path.display(),
            loaded.detected.encoding.label(),
            if loaded.had_errors {
                "; invalid sequences shown as U+FFFD, original bytes kept for byte-exact save"
            } else {
                ""
            }
        ))
    } else {
        None
    };
    let save_error = match app.buffers.get_mut(&buffer_id) {
        Some(buffer) => {
            buffer.adopt_loaded(loaded.buffer);
            // Complete the save that raised the question: the buffer now
            // matches disk, so this resyncs the mtime bookkeeping.
            buffer.save().err().map(|e| e.to_string())
        }
        None => {
            app.message = Some("Buffer is no longer open; nothing reloaded.".to_string());
            return;
        }
    };
    app.message = match (encoding_note, save_error) {
        (_, Some(e)) => Some(format!("Reloaded {}, but save failed: {e}", path.display())),
        (Some(note), None) => Some(note),
        (None, None) => Some(format!("Reloaded {}", path.display())),
    };
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
