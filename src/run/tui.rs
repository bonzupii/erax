//! Terminal (TUI) mode implementation.

use std::io;

use crate::cli::{ColorMode, Invocation};
use crate::config::keys::bindings_for_model;
use crate::core;
use crate::terminal;
use crate::terminal::events::EditorEvent;

/// Typed errors for terminal-mode startup and the event loop.
#[derive(Debug)]
pub enum TuiError {
    /// The terminal size could not be queried.
    TerminalSize(std::io::Error),
    /// Raw mode could not be enabled.
    RawMode(std::io::Error),
    /// Rendering failed.
    Render(std::io::Error),
    /// An input event could not be read.
    Event(String),
}

impl std::fmt::Display for TuiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TuiError::TerminalSize(e) => write!(f, "cannot get terminal size: {e}"),
            TuiError::RawMode(e) => write!(f, "cannot enable raw mode: {e}"),
            TuiError::Render(e) => write!(f, "render error: {e}"),
            TuiError::Event(e) => write!(f, "input error: {e}"),
        }
    }
}

/// Run in terminal (TUI) mode.
pub fn run_terminal_mode(
    inv: &Invocation,
    color_mode: ColorMode,
) -> Result<(), TuiError> {
    let mut keybind_manager = terminal::keybinds::KeyBindingManager::new();
    keybind_manager.load(bindings_for_model(crate::config::keys::INPUT_MODEL));

    let mut app = core::app::EditorApp::initialize(&inv.files);

    let mut display =
        terminal::display::Display::new_terminal(inv.theme, inv.wrap).map_err(TuiError::TerminalSize)?;
    // The CLI color mode decides the display's color capability (N-002).
    display.display_mode = match color_mode {
        ColorMode::TrueColor => terminal::capabilities::DisplayMode::TrueColor,
        ColorMode::Ansi16 => terminal::capabilities::DisplayMode::Ansi,
        ColorMode::Mono => terminal::capabilities::DisplayMode::Ascii,
    };
    {
        // Window dimensions come from the unified frame, the same source
        // the renderer and the resize handler use.
        let (cols, rows) = display.terminal_size;
        let frame = core::geometry::compute_frame(
            core::geometry::FrameOptions {
                cols: cols as usize,
                rows: rows as usize,
                menu_rows: display.menu_bar_height(),
                show_line_numbers: display.show_line_numbers,
            },
            &app.layout,
            &app.geom_inputs(),
        );
        frame.apply_window_dims(&mut app.windows);
    }

    // Initialize GPM for Linux console mouse support
    #[cfg(target_os = "linux")]
    let mut gpm_client = {
        let mut client = terminal::gpm::GpmClient::new();
        if terminal::gpm::GpmClient::should_use_gpm() {
            let _ = client.connect();
        }
        client
    };

    let _raw_mode = terminal::raw::RawMode::new()
        .map_err(|e| TuiError::RawMode(io::Error::other(e.to_string())))?;
    display.render(&mut app).map_err(|e| TuiError::Render(io::Error::other(e.to_string())))?;
    // Render menu bar on initial display
    {
        let menu_bar = display.menu_bar.clone();
        let show = display.show_menu_bar;
        display.render_menu_bar(&menu_bar, show);
    }
    let mut stdout = io::stdout();
    terminal::render::render_display_to_terminal(&mut display, &mut stdout)
        .map_err(|e| TuiError::Render(io::Error::other(e.to_string())))?;

    let mut event_handler = terminal::events::EventHandler::new();
    // Deadline for an `AwaitLonger` leaf: if no key arrives by then, the leaf
    // command executes (N-013).
    let mut await_deadline: Option<std::time::Instant> = None;
    let await_timeout =
        std::time::Duration::from_millis(crate::config::keys::KEY_SEQUENCE_TIMEOUT_MS);
    loop {
        // If we're waiting on an await_longer leaf and the deadline passed,
        // execute the leaf now.
        if await_deadline.map(|d| std::time::Instant::now() >= d).unwrap_or(false) {
            await_deadline = None;
            if let Some(cmd) = keybind_manager.take_awaited()
                && terminal::event_handler::dispatch_command(&mut app, &mut display, cmd)
                    .map_err(|e| TuiError::Event(e.to_string()))?
                {
                    break;
                }
        }
        // Arm the deadline when the trie is positioned on an await_longer leaf.
        if keybind_manager.is_awaiting() && await_deadline.is_none() {
            await_deadline = Some(std::time::Instant::now() + await_timeout);
        }
        let mut event_processed = false;

        // Poll GPM events on Linux console (Non-blocking, Check First)
        #[cfg(target_os = "linux")]
        if gpm_client.is_connected()
            && let Some(mouse_event) = gpm_client.poll() {
                // Update mouse cursor position in display for rendering
                let (gx, gy) = gpm_client.cursor_pos();
                display.mouse_cursor = Some((gx, gy));

                // Force redraw on mouse event to update cursor position
                display.dirty = true;

                let event = EditorEvent::Mouse(mouse_event);
                let exit = terminal::event_handler::process_terminal_event(
                    &mut app,
                    &mut display,
                    &mut keybind_manager,
                    event,
                )
                .map_err(|e| TuiError::Event(e.to_string()))?;
                if exit {
                    break;
                }
                event_processed = true;
            }

        // Poll crossterm events
        // If we processed a mouse event, don't block (poll immediately for keys).
        // If idle, wait up to 10ms (100Hz) to save CPU but remain responsive.
        let poll_timeout = if event_processed {
            std::time::Duration::from_millis(0)
        } else {
            std::time::Duration::from_millis(10)
        };

        if event_handler
            .poll(poll_timeout)
            .map_err(|e| TuiError::Event(e.to_string()))?
        {
            let event = event_handler
                .read()
                .map_err(|e| TuiError::Event(e.to_string()))?;
            let exit = terminal::event_handler::process_terminal_event(
                &mut app,
                &mut display,
                &mut keybind_manager,
                event,
            )
            .map_err(|e| TuiError::Event(e.to_string()))?;
            if exit {
                break;
            }
        }

        if let Some(ref mut host) = app.terminal_host
            && host.is_alive()
            && host.read().is_some()
        {
            // The child wrote: its grid changed, so re-render. An empty
            // poll leaves the frame alone instead of forcing 100Hz
            // re-renders while the terminal idles.
            display.dirty = true;
        }

        // Accessibility cues: drain the core's cue bus into the TUI's
        // senses (bell, visual flash). The sink marks the display
        // dirty itself when a cue needs a frame, so this runs even
        // when nothing else dirtied the display.
        crate::terminal::cue_sink::drain_cues_to_tui(
            &mut app,
            &mut display,
            crate::config::accessibility::CUE_ERROR_BELL,
            crate::config::accessibility::CUE_VISUAL_FLASH,
        );

        if display.dirty {
            display
                .render(&mut app)
                .map_err(|e| TuiError::Render(io::Error::other(e.to_string())))?;

            let menu_bar = display.menu_bar.clone();
            let show = display.show_menu_bar;
            display.render_menu_bar(&menu_bar, show);

            // Apply GPM mouse cursor overlay to back buffer (Invert output)
            // Done AFTER UI rendering (including menu) to ensure it's on top
            #[cfg(target_os = "linux")]
            if let Some((mx, my)) = display.mouse_cursor {
                let width = display.back_buffer.width as usize;
                let idx = (my as usize) * width + (mx as usize);
                if idx < display.back_buffer.cells.len() {
                    let cell = &mut display.back_buffer.cells[idx];
                    // Invert colors for cursor
                    std::mem::swap(&mut cell.fg, &mut cell.bg);
                    cell.role = crate::terminal::display::CellRole::Cursor;
                }
            }

            let mut stdout = io::stdout();
            terminal::render::render_display_to_terminal(&mut display, &mut stdout)
                .map_err(|e| TuiError::Render(io::Error::other(e.to_string())))?;

            display.swap_buffers();
        }
    }
    Ok(())
}
