//! GUI mode: dumb passthrough for the TUI plus shader presentation.
//!
//! The GUI owns no editor state or input semantics (principle 12). It renders
//! the TUI screen model and forwards raw input events to the backend event
//! handler. All editor logic lives in the backend exactly once.

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::cli::Invocation;
use crate::config::keys::{INPUT_MODEL, bindings_for_model};
use crate::core;
use crate::terminal;

/// Typed errors for GUI startup.
#[derive(Debug)]
pub enum GuiError {
    /// The event loop could not be created.
    EventLoop(String),
    /// The window could not be created.
    Window(String),
    /// The renderer could not be initialized.
    Renderer(String),
}

impl std::fmt::Display for GuiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GuiError::EventLoop(e) => write!(f, "cannot create event loop: {e}"),
            GuiError::Window(e) => write!(f, "cannot create window: {e}"),
            GuiError::Renderer(e) => write!(f, "cannot initialize renderer: {e}"),
        }
    }
}

/// Run in GUI mode.
pub fn run_gui_mode(inv: &Invocation) -> Result<(), GuiError> {
    let mut keybind_manager = terminal::keybinds::KeyBindingManager::new();
    // Load the keymap for the compile-time input model.
    keybind_manager.load(bindings_for_model(INPUT_MODEL));

    let editor_app = core::app::EditorApp::initialize(&inv.files);
    let display = terminal::display::Display::new(120, 40, inv.theme, inv.wrap);

    let event_loop = EventLoop::new().map_err(|e| GuiError::EventLoop(e.to_string()))?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = GuiApp {
        window: None,
        renderer: None,
        editor: editor_app,
        display,
        keybinds: keybind_manager,
        modifiers: winit::keyboard::ModifiersState::default(),
        dirty: true,
        cursor_pos: None,
        pressed_button: None,
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| GuiError::EventLoop(e.to_string()))?;
    Ok(())
}

struct GuiApp {
    window: Option<Arc<Window>>,
    renderer: Option<crate::gui::GridRenderer>,
    editor: core::app::EditorApp,
    display: terminal::display::Display,
    keybinds: terminal::keybinds::KeyBindingManager,
    modifiers: winit::keyboard::ModifiersState,
    dirty: bool,
    /// Last cursor position in pixels (for wheel events); transient input
    /// state, not editor state.
    cursor_pos: Option<(f64, f64)>,
    /// Currently pressed mouse button, for Drag vs Moved synthesis.
    /// Transient input state; click counting lives in the backend.
    pressed_button: Option<winit::event::MouseButton>,
}

