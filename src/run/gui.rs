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
        await_deadline: None,
        startup_error: None,
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| GuiError::EventLoop(e.to_string()))?;
    gui_run_outcome(&mut app)
}

/// Decide `run_gui_mode`'s result after the event loop returns: a
/// startup failure recorded by `resumed()` (window/renderer creation)
/// becomes the run's error. Without this, a reachable-but-unrenderable
/// display exits 0 having started nothing.
fn gui_run_outcome(app: &mut GuiApp) -> Result<(), GuiError> {
    match app.startup_error.take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
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
    /// Deadline for an `AwaitLonger` leaf: if no key arrives by then,
    /// the leaf command executes. The TUI loop manages the same
    /// deadline; the GUI was missing it, so e.g. `InsertSpace` on `^C`
    /// never fired without a second event.
    await_deadline: Option<std::time::Instant>,
    /// Window/renderer creation failure recorded by `resumed()`: the
    /// handler signature returns (), so `run_gui_mode` reports the
    /// recorded error after the loop exits instead of exiting 0 having
    /// started nothing.
    startup_error: Option<GuiError>,
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
                            if let Err(e) = terminal::event_handler::process_terminal_event(
                                &mut self.editor,
                                &mut self.display,
                                &mut self.keybinds,
                                resize_event,
                            ) {
                                self.editor.message = Some(format!("Error: {e}"));
                            }

                            // Preload fonts for initial buffer content
                            if let Err(e) = self.display.render(&mut self.editor) {
                                self.editor.message = Some(format!("Error: {e}"));
                            }
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
                            // Recorded for run_gui_mode to report after
                            // the loop exits: the typed error reaches the
                            // dispatch, which fails loudly instead of
                            // exiting 0 having started nothing.
                            self.startup_error =
                                Some(GuiError::Renderer(e.to_string()));
                            event_loop.exit();
                            return;
                        }
                    }
                    self.window = Some(window);
                }
                Err(e) => {
                    // Recorded for run_gui_mode to report after the loop
                    // exits (see the renderer arm above).
                    self.startup_error = Some(GuiError::Window(e.to_string()));
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
                    if let Err(e) = terminal::event_handler::process_terminal_event(
                        &mut self.editor,
                        &mut self.display,
                        &mut self.keybinds,
                        resize_event,
                    ) {
                        self.editor.message = Some(format!("Error: {e}"));
                    }

                    self.dirty = true;
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if self.dirty {
                    if let Err(e) = self.display.render(&mut self.editor) {
                        self.editor.message = Some(format!("Error: {e}"));
                    }
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
                    match renderer.render(&self.display.back_buffer, &self.display.theme) {
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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(cmd) = self.fire_await_deadline() {
            match terminal::event_handler::dispatch_command(
                &mut self.editor,
                &mut self.display,
                cmd,
            ) {
                Ok(should_exit) => {
                    if should_exit {
                        event_loop.exit();
                        return;
                    }
                }
                Err(e) => {
                    self.editor.message = Some(format!("Error: {e}"));
                }
            }
            self.dirty = true;
        }
        // Wake at the deadline when armed; block otherwise.
        match self.await_deadline {
            Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
        if self.dirty
            && let Some(window) = &self.window {
                window.request_redraw();
            }
    }
}

impl GuiApp {
    /// Arm the await_longer deadline when the key trie is positioned on
    /// a leaf that a longer sequence could extend.
    fn arm_await_deadline(&mut self) {
        if self.keybinds.is_awaiting() && self.await_deadline.is_none() {
            self.await_deadline = Some(
                std::time::Instant::now()
                    + std::time::Duration::from_millis(
                        crate::config::keys::KEY_SEQUENCE_TIMEOUT_MS,
                    ),
            );
        }
    }

    /// If the await_longer deadline passed with no further key, take
    /// the leaf command to run now. Same semantics as the TUI loop.
    fn fire_await_deadline(&mut self) -> Option<crate::core::command::Command> {
        if self
            .await_deadline
            .map(|d| std::time::Instant::now() >= d)
            .unwrap_or(false)
        {
            self.await_deadline = None;
            self.keybinds.take_awaited()
        } else {
            None
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

            self.arm_await_deadline();

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

        // Convert scroll delta to scroll direction. The line count is
        // dropped: the handler scrolls a fixed step per wheel event.
        let kind = match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                if y > 0.0 {
                    core::input::MouseEventKind::ScrollUp
                } else {
                    core::input::MouseEventKind::ScrollDown
                }
            }
            MouseScrollDelta::PixelDelta(pos) => {
                if pos.y > 0.0 {
                    core::input::MouseEventKind::ScrollUp
                } else {
                    core::input::MouseEventKind::ScrollDown
                }
            }
        };

        // Scroll position clamps onto the grid: a wheel over the scrollbar
        // strips or the status line scrolls the adjacent pane. With no
        // prior cursor position there is no pane under the cursor; the
        // event is dropped rather than fabricated at (0, 0), which would
        // scroll the top-left pane for a wheel that happened elsewhere.
        let (col, row) = match (self.cursor_pos, self.renderer.as_ref()) {
            (Some((x, y)), Some(renderer)) => {
                crate::gui::input::mouse_pos_to_grid_clamped(x as f32, y as f32, &renderer.metrics)
            }
            _ => return,
        };

        let mouse_event = core::input::MouseEvent {
            column: col as u16,
            row: row as u16,
            kind,
            shift: self.modifiers.shift_key(),
            alt: self.modifiers.alt_key(),
            ctrl: self.modifiers.control_key(),
            click_count: 1,
        };

        self.forward_mouse(mouse_event);
    }

    fn handle_mouse_input(
        &mut self,
        _event_loop: &ActiveEventLoop,
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
        };

        self.forward_mouse(mouse_event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::command::Command;
    use crate::core::input::{Key, KeyPress};

    fn test_app() -> GuiApp {
        let mut keybinds = terminal::keybinds::KeyBindingManager::new();
        keybinds.load(bindings_for_model(INPUT_MODEL));
        GuiApp {
            window: None,
            renderer: None,
            editor: core::app::EditorApp::new(),
            display: terminal::display::Display::new(120, 40, None, None),
            keybinds,
            modifiers: winit::keyboard::ModifiersState::default(),
            dirty: true,
            cursor_pos: None,
            pressed_button: None,
            await_deadline: None,
            startup_error: None,
        }
    }

    #[test]
    fn await_longer_deadline_arms_and_fires() {
        // ^C is an AwaitLonger leaf (insert-space) and the prefix of
        // ^C Tab: feeding it arms the deadline.
        let mut app = test_app();
        let _ = app
            .keybinds
            .process_key(KeyPress::ctrl(Key::Char('c')));
        assert!(app.keybinds.is_awaiting());

        app.arm_await_deadline();
        let deadline = app.await_deadline.expect("deadline armed");
        assert!(deadline > std::time::Instant::now());

        // Re-arming while armed keeps the original deadline.
        app.arm_await_deadline();
        assert_eq!(app.await_deadline, Some(deadline));

        // Not yet expired: nothing fires.
        assert_eq!(app.fire_await_deadline(), None);
        assert!(app.await_deadline.is_some());

        // Expired: the leaf command is taken and the deadline clears.
        app.await_deadline =
            Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
        assert_eq!(
            app.fire_await_deadline(),
            Some(Command::InsertSpace)
        );
        assert_eq!(app.await_deadline, None);
    }

    #[test]
    fn await_longer_deadline_idle_does_not_fire() {
        let mut app = test_app();
        assert!(!app.keybinds.is_awaiting());
        app.arm_await_deadline();
        assert_eq!(app.await_deadline, None);
        assert_eq!(app.fire_await_deadline(), None);
    }

    #[test]
    fn recorded_startup_error_becomes_the_run_error() {
        // The contract run_gui_mode's tail implements: a startup failure
        // recorded by resumed() (window/renderer creation) is reported
        // as the run's error instead of exiting 0 having started nothing.
        // The old code returned Ok(()) unconditionally here.
        let mut app = test_app();
        assert!(gui_run_outcome(&mut app).is_ok());

        let mut app = test_app();
        app.startup_error = Some(GuiError::Window("no window".to_string()));
        assert!(matches!(gui_run_outcome(&mut app), Err(GuiError::Window(_))));

        let mut app = test_app();
        app.startup_error = Some(GuiError::Renderer("no gl".to_string()));
        assert!(matches!(
            gui_run_outcome(&mut app),
            Err(GuiError::Renderer(_))
        ));

        let mut app = test_app();
        app.startup_error = Some(GuiError::EventLoop("no loop".to_string()));
        assert!(matches!(
            gui_run_outcome(&mut app),
            Err(GuiError::EventLoop(_))
        ));
    }
}
