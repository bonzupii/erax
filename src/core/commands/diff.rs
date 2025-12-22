use crate::core::app::EditorApp;
use crate::core::command::Command;
use crate::core::dispatcher::DispatchResult;
use crate::sed::SedConfig;
use crate::sed::diff::{DiffState, DiffView};
use std::io::Cursor;

#[derive(Clone)]
pub struct SedPreviewCommand;

impl Command for SedPreviewCommand {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        if app.active_buffer().is_none() {
            return DispatchResult::NotHandled;
        }

        // Prompt for sed expression
        DispatchResult::NeedsInput {
            prompt: "Sed preview expression: ".to_string(),
            action: crate::core::dispatcher::InputAction::SedPreview,
        }
    }
}

pub fn start_sed_diff(
    app: &mut EditorApp,
    expression: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let (original_content, original_buffer_id) = {
        let buffer = app.active_buffer().ok_or("No active buffer")?;
        let window = app
            .windows
            .get(&app.active_window)
            .ok_or("No active window")?;
        (buffer.to_string(), window.buffer_id)
    };
    let original_window_id = app.active_window;

    let mut config = SedConfig::new();
    config.add_script(expression).map_err(|e| e.to_string())?;

    let mut modified_output = Vec::new();
    config.execute(Cursor::new(&original_content), &mut modified_output)?;
    let modified_content = String::from_utf8(modified_output)?;

    let diff_view = DiffView::new(original_content.clone(), modified_content.clone());
    let hunks = diff_view.compute_hunks();

    if hunks.is_empty() {
        app.message = Some("No changes".to_string());
        return Ok(());
    }

    // Create buffers
    let mut original_diff_buffer = crate::core::buffer::Buffer::from_string(original_content);
    original_diff_buffer.filename = Some(std::path::PathBuf::from("*Diff Original*"));
    original_diff_buffer.buffer_kind = crate::core::buffer::BufferKind::DiffOriginal;
    let obid = app.add_buffer(original_diff_buffer);

    let mut modified_diff_buffer = crate::core::buffer::Buffer::from_string(modified_content);
    modified_diff_buffer.filename = Some(std::path::PathBuf::from("*Diff Modified*"));
    modified_diff_buffer.buffer_kind = crate::core::buffer::BufferKind::DiffModified;
    let mbid = app.add_buffer(modified_diff_buffer);

    // Split window vertically
    app.split_window_vertically();
    let right_window_id = app.active_window;

    // Set buffers
    if let Some(window) = app.windows.get_mut(&right_window_id) {
        window.buffer_id = mbid;
    }

    // Switch to other window to set its buffer
    app.next_window();
    let left_window_id = app.active_window;
    if let Some(window) = app.windows.get_mut(&left_window_id) {
        window.buffer_id = obid;
    }

    app.diff_state = Some(DiffState {
        hunks,
        current_hunk: 0,
        original_buffer_id,
        original_window_id,
    });

    // Jump to first hunk
    jump_to_hunk(app, 0);

    Ok(())
}

fn jump_to_hunk(app: &mut EditorApp, index: usize) {
    let state = match &app.diff_state {
        Some(s) => s,
        None => return,
    };
    if index >= state.hunks.len() {
        return;
    }

    let hunk = &state.hunks[index];
    let line = hunk.start_line;

    // Update both windows
    let window_ids: Vec<crate::core::id::WindowId> = app.windows.keys().cloned().collect();
    for wid in window_ids {
        if let Some(window) = app.windows.get_mut(&wid) {
            if let Some(buffer) = app.buffers.get(&window.buffer_id) {
                use crate::core::buffer::BufferKind;
                let kind = buffer.buffer_kind();
                if kind == BufferKind::DiffOriginal || kind == BufferKind::DiffModified {
                    window.cursor_y = line;
                    window.cursor_x = 0;
                    window.scroll_offset = line.saturating_sub(5);
                    window.update_visual_cursor(buffer);
                    window.ensure_cursor_visible(buffer);
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct DiffNextHunk;

impl Command for DiffNextHunk {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let mut state = match app.diff_state.take() {
            Some(s) => s,
            None => return DispatchResult::NotHandled,
        };

        if state.current_hunk + 1 < state.hunks.len() {
            state.current_hunk += 1;
        }

        let current_idx = state.current_hunk;
        app.diff_state = Some(state);
        jump_to_hunk(app, current_idx);
        DispatchResult::Success
    }
}

#[derive(Clone)]
pub struct DiffPrevHunk;

impl Command for DiffPrevHunk {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let mut state = match app.diff_state.take() {
            Some(s) => s,
            None => return DispatchResult::NotHandled,
        };

        if state.current_hunk > 0 {
            state.current_hunk -= 1;
        }

        let current_idx = state.current_hunk;
        app.diff_state = Some(state);
        jump_to_hunk(app, current_idx);
        DispatchResult::Success
    }
}

#[derive(Clone)]
pub struct DiffAcceptHunk;

impl Command for DiffAcceptHunk {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        let mut state = match app.diff_state.take() {
            Some(s) => s,
            None => return DispatchResult::NotHandled,
        };

        let hunk = state.hunks[state.current_hunk].clone();
        let original_buffer_id = state.original_buffer_id;

        if let Some(buffer) = app.buffers.get_mut(&original_buffer_id) {
            // Apply hunk to original buffer
            if let Some(start_byte) = buffer.line_to_byte(hunk.start_line) {
                let end_line_plus_one = hunk.end_line + 1;
                let end_byte = buffer
                    .line_to_byte(end_line_plus_one)
                    .unwrap_or_else(|| buffer.len());

                buffer.delete(start_byte, end_byte - start_byte);

                let new_text = hunk.new_lines.join("");
                buffer.insert(start_byte, &new_text);
            }
        }

        if state.current_hunk + 1 < state.hunks.len() {
            state.current_hunk += 1;
        } else {
            app.message = Some("Applied last hunk".to_string());
        }

        let current_idx = state.current_hunk;
        app.diff_state = Some(state);
        jump_to_hunk(app, current_idx);

        DispatchResult::Success
    }
}

#[derive(Clone)]
pub struct DiffQuit;

impl Command for DiffQuit {
    fn execute(&self, app: &mut EditorApp, _count: usize) -> DispatchResult {
        if let Some(state) = app.diff_state.take() {
            let original_buffer_id = state.original_buffer_id;
            app.delete_other_windows();
            if let Some(window) = app.windows.get_mut(&app.active_window) {
                window.buffer_id = original_buffer_id;
            }
            return DispatchResult::Success;
        }
        DispatchResult::NotHandled
    }
}