impl ApplicationHandler for GuiApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window_attrs = Window::default_attributes()
                .with_title("erax")
                .with_inner_size(winit::dpi::LogicalSize::new(1200, 800));

            match event_loop.create_window(window_attrs) {
                Ok(window) => {
                    let window = Arc::new(window);
                    // Native shell: resolve the configured font to bytes
                    // (platform lookup) and build the presentation core.
                    match crate::gui::shell::native::create_renderer(
                        window.clone(),
                        window.scale_factor(),
                    ) {
                        Ok(mut renderer) => {
                            let (cols, rows) = renderer.grid_size();

                            // Delegate resize handling to TUI
                            let resize_event =
                                terminal::events::EditorEvent::Resize(cols as u16, rows as u16);
                            let _ = terminal::event_handler::process_terminal_event(
                                &mut self.editor,
                                &mut self.display,
                                &mut self.keybinds,
                                resize_event,
                            );

                            // Preload fonts for initial buffer content
                            let _ = self.display.render(&mut self.editor);
                            renderer.preload_fonts_for_buffer(&self.display.back_buffer);

                            // Pre-warm ALL chars in the document (not just viewport)
                            if let Some(buffer) = self.editor.active_buffer() {
                                let doc_text = buffer.content_string();
                                renderer.preload_document(&doc_text);
                            }

                            self.renderer = Some(renderer);
                            self.dirty = true;
                        }
                        Err(e) => {
                            eprintln!("Failed to create renderer: {}", e);
                            event_loop.exit();
                            return;
                        }
                    }
                    self.window = Some(window);
                }
                Err(e) => {
                    eprintln!("Failed to create window: {}", e);
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize((size.width, size.height));
                    let (cols, rows) = renderer.grid_size();

                    // Delegate resize handling to TUI
                    let resize_event =
                        terminal::events::EditorEvent::Resize(cols as u16, rows as u16);
                    let _ = terminal::event_handler::process_terminal_event(
                        &mut self.editor,
                        &mut self.display,
                        &mut self.keybinds,
                        resize_event,
                    );

                    self.dirty = true;
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if self.dirty {
                    let _ = self.display.render(&mut self.editor);
                    let show = self.display.show_menu_bar;
                    self.display.render_menu_bar(show);
                    self.dirty = false;
                }
                if let Some(renderer) = &mut self.renderer {
                    // Backend cursor shape → presentation core (verbatim;
                    // position comes from the Cursor-role cell in the buffer).
                    let dpr = self
                        .window
                        .as_ref()
                        .map(|w| w.scale_factor() as f32)
                        .unwrap_or(1.0);
                    renderer.set_cursor(self.display.cursor_style, dpr);
                    match renderer.render(&self.display.back_buffer) {
                        Ok(_) => {}
                        Err(crate::gui::RenderError::SurfaceLost) => {
                            let size = renderer.size();
                            renderer.resize(size);
                        }
                        Err(e) => {
                            eprintln!("Render error: {e}");
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.handle_keyboard(event_loop, event);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.handle_cursor_moved(position);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.handle_mouse_wheel(delta);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.handle_mouse_input(event_loop, state, button);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.dirty
            && let Some(window) = &self.window {
                window.request_redraw();
            }
    }
}

impl Drop for GuiApp {
    fn drop(&mut self) {
        // Explicitly drop renderer first - it holds a Surface that references the Window
        drop(self.renderer.take());
        // Now the window can be safely dropped
        drop(self.window.take());
    }
}

// Event handler helper methods
impl GuiApp {
    fn handle_keyboard(&mut self, event_loop: &ActiveEventLoop, event: winit::event::KeyEvent) {
        // Only process key presses (not releases).
        if event.state != winit::event::ElementState::Pressed {
            return;
        }

        // Dumb passthrough (principle 12): convert to the backend's typed
        // KeyPress and forward raw to the TUI event handler. No GUI-side
        // input semantics.
        let press = crate::gui::input::winit_key_to_press(&event, self.modifiers);

        if let Some(press) = press {
            // Create EditorEvent and delegate to TUI event handler
            let editor_event = terminal::events::EditorEvent::Input(press);

            // Use TUI event handler for all keyboard processing
            match terminal::event_handler::process_terminal_event(
                &mut self.editor,
                &mut self.display,
                &mut self.keybinds,
                editor_event,
            ) {
                Ok(should_exit) => {
                    if should_exit {
                        event_loop.exit();
                        return;
                    }
                }
                Err(e) => {
                    self.editor.message = Some(format!("Error: {}", e));
                }
            }

            self.dirty = true;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }

    fn handle_cursor_moved(&mut self, position: winit::dpi::PhysicalPosition<f64>) {
        self.cursor_pos = Some((position.x, position.y));

        let grid_pos = if let Some(renderer) = self.renderer.as_ref() {
            crate::gui::input::mouse_pos_to_grid(
                position.x as f32,
                position.y as f32,
                &renderer.metrics,
            )
        } else {
            None
        };

        let Some((col, row)) = grid_pos else {
            return;
        };

        // Dumb passthrough: Drag if a button is held, else Moved.
        // Click counting lives in the backend (no GUI-side synthesis).
        let kind = match self.pressed_button {
            Some(winit::event::MouseButton::Left) => {
                core::input::MouseEventKind::Drag(core::input::MouseButton::Left)
            }
            Some(winit::event::MouseButton::Right) => {
                core::input::MouseEventKind::Drag(core::input::MouseButton::Right)
            }
            Some(winit::event::MouseButton::Middle) => {
                core::input::MouseEventKind::Drag(core::input::MouseButton::Middle)
            }
            _ => core::input::MouseEventKind::Moved,
        };

        let mouse_event = core::input::MouseEvent {
            column: col as u16,
            row: row as u16,
            kind,
            shift: self.modifiers.shift_key(),
            alt: self.modifiers.alt_key(),
            ctrl: self.modifiers.control_key(),
            click_count: 1,
            scroll_amount: 0,
        };

        self.forward_mouse(mouse_event);
    }

    /// Forward a mouse event to the backend event handler (dumb passthrough).
    fn forward_mouse(&mut self, mouse_event: core::input::MouseEvent) {
        let editor_event = terminal::events::EditorEvent::Mouse(mouse_event);
        if terminal::event_handler::process_terminal_event(
            &mut self.editor,
            &mut self.display,
            &mut self.keybinds,
            editor_event,
        )
        .is_ok()
            && self.display.dirty {
                self.dirty = true;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
    }

    fn handle_mouse_wheel(&mut self, delta: winit::event::MouseScrollDelta) {
        use winit::event::MouseScrollDelta;

        // Convert scroll delta to scroll direction
        let (kind, lines) = match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                if y > 0.0 {
                    (
                        core::input::MouseEventKind::ScrollUp,
                        (y.abs() * 3.0).round() as u8,
                    )
                } else {
                    (
                        core::input::MouseEventKind::ScrollDown,
                        (y.abs() * 3.0).round() as u8,
                    )
                }
            }
            MouseScrollDelta::PixelDelta(pos) => {
                if pos.y > 0.0 {
                    (
                        core::input::MouseEventKind::ScrollUp,
                        (pos.y.abs() / 20.0).round().max(1.0) as u8,
                    )
                } else {
                    (
                        core::input::MouseEventKind::ScrollDown,
                        (pos.y.abs() / 20.0).round().max(1.0) as u8,
                    )
                }
            }
        };

        // Scroll position clamps onto the grid: a wheel over the scrollbar
        // strips or the status line scrolls the adjacent pane. The old
        // `unwrap_or_default()` attributed those events to cell (0, 0),
        // scrolling the top-left pane instead of the one under the cursor.
        let (col, row) =
            if let (Some((x, y)), Some(renderer)) = (self.cursor_pos, self.renderer.as_ref()) {
                crate::gui::input::mouse_pos_to_grid_clamped(x as f32, y as f32, &renderer.metrics)
            } else {
                (0, 0)
            };

        let mouse_event = core::input::MouseEvent {
            column: col as u16,
            row: row as u16,
            kind,
            shift: self.modifiers.shift_key(),
            alt: self.modifiers.alt_key(),
            ctrl: self.modifiers.control_key(),
            click_count: 1,
            scroll_amount: lines,
        };

        self.forward_mouse(mouse_event);
    }

    fn handle_mouse_input(
        &mut self,
        event_loop: &ActiveEventLoop,
        state: winit::event::ElementState,
        button: winit::event::MouseButton,
    ) {
        use winit::event::ElementState;

        // Track pressed button for Drag synthesis; forward ALL states
        // including release. Click counting lives in the backend.
        if state == ElementState::Pressed {
            self.pressed_button = Some(button);
        } else {
            self.pressed_button = None;
        }

        // Convert pixel position to grid
        let grid_pos =
            if let (Some((x, y)), Some(renderer)) = (self.cursor_pos, self.renderer.as_ref()) {
                crate::gui::input::mouse_pos_to_grid(x as f32, y as f32, &renderer.metrics)
            } else {
                None
            };

        let Some((col, row)) = grid_pos else {
            return;
        };

        // Convert winit button to core button
        let core_button = match button {
            winit::event::MouseButton::Left => core::input::MouseButton::Left,
            winit::event::MouseButton::Right => core::input::MouseButton::Right,
            winit::event::MouseButton::Middle => core::input::MouseButton::Middle,
            _ => core::input::MouseButton::Left,
        };

        let kind = match state {
            ElementState::Pressed => core::input::MouseEventKind::Down(core_button),
            ElementState::Released => core::input::MouseEventKind::Up(core_button),
        };

        // Create MouseEvent and delegate to TUI (dumb passthrough)
        let mouse_event = core::input::MouseEvent {
            column: col as u16,
            row: row as u16,
            kind,
            shift: self.modifiers.shift_key(),
            alt: self.modifiers.alt_key(),
            ctrl: self.modifiers.control_key(),
            click_count: 1,
            scroll_amount: 0,
        };

        self.forward_mouse(mouse_event);
        let _ = event_loop;
    }
}
