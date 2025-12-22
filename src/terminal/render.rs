//! Terminal rendering module
//!
//! This module handles the low-level rendering of the display buffer to the terminal,
//! using ANSI escape codes and crossterm for cursor control.

use std::io::{Stdout, Write};

use crate::terminal::display::Display;

/// Dedicated rendering function that handles terminal I/O using diffing
///
/// This function renders the display's back buffer to the terminal, using the
/// front buffer for diffing to minimize updates. It handles:
/// - Full redraws when needed (e.g., after resize)
/// - Incremental updates by comparing old and new cells
/// - Color optimization by tracking last used colors
/// - Proper cursor positioning
pub fn render_display_to_terminal(
    display: &Display,
    stdout: &mut Stdout,
) -> Result<(), Box<dyn std::error::Error>> {
    // Hide cursor during update to prevent flickering
    write!(stdout, "\x1b[?25l")?;

    // Handle full redraw request (e.g. after resize) - use ANSI clear sequence
    // Check both display flag AND dirty_tracker's full redraw state
    let needs_full = display.needs_full_redraw || display.dirty_tracker.needs_full_redraw();
    if needs_full {
        write!(stdout, "\x1b[2J\x1b[H")?; // Clear screen and move cursor to top-left
    }

    // Get buffers for diffing
    // back_buffer has the NEW frame
    // front_buffer has the OLD frame (currently on screen)
    let front_buffer = &display.front_buffer;
    let back_buffer = &display.back_buffer;

    let mut last_fg = String::new();
    let mut last_bg = String::new();
    let mut cursor_moved = false;

    for y in 0..display.terminal_size.1 {
        // Use dirty_tracker for row-level incremental rendering
        if !needs_full && !display.dirty_tracker.is_row_dirty(y as usize) {
            continue; // Skip clean rows entirely
        }

        for x in 0..display.terminal_size.0 {
            let new_cell = match back_buffer.get(x, y) {
                Some(cell) => cell,
                None => continue,
            };

            let old_cell = front_buffer.get(x, y);

            // If not full redraw, check if cell changed
            if !needs_full {
                if let Some(old) = old_cell {
                    if old == new_cell {
                        continue; // No change, skip
                    }
                }
            }

            if new_cell.hidden {
                continue;
            }

            // Move cursor to cell position
            write!(stdout, "\x1b[{};{}H", y + 1, x + 1)?;
            cursor_moved = true;

            // Update colors if changed (apply fallback for non-TrueColor terminals)
            let (fg, bg) = match display.display_mode {
                crate::terminal::capabilities::DisplayMode::TrueColor
                | crate::terminal::capabilities::DisplayMode::Gui => (new_cell.fg, new_cell.bg),
                _ => (
                    new_cell.fg.to_ansi_fallback(),
                    new_cell.bg.to_ansi_fallback(),
                ),
            };
            let fg_code = fg.to_ansi_fg_code();
            let bg_code = bg.to_ansi_bg_code();

            if fg_code != last_fg || bg_code != last_bg {
                write!(stdout, "\x1b[{}m\x1b[{}m", fg_code, bg_code)?;
                last_fg = fg_code;
                last_bg = bg_code;
            }

            write!(stdout, "{}", new_cell.ch)?;
        }
    }

    // Reset colors and flush all output
    if cursor_moved {
        write!(stdout, "\x1b[0m")?;
    }

    // Position cursor at the end and SHOW it explicitly using crossterm
    if let Some((cx, cy)) = display.cursor_pos {
        use crossterm::{QueueableCommand, cursor};
        stdout.queue(cursor::MoveTo(cx as u16, cy as u16))?;
        stdout.queue(cursor::Show)?;
    } else {
        use crossterm::{QueueableCommand, cursor};
        stdout.queue(cursor::Hide)?;
    }

    stdout.flush()?;

    Ok(())
}
