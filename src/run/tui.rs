//! Terminal (TUI) mode implementation.

use std::io;
use std::path::PathBuf;

use crate::config::Config;
use crate::core;
use crate::terminal;
use crate::terminal::events::EditorEvent;

/// Run in terminal (TUI) mode.
pub fn run_terminal_mode(
    files: &[PathBuf],
    config: &Config,
    _display_mode: terminal::capabilities::DisplayMode,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut keybind_manager = terminal::keybinds::KeyBindingManager::new();
    for (binding, command) in &config.keybindings {
        keybind_manager.bind(binding, command.clone());
    }

    let mut app = core::app::EditorApp::initialize_with_config(config, files);

    let mut display = terminal::display::Display::new_terminal(config)?;
    {
        let (cols, rows) = display.terminal_size;
        let viewport = core::layout::Viewport::new(cols, rows, display.show_menu_bar);
        let window_rects = app.layout.collect_windows(viewport.editor);
        for (window_id, win_rect) in window_rects {
            if let Some(window) = app.windows.get_mut(&window_id) {
                window.set_dimensions(win_rect.width, win_rect.height.saturating_sub(1));
            }
        }
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

    let _raw_mode = terminal::raw::RawMode::new()?;
    display.render(&mut app)?;
    // Render menu bar on initial display
    {
        let menu_bar = display.menu_bar.clone();
        let show = display.show_menu_bar;
        display.render_menu_bar(&menu_bar, show);
    }
    let mut stdout = io::stdout();
    terminal::render::render_display_to_terminal(&display, &mut stdout)?;

    let mut event_handler = terminal::events::EventHandler::new();
    loop {
        let mut event_processed = false;

        // Poll GPM events on Linux console (Non-blocking, Check First)
        #[cfg(target_os = "linux")]
        if gpm_client.is_connected() {
            if let Some(mouse_event) = gpm_client.poll() {
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
                )?;
                if exit {
                    break;
                }
                event_processed = true;
            }
        }

        // Poll crossterm events
        // If we processed a mouse event, don't block (poll immediately for keys).
        // If idle, wait up to 10ms (100Hz) to save CPU but remain responsive.
        let poll_timeout = if event_processed {
            std::time::Duration::from_millis(0)
        } else {
            std::time::Duration::from_millis(10)
        };

        if event_handler.poll(poll_timeout)? {
            let event = event_handler.read()?;
            let exit = terminal::event_handler::process_terminal_event(
                &mut app,
                &mut display,
                &mut keybind_manager,
                event,
            )?;
            if exit {
                break;
            }
        }

        if let Some(ref mut host) = app.terminal_host {
            if host.is_alive() {
                // Non-blocking read from terminal host
                let _ = host.read();
                display.dirty = true;
            }
        }

        if display.dirty {
            display.render(&mut app)?;

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
                    let temp = cell.fg;
                    cell.fg = cell.bg;
                    cell.bg = temp;
                }
            }

            let mut stdout = io::stdout();
            terminal::render::render_display_to_terminal(&display, &mut stdout)?;

            display.swap_buffers();
        }
    }
    Ok(())
}
