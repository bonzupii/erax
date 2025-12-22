use crate::core::buffer::Buffer;
use crate::core::layout::Rect;
use crate::core::window::Window;
use crate::terminal::display::{Cell, ScreenBuffer};
use crate::terminal::prompt::PromptState;
use crate::terminal::theme::Theme;

/// Renders the status line for a window
pub struct StatusRenderer;

impl StatusRenderer {
    /// Render status line to the screen buffer
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        screen_buffer: &mut ScreenBuffer,
        window: &Window,
        buffer: &Buffer,
        rect: &Rect,
        theme: &Theme,
        is_active: bool,
        prompt_state: Option<&PromptState>,
        message: &str,
        key_sequence: &str,
    ) {
        let status_y = (rect.y + rect.height.saturating_sub(1)) as u16;
        let bg_color = if is_active {
            theme.status_line_bg().clone().into()
        } else {
            theme.status_line_inactive_bg().clone().into()
        };
        let fg_color = if is_active {
            theme.status_line_fg().clone().into()
        } else {
            theme.status_line_inactive_fg().clone().into()
        };

        let filename = match buffer.filename.as_ref().map(|p| p.display().to_string()) {
            Some(s) => s,
            None => "[No Name]".to_string(),
        };

        let modified = if buffer.modified { "[+]" } else { "" };
        let pos_info = format!("Ln {}, Col {}", window.cursor_y + 1, window.cursor_x + 1);

        let left = format!(" {} {} ", filename, modified);
        let right = format!(" {} ", pos_info);

        // For active window: show prompt OR message OR key_sequence
        let middle = if is_active {
            if let Some(ref prompt_state) = prompt_state {
                // Show prompt input
                format!("{}{}", prompt_state.prompt, prompt_state.input)
            } else if !message.is_empty() {
                format!(" {} ", message)
            } else if !key_sequence.is_empty() {
                format!(" {} ", key_sequence)
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let used_len = left.len() + middle.len() + right.len();
        let padding = rect.width.saturating_sub(used_len);
        let full_status = format!("{}{}{}{}", left, middle, " ".repeat(padding), right);

        // Render status line
        for (i, ch) in full_status.chars().take(rect.width).enumerate() {
            let screen_x = (rect.x + i) as u16;
            screen_buffer.set(screen_x, status_y, Cell::new(ch, fg_color, bg_color));
        }
    }
}
