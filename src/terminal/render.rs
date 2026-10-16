//! Terminal rendering: full-crossterm batched output.
//!
//! The render path uses crossterm commands queued into a single buffer and
//! flushed once per frame (N-106, N-107). Colors are `crossterm::style::Color`
//! values — `Copy`, compared by value on the stack, with no per-cell
//! allocation (N-104). ANSI run-batching: consecutive cells with identical
//! fg/bg emit one color command for the run.

use std::io::{Stdout, Write};

use crossterm::{
    QueueableCommand,
    cursor::{Hide, MoveTo, Show},
    style::{Color as CrosstermColor, Print, SetBackgroundColor, SetForegroundColor},
    terminal::{Clear, ClearType},
};

use crate::cli::ColorMode;
use crate::terminal::color::Color;
use crate::terminal::display::Display;

/// Render the display's back buffer to the terminal.
///
/// Diffs against the front buffer, batches crossterm commands, and flushes
/// once. Generic over `Write` so the batching is benchmarkable without a TTY.
pub fn render_display_to_terminal(
    display: &mut Display,
    stdout: &mut Stdout,
) -> Result<(), Box<dyn std::error::Error>> {
    render_display(display, stdout)?;
    stdout.flush()?;
    Ok(())
}

/// The batched render core, generic over the writer (benchmark seam).
pub fn render_display<W: Write>(
    display: &mut Display,
    out: &mut W,
) -> Result<(), Box<dyn std::error::Error>> {
    // Hide cursor during update to prevent flickering.
    out.queue(Hide)?;

    // Full redraw on request (e.g. after resize).
    let needs_full = display.needs_full_redraw;
    if needs_full {
        out.queue(Clear(ClearType::All))?;
        out.queue(MoveTo(0, 0))?;
        // The full redraw is consumed by this render.
        display.needs_full_redraw = false;
    }

    let front_buffer = &display.front_buffer;
    let back_buffer = &display.back_buffer;

    // Last emitted colors as Copy stack values (N-104: no per-cell allocation).
    let mut last_fg: Option<CrosstermColor> = None;
    let mut last_bg: Option<CrosstermColor> = None;
    let mut cursor_moved = false;

    // Unchanged cells are skipped by the per-cell diff below; no
    // row-level tracker is needed.
    for y in 0..display.terminal_size.1 {
        for x in 0..display.terminal_size.0 {
            let new_cell = match back_buffer.get(x, y) {
                Some(cell) => cell,
                None => continue,
            };
            if !needs_full
                && let Some(old) = front_buffer.get(x, y)
                    && old == new_cell {
                        continue;
                    }
            if new_cell.hidden {
                continue;
            }

            out.queue(MoveTo(x, y))?;
            cursor_moved = true;

            // Resolve colors for the display mode; Ascii emits no color (N-110).
            let (fg, bg) = match display.display_mode {
                ColorMode::TrueColor => (new_cell.fg, new_cell.bg),
                ColorMode::Ansi16 => (
                    new_cell.fg.to_ansi_fallback(),
                    new_cell.bg.to_ansi_fallback(),
                ),
                ColorMode::Mono => (Color::Reset, Color::Reset),
            };
            let fg_term: CrosstermColor = fg.to_crossterm();
            let bg_term: CrosstermColor = bg.to_crossterm();

            // ANSI run-batching: one color command per fg/bg change.
            if Some(fg_term) != last_fg {
                out.queue(SetForegroundColor(fg_term))?;
                last_fg = Some(fg_term);
            }
            if Some(bg_term) != last_bg {
                out.queue(SetBackgroundColor(bg_term))?;
                last_bg = Some(bg_term);
            }

            out.queue(Print(new_cell.text()))?;
        }
    }

    // Reset colors if we moved the cursor (emitted any styled output).
    if cursor_moved {
        out.queue(SetForegroundColor(CrosstermColor::Reset))?;
        out.queue(SetBackgroundColor(CrosstermColor::Reset))?;
    }

    // Emit the cursor style (DECSCUSR) when it changed since the last frame.
    // The style itself is derived from backend mode state in Display::render
    // (mode-aware UI, D6); this only publishes it to the terminal.
    if display.last_emitted_cursor_style != Some(display.cursor_style) {
        out.write_all(&display.cursor_style.decscusr_bytes())?;
        display.last_emitted_cursor_style = Some(display.cursor_style);
    }

    // Position cursor at the end and show/hide explicitly.
    if let Some((cx, cy)) = display.cursor_pos {
        out.queue(MoveTo(cx as u16, cy as u16))?;
        out.queue(Show)?;
    } else {
        out.queue(Hide)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::display::Display;

    #[test]
    fn render_batches_without_allocating_per_cell() {
        // Render a small display into a Vec<u8>; assert it produces output
        // and that repeated identical frames diff to (almost) nothing.
        let mut display = Display::new(10, 5, None, None);
        let mut out = Vec::new();
        render_display(&mut display, &mut out).unwrap();
        assert!(!out.is_empty(), "first frame emits output");

        // Second identical frame: front == back after swap, so no cells emit.
        display.swap_buffers();
        let mut out2 = Vec::new();
        render_display(&mut display, &mut out2).unwrap();
        // Only cursor positioning remains; no cell content.
        assert!(
            out2.len() < out.len(),
            "identical frame emits less output"
        );
    }
}
