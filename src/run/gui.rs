//! GUI mode implementation (requires "gui" feature).

use std::path::PathBuf;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::config::{Config, ConfigValue};
use crate::core;
use crate::terminal;

/// Run in GUI mode (requires "gui" feature).
pub fn run_gui_mode(files: &[PathBuf], config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let font_path: Option<String> = config.settings.get("gui_font").and_then(|v| {
        if let ConfigValue::String(s) = v {
            Some(s.clone())
        } else {
            None
        }
    });
    let font_size = match config.settings.get("font_size").and_then(|v| {
        if let ConfigValue::Int(i) = v {
            Some(*i as f32 + 1.0)
        } else {
            None
        }
    }) {
        Some(f) => f,
        None => crate::gui::DEFAULT_FONT_SIZE + 1.0,
    };

    let mut keybind_manager = terminal::keybinds::KeyBindingManager::new();
    for (binding, command) in &config.keybindings {
        keybind_manager.bind(binding, command.clone());
    }

    let editor_app = core::app::EditorApp::initialize_with_config(config, files);

    let display = terminal::display::Display::new_terminal(config)?;

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = GuiApp {
        window: None,
        renderer: None,
        editor: editor_app,
        display,
        keybinds: keybind_manager,
        modifiers: winit::keyboard::ModifiersState::default(),
        font_path,
        font_size,
        dirty: true,
        cursor_pos: None,
        is_mouse_down: false,
        scrollbar_dragging: false,
        last_click_time: None,
        last_click_pos: (0, 0),
        last_click_button: None,
        click_count: 0,
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}

struct GuiApp {
    window: Option<Arc<Window>>,
    renderer: Option<crate::gui::GridRenderer>,
    editor: core::app::EditorApp,
    display: terminal::display::Display,
    keybinds: terminal::keybinds::KeyBindingManager,
    modifiers: winit::keyboard::ModifiersState,
    font_path: Option<String>,
    font_size: f32,
    dirty: bool,
    cursor_pos: Option<(f64, f64)>,
    is_mouse_down: bool,
    scrollbar_dragging: bool,
    last_click_time: Option<std::time::Instant>,
    last_click_pos: (usize, usize),
    last_click_button: Option<winit::event::MouseButton>,
    click_count: u8,
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
                    match crate::gui::GridRenderer::new(
                        window.clone(),
                        self.font_path.as_deref(),
                        self.font_size,
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
                    let menu_bar = self.display.menu_bar.clone();
                    let show = self.display.show_menu_bar;
                    self.display.render_menu_bar(&menu_bar, show);
                    self.dirty = false;
                }
                if let Some(renderer) = &mut self.renderer {
                    match renderer.render(&self.display.back_buffer) {
                        Ok(_) => {}
                        Err(wgpu::SurfaceError::Lost) => {
                            let size = renderer.size();
                            renderer.resize(size);
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            eprintln!("GPU out of memory");
                            event_loop.exit();
                        }
                        Err(e) => {
                            eprintln!("Render error: {:?}", e);
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
        if self.dirty {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
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
        if !crate::gui::input::should_process(event.state) {
            return;
        }

        if let Some(key) = crate::gui::input::winit_key_to_key(event.physical_key, self.modifiers) {
            // Convert winit key to InputEvent
            let input_event = crate::gui::input::create_input_event(key, self.modifiers);

            // Create EditorEvent and delegate to TUI event handler
            let editor_event = terminal::events::EditorEvent::Input(input_event);

            // Use TUI event handler for all keyboard processing
            match terminal::event_handler::process_terminal_event(
                &mut self.editor,
                &mut self.display,
                &mut self.keybinds, // Changed from keybind_manager to keybinds
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
            let (vw, vh) = renderer.size();
            crate::gui::input::mouse_pos_to_grid(
                position.x as f32,
                position.y as f32,
                renderer.cell_width(),
                renderer.cell_height(),
                vw as f32,
                vh as f32,
            )
        } else {
            None
        };

        let Some((col, row)) = grid_pos else {
            return;
        };

        // Determine event kind based on mouse button state
        let kind = if self.is_mouse_down
            && self.last_click_button == Some(winit::event::MouseButton::Left)
        {
            core::input::MouseEventKind::Drag(core::input::MouseButton::Left)
        } else {
            core::input::MouseEventKind::Moved
        };

        let mouse_event = core::input::MouseEvent {
            column: col as u16,
            row: row as u16,
            kind,
            shift: self.modifiers.shift_key(),
            alt: self.modifiers.alt_key(),
            ctrl: self.modifiers.control_key(),
            click_count: self.click_count,
        };

        let editor_event = terminal::events::EditorEvent::Mouse(mouse_event);

        if let Ok(_) = terminal::event_handler::process_terminal_event(
            &mut self.editor,
            &mut self.display,
            &mut self.keybinds,
            editor_event,
        ) {
            if self.display.dirty {
                self.dirty = true;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
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

        // Get current cursor grid position (default to 0,0 if not available)
        let (col, row) =
            if let (Some((x, y)), Some(renderer)) = (self.cursor_pos, self.renderer.as_ref()) {
                let (vw, vh) = renderer.size();
                match crate::gui::input::mouse_pos_to_grid(
                    x as f32,
                    y as f32,
                    renderer.cell_width(),
                    renderer.cell_height(),
                    vw as f32,
                    vh as f32,
                ) {
                    Some(pos) => pos,
                    None => (0, 0),
                }
            } else {
                (0, 0)
            };

        let mouse_event = core::input::MouseEvent {
            column: col as u16,
            row: row as u16,
            kind,
            shift: false,
            alt: false,
            ctrl: false,
            click_count: lines, // Repurpose click_count for scroll amount
        };

        let editor_event = terminal::events::EditorEvent::Mouse(mouse_event);

        if let Ok(_) = terminal::event_handler::process_terminal_event(
            &mut self.editor,
            &mut self.display,
            &mut self.keybinds,
            editor_event,
        ) {
            self.dirty = true;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }

    fn handle_mouse_input(
        &mut self,
        event_loop: &ActiveEventLoop,
        state: winit::event::ElementState,
        button: winit::event::MouseButton,
    ) {
        use winit::event::ElementState;

        // Track mouse state for drag detection
        self.is_mouse_down = state == ElementState::Pressed;
        if state == ElementState::Released {
            self.scrollbar_dragging = false;
            return; // TUI doesn't handle mouse up events currently
        }

        // Only handle press events
        if state != ElementState::Pressed {
            return;
        }

        // Convert pixel position to grid
        let grid_pos =
            if let (Some((x, y)), Some(renderer)) = (self.cursor_pos, self.renderer.as_ref()) {
                let (vw, vh) = renderer.size();
                crate::gui::input::mouse_pos_to_grid(
                    x as f32,
                    y as f32,
                    renderer.cell_width(),
                    renderer.cell_height(),
                    vw as f32,
                    vh as f32,
                )
            } else {
                None
            };

        let Some((col, row)) = grid_pos else {
            return;
        };

        // Track click count for double/triple click
        let now = std::time::Instant::now();
        if let Some(last_time) = self.last_click_time {
            if now.duration_since(last_time) < std::time::Duration::from_millis(500)
                && self.last_click_pos == (col, row)
                && self.last_click_button == Some(button)
            {
                self.click_count = self.click_count.saturating_add(1).min(3);
            } else {
                self.click_count = 1;
            }
        } else {
            self.click_count = 1;
        }
        self.last_click_time = Some(now);
        self.last_click_pos = (col, row);
        self.last_click_button = Some(button);

        // Convert winit button to core button
        let core_button = match button {
            winit::event::MouseButton::Left => core::input::MouseButton::Left,
            winit::event::MouseButton::Right => core::input::MouseButton::Right,
            winit::event::MouseButton::Middle => core::input::MouseButton::Middle,
            _ => core::input::MouseButton::Left,
        };

        // Create MouseEvent and delegate to TUI
        let mouse_event = core::input::MouseEvent {
            column: col as u16,
            row: row as u16,
            kind: core::input::MouseEventKind::Down(core_button),
            shift: self.modifiers.shift_key(),
            alt: self.modifiers.alt_key(),
            ctrl: self.modifiers.control_key(),
            click_count: self.click_count,
        };

        let editor_event = terminal::events::EditorEvent::Mouse(mouse_event);

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
