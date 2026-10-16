//! Diff-navigation commands.
use crate::core::app::EditorApp;
use crate::core::dispatcher::DispatchResult;
use crate::sed::SedConfig;
use crate::sed::diff::DiffView;
use std::io::Cursor;

/// Navigation state for the interactive diff view.
///
/// Stores owned hunk spans (not borrowed `Hunk`s) so the state outlives the
/// `DiffView` used to compute them.
pub struct DiffNavState {
    /// (old_start, old_end, new_start, new_end, new_lines), 1-based spans with
    /// end exclusive. new_lines is the replacement text for accept-hunk:
    /// the hunk's added lines interleaved with its context lines, in order
    /// (removed lines excluded), so it covers the whole old span.
    pub hunks: Vec<(usize, usize, usize, usize, Vec<String>)>,
    pub current_hunk: usize,
    /// Hunks already applied to the original buffer. Spans are in
    /// pre-diff coordinates; an accepted hunk shifts every hunk below it
    /// by (new_lines.len() - old span length).
    pub applied: Vec<bool>,
    pub original_buffer_id: crate::core::id::BufferId,
    pub original_window_id: crate::core::id::WindowId,
    /// The two scratch buffers backing the diff view; removed on quit.
    pub original_diff_buffer_id: crate::core::id::BufferId,
    pub modified_diff_buffer_id: crate::core::id::BufferId,
}

pub fn sed_preview(app: &mut EditorApp, _count: usize) -> DispatchResult {
        if app.active_buffer().is_none() {
            return DispatchResult::NotHandled;
        }

        // Prompt for sed expression
        DispatchResult::NeedsInput {
            prompt: "Sed preview expression: ".to_string(),
            action: crate::core::dispatcher::InputAction::SedPreview,
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
        (buffer.content_string(), window.buffer_id)
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

    // Extract owned spans for navigation state. The replacement text is
    // every hunk line except the removed ones: added lines interleaved
    // with their context, covering the whole old span.
    let spans: Vec<(usize, usize, usize, usize, Vec<String>)> = hunks
        .iter()
        .map(|h| {
            let new_lines: Vec<String> = h
                .lines
                .iter()
                .filter(|l| !matches!(l.kind, crate::sed::diff::DiffKind::Removed))
                .map(|l| l.text.to_string())
                .collect();
            (h.old_span.0, h.old_span.1, h.new_span.0, h.new_span.1, new_lines)
        })
        .collect();
    let applied = vec![false; spans.len()];

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

    app.diff_state = Some(DiffNavState {
        hunks: spans,
        current_hunk: 0,
        applied,
        original_buffer_id,
        original_window_id,
        original_diff_buffer_id: obid,
        modified_diff_buffer_id: mbid,
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

    let (old_start, _, new_start, _, _) = state.hunks[index];

    // Update both windows: each pane jumps to its own document's
    // hunk start (1-based to 0-based). The old code used old_start
    // for both, mispositioning the modified pane whenever the hunk
    // shifted line numbers.
    let window_ids: Vec<crate::core::id::WindowId> = app.windows.keys().cloned().collect();
    for wid in window_ids {
        if let Some(window) = app.windows.get_mut(&wid)
            && let Some(buffer) = app.buffers.get(&window.buffer_id) {
                use crate::core::buffer::BufferKind;
                let line = match buffer.buffer_kind() {
                    BufferKind::DiffOriginal => old_start.saturating_sub(1),
                    BufferKind::DiffModified => new_start.saturating_sub(1),
                    _ => continue,
                };
                window.cursor_y = line;
                window.cursor_x = 0;
                window.scroll_offset = line.saturating_sub(5);
                window.update_visual_cursor(buffer);
                window.ensure_cursor_visible(buffer);
            }
    }
}

pub fn diff_next_hunk(app: &mut EditorApp, _count: usize) -> DispatchResult {
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

pub fn diff_previous_hunk(app: &mut EditorApp, _count: usize) -> DispatchResult {
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

pub fn diff_accept_hunk(app: &mut EditorApp, _count: usize) -> DispatchResult {
        let mut state = match app.diff_state.take() {
            Some(s) => s,
            None => return DispatchResult::NotHandled,
        };

        let idx = state.current_hunk;
        if state.applied.get(idx).copied().unwrap_or(true) {
            app.message = Some("Hunk already applied".to_string());
            app.diff_state = Some(state);
            return DispatchResult::Success;
        }

        // Spans are in pre-diff coordinates. Every accepted hunk above
        // this one shifts its lines by (replacement lines - old lines);
        // accepted hunks below it do not affect it.
        let offset: isize = state
            .hunks
            .iter()
            .zip(state.applied.iter())
            .take(idx)
            .filter(|(_, applied)| **applied)
            .map(|((old_start, old_end, _, _, new_lines), _)| {
                new_lines.len() as isize - (old_end - old_start) as isize
            })
            .sum();
        let (old_start, old_end, _, _, new_lines) = &state.hunks[idx];
        let old_start = old_start.saturating_add_signed(offset);
        let old_end = old_end.saturating_add_signed(offset);

        if let Some(buffer) = app.buffers.get_mut(&state.original_buffer_id)
            && let Some(start_byte) = buffer.line_to_byte(old_start.saturating_sub(1))
        {
            // Apply hunk to original buffer. The replaced region ends at
            // a line start exactly when it consumed a trailing newline;
            // the replacement text keeps it.
            let after_start = buffer.line_to_byte(old_end.saturating_sub(1));
            let had_trailing_newline = after_start.is_some();
            let end_byte = after_start.unwrap_or_else(|| buffer.len());

            buffer.delete(start_byte, end_byte - start_byte);

            let mut new_text = new_lines.join("\n");
            if had_trailing_newline && !new_text.is_empty() {
                new_text.push('\n');
            }
            if !new_text.is_empty() {
                buffer.insert(start_byte, &new_text);
            }
            state.applied[idx] = true;
        }
        // The hunk can pull lines up from under any window viewing the
        // original buffer; clamp their cursors back into it.
        let original_buffer_id = state.original_buffer_id;
        for window in app.windows.values_mut() {
            if window.buffer_id == original_buffer_id
                && let Some(buffer) = app.buffers.get(&original_buffer_id)
            {
                window.ensure_cursor_valid(buffer);
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

pub fn diff_quit(app: &mut EditorApp, _count: usize) -> DispatchResult {
        if let Some(state) = app.diff_state.take() {
            let original_buffer_id = state.original_buffer_id;
            let scratch = [
                state.original_diff_buffer_id,
                state.modified_diff_buffer_id,
            ];
            app.delete_other_windows();
            if let Some(window) = app.windows.get_mut(&app.active_window) {
                window.buffer_id = original_buffer_id;
            }
            for id in scratch {
                app.buffers.remove(&id);
            }
            return DispatchResult::Success;
        }
        DispatchResult::NotHandled
    }
