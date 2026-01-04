use crate::core::app::EditorApp;
use crate::core::layout::Rect;
use crate::core::spell::SpellChecker;
use crate::core::syntax::SyntaxHighlighter;
use crate::terminal::capabilities::DisplayMode;
use crate::terminal::prompt::PromptState;
use crate::terminal::renderers::DirtyTracker;

// Re-export Color from color module for backward compatibility
pub use crate::terminal::color::Color;

/// Represents a single cell on the terminal screen
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    /// If true, this cell is covered by the previous wide character and should be skipped
    pub hidden: bool,
}

impl Cell {
    pub(crate) fn new(ch: char, fg: Color, bg: Color) -> Self {
        Self {
            ch,
            fg,
            bg,
            hidden: false,
        }
    }

    pub fn hidden() -> Self {
        Self {
            ch: ' ',
            fg: Color::Reset,
            bg: Color::Reset,
            hidden: true,
        }
    }

    pub fn empty() -> Self {
        Self {
            ch: ' ',
            fg: Color::Reset,
            bg: Color::Reset,
            hidden: false,
        }
    }
}

/// Represents the state of the entire screen
#[derive(Clone, Debug)]
pub struct ScreenBuffer {
    pub cells: Vec<Cell>,

    pub width: u16,

    pub height: u16,
}

impl ScreenBuffer {
    fn new(width: u16, height: u16) -> Self {
        let cells = vec![Cell::empty(); (width as usize) * (height as usize)];
        Self {
            cells,
            width,
            height,
        }
    }

    fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.cells = vec![Cell::empty(); (width as usize) * (height as usize)];
    }

    fn clear(&mut self) {
        for cell in &mut self.cells {
            *cell = Cell::empty();
        }
    }

    pub(crate) fn set(&mut self, x: u16, y: u16, cell: Cell) {
        if x < self.width && y < self.height {
            let idx = (y as usize) * (self.width as usize) + (x as usize);
            self.cells[idx] = cell;
        }
    }

    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        if x < self.width && y < self.height {
            let idx = (y as usize) * (self.width as usize) + (x as usize);
            Some(&self.cells[idx])
        } else {
            None
        }
    }
}

use crate::terminal::theme::{Theme, ThemeManager};

/// Manages the terminal display and rendering
pub struct Display {
    /// Terminal dimensions (cols, rows)
    pub terminal_size: (u16, u16),
    /// Whether the display needs a full redraw
    pub dirty: bool,
    /// Whether a full screen clear is needed (e.g. after resize)
    pub needs_full_redraw: bool,
    /// Current status message to display
    pub message: String,
    /// Current key sequence being built (e.g., "C-x-")
    pub key_sequence: String,
    /// Front buffer (currently displayed)
    pub front_buffer: ScreenBuffer,
    /// Back buffer (being drawn to)
    pub back_buffer: ScreenBuffer,
    /// Show line numbers in gutter
    pub show_line_numbers: bool,
    /// Syntax highlighter for code highlighting
    pub syntax_highlighter: SyntaxHighlighter,
    /// Current color theme
    pub theme: Theme,
    /// Current cursor position for GUI
    pub cursor_pos: Option<(usize, usize)>,
    /// Active prompt state for minibuffer input
    pub prompt_state: Option<PromptState>,
    /// Spell checker for prose
    pub spell_checker: SpellChecker,
    /// Menu bar for dropdown menus (TUI and GUI)
    pub menu_bar: crate::core::menu::MenuBar,
    /// Whether to show the menu bar
    pub show_menu_bar: bool,
    /// Dirty region tracker for incremental rendering
    pub dirty_tracker: DirtyTracker,
    /// Software mouse cursor position (for TTYs)
    pub mouse_cursor: Option<(u16, u16)>,
    /// Display mode (TrueColor, Ansi, Ascii) for color fallback
    pub display_mode: DisplayMode,
}

impl Display {
    /// Initialize the display system with specified dimensions
    pub fn new(width: u16, height: u16, config: &crate::config::Config) -> Self {
        let front_buffer = ScreenBuffer::new(width, height);
        let back_buffer = ScreenBuffer::new(width, height);

        // Load theme from config
        let theme_manager = ThemeManager::new();
        let theme_name = match config.settings.get("theme").and_then(|v| match v {
            crate::config::ConfigValue::String(s) => Some(s.as_str()),
            _ => None,
        }) {
            Some(name) => name,
            None => "dracula",
        };

        let theme = match theme_manager.get(theme_name) {
            Some(t) => t,
            None => Theme::default(),
        };

        Self {
            terminal_size: (width, height),
            dirty: true,
            needs_full_redraw: true, // Initial full redraw
            message: String::new(),
            key_sequence: String::new(),
            front_buffer,
            back_buffer,
            show_line_numbers: match config.settings.get("line_numbers").and_then(|v| match v {
                crate::config::ConfigValue::Bool(b) => Some(*b),
                _ => None,
            }) {
                Some(b) => b,
                None => false,
            },
            syntax_highlighter: SyntaxHighlighter::new(),
            theme,
            cursor_pos: None,
            prompt_state: None,
            spell_checker: SpellChecker::new(),
            menu_bar: crate::core::menu::MenuBar::new(),
            show_menu_bar: true,
            dirty_tracker: DirtyTracker::new(width, height),
            mouse_cursor: None,
            display_mode: DisplayMode::detect(),
        }
    }

    /// Initialize the display system with terminal detection (for terminal mode)
    pub fn new_terminal(
        config: &crate::config::Config,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (cols, rows) = Self::get_terminal_size()?;
        Ok(Self::new(cols, rows, config))
    }

    /// Get terminal size using crossterm
    fn get_terminal_size() -> Result<(u16, u16), Box<dyn std::error::Error>> {
        let (cols, rows) = crossterm::terminal::size()?;
        Ok((cols, rows))
    }

    /// Set a status message to display
    pub fn set_message(&mut self, msg: String) {
        self.message = msg;
        self.dirty = true;
    }

    /// Set the current key sequence state
    pub fn set_key_sequence(&mut self, seq: String) {
        self.key_sequence = seq;
        self.dirty = true;
    }

    /// Update terminal size with improved synchronization
    pub fn update_size(&mut self, cols: u16, rows: u16) {
        // Validate new dimensions
        if cols == 0 || rows == 0 {
            return; // Ignore invalid dimensions
        }

        // Update terminal size
        self.terminal_size = (cols, rows);

        // Create fresh buffers - don't preserve old content as it causes
        // visual artifacts. The needs_full_redraw flag ensures everything
        // gets repainted properly.
        self.front_buffer = ScreenBuffer::new(cols, rows);
        self.back_buffer = ScreenBuffer::new(cols, rows);

        // Sync dirty tracker (marks full redraw internally)
        self.dirty_tracker.resize(cols, rows);

        self.dirty = true;
        self.needs_full_redraw = true; // Force full redraw on resize
    }

    /// Render the editor state to the terminal (double-buffering logic only)
    pub fn render(&mut self, app: &mut EditorApp) -> Result<(), Box<dyn std::error::Error>> {
        // Sync message and focus state from EditorApp
        if let Some(msg) = &app.message {
            if self.message != *msg {
                self.message = msg.clone();
                self.dirty = true;
            }
        } else if !self.message.is_empty() {
            self.message = String::new();
            self.dirty = true;
        }

        if let Some(focus) = app.focus_manager.current_state() {
            if focus.target.uses_minibuffer() {
                let mut state = PromptState::new(focus.prompt.clone());
                state.input = focus.input.clone();
                state.cursor = focus.cursor;
                self.prompt_state = Some(state);
                self.dirty = true;
            } else if self.prompt_state.is_some() {
                self.prompt_state = None;
                self.dirty = true;
            }
        } else if self.prompt_state.is_some() {
            self.prompt_state = None;
            self.dirty = true;
        }

        if !self.dirty {
            return Ok(());
        }

        // Handle full redraw request (e.g. after resize)
        if self.needs_full_redraw {
            self.dirty_tracker.mark_full_redraw();
            self.needs_full_redraw = false;
        }

        // 1. Clear back buffer (the one we are drawing to)
        self.back_buffer.clear();

        // 2. Calculate full screen rect
        // Per-window status bars are included in window height, no separate global bar needed
        // IMPORTANT: Account for menu bar height if visible to avoid overlap
        let menu_height = self.menu_bar_height();
        let main_height = (self.terminal_size.1 as usize).saturating_sub(menu_height);
        let full_rect = Rect::new(0, menu_height, self.terminal_size.0 as usize, main_height);

        // 3. Collect all windows with their geometry from the layout tree
        let window_rects = app.layout.collect_windows(full_rect);

        // 4. Render each window to the back buffer
        // 4. Render each window to the back buffer
        // Collect updates to apply after rendering (to avoid borrow issues)
        let mut width_updates: Vec<(crate::core::id::WindowId, usize)> = Vec::new();

        for (window_id, rect) in &window_rects {
            // Mark this window's rect as dirty for incremental rendering
            self.dirty_tracker.mark_rect(rect);

            if let Some(window) = app.windows.get(window_id) {
                if let Some(buffer) = app.buffers.get_mut(&window.buffer_id) {
                    let is_active = *window_id == app.active_window;

                    // Render Content, Scrollbars, etc.
                    let visible_max_width = crate::terminal::renderers::WindowRenderer::render(
                        buffer,
                        window,
                        rect,
                        &mut self.back_buffer,
                        &self.theme,
                        &mut self.syntax_highlighter,
                        &self.spell_checker,
                        &self.dirty_tracker,
                        is_active,
                        self.show_line_numbers,
                        app.diff_state.as_ref(),
                        app.terminal_host.as_ref(),
                    );

                    // Track width update for after render loop
                    if visible_max_width > window.cached_content_width {
                        width_updates.push((*window_id, visible_max_width));
                    }

                    // Render Status Line
                    crate::terminal::renderers::StatusRenderer::render(
                        &mut self.back_buffer,
                        window,
                        buffer,
                        rect,
                        &self.theme,
                        is_active,
                        self.prompt_state.as_ref(),
                        &self.message,
                        &self.key_sequence,
                    );
                }
            }
        }

        // Apply cached content width updates (only grows, never shrinks to keep scrollbar stable)
        for (window_id, new_width) in width_updates {
            if let Some(window) = app.windows.get_mut(&window_id) {
                window.cached_content_width = window.cached_content_width.max(new_width);
            }
        }

        // Global status bar removed - prompts/messages/key_sequence are now in per-window status

        // 5. Position cursor in active window and style cursor cell
        if let Some(window) = app.windows.get(&app.active_window) {
            for (window_id, rect) in &window_rects {
                if *window_id == app.active_window {
                    // Calculate gutter width to offset cursor properly
                    let gutter_width: usize = if self.show_line_numbers {
                        if let Some(buffer) = app.buffers.get(&window.buffer_id) {
                            let text_height = rect.height.saturating_sub(1);
                            let max_line = window.scroll_offset + text_height;
                            let max_line_num = buffer.line_count().min(max_line).max(1);
                            format!("{}", max_line_num).len() + 1 // digits + border
                        } else {
                            3 // fallback: "1│"
                        }
                    } else {
                        0
                    };

                    let screen_y = rect.y + (window.cursor_y.saturating_sub(window.scroll_offset));
                    let screen_x = rect.x + gutter_width + window.visual_cursor_x;

                    let max_y = rect.y + rect.height.saturating_sub(2);
                    let max_x = rect.x + rect.width.saturating_sub(1);

                    let final_y = screen_y.min(max_y);
                    let final_x = screen_x.min(max_x);

                    // Style cursor cell with cursor_bg/cursor_fg from theme
                    if let Some(cell) = self.back_buffer.get_cell_mut(final_x, final_y) {
                        cell.fg = self.theme.cursor_fg().clone().into();
                        cell.bg = self.theme.cursor_bg().clone().into();
                    }

                    // Store cursor position for GUI access
                    self.cursor_pos = Some((final_x, final_y));
                    break;
                }
            }
        }

        // 6. Validate buffer synchronization before swap
        self.validate_buffer_synchronization()?;

        // NOTE: Swap is now handled by the caller (main.rs) after diffing
        // self.swap_buffers_with_validation();
        self.dirty = false; // Clear dirty flag after successful render

        // NOTE: dirty_tracker is cleared in swap_buffers(), not here, so that
        // render_display_to_terminal can still check dirty state

        Ok(())
    }

    // =========================================================================
    // MENU BAR HANDLING
    // =========================================================================

    /// Check if menu is open (focus is on menu)
    pub fn is_menu_open(&self) -> bool {
        self.menu_bar.is_open()
    }

    /// Toggle menu focus (F10)
    pub fn toggle_menu_focus(&mut self) {
        if self.menu_bar.is_open() {
            self.menu_bar.close();
        } else {
            self.menu_bar.open_menu(0);
        }
        self.dirty = true;
    }

    /// Close menu if open
    pub fn close_menu(&mut self) {
        if self.menu_bar.is_open() {
            self.menu_bar.close();
            self.dirty = true;
        }
    }

    /// Handle up arrow in menu
    pub fn menu_handle_up(&mut self) {
        if let Some(menu) = self.menu_bar.active() {
            menu.select_prev();
            self.dirty = true;
        }
    }

    /// Handle down arrow in menu
    pub fn menu_handle_down(&mut self) {
        if let Some(menu) = self.menu_bar.active() {
            menu.select_next();
            self.dirty = true;
        }
    }

    /// Handle left arrow in menu
    pub fn menu_handle_left(&mut self) {
        self.menu_bar.prev_menu();
        self.dirty = true;
    }

    /// Handle right arrow in menu
    pub fn menu_handle_right(&mut self) {
        self.menu_bar.next_menu();
        self.dirty = true;
    }

    /// Handle enter in menu - returns command to execute
    pub fn menu_handle_enter(&mut self) -> Option<&'static str> {
        let cmd = self.menu_bar.execute_selected();
        self.dirty = true;
        cmd
    }

    /// Handle click on menu - returns command if item clicked
    pub fn menu_handle_click(&mut self, x: usize, y: usize) -> Option<&'static str> {
        if y == 0 && self.show_menu_bar {
            // Click on menu bar - check which menu
            let layout = self.menu_bar.layout();
            for (i, (_, start, end)) in layout.iter().enumerate() {
                if x >= *start && x < *end {
                    if self.menu_bar.active_menu == Some(i) {
                        self.menu_bar.close();
                    } else {
                        self.menu_bar.open_menu(i);
                    }
                    self.dirty = true;
                    return None;
                }
            }
            // Click on bar but not on title
            if self.menu_bar.is_open() {
                self.menu_bar.close();
                self.dirty = true;
            }
            return None;
        }

        // Click in dropdown area
        if self.menu_bar.is_open() {
            #[cfg(debug_assertions)]
            eprintln!("Menu is open, checking dropdown click at x={}, y={}", x, y);

            if let Some(active_idx) = self.menu_bar.active_menu {
                let layout = self.menu_bar.layout();
                if let Some((_, menu_x, _)) = layout.get(active_idx) {
                    let menu = &self.menu_bar.menus[active_idx];
                    let menu_width = menu.render_width();

                    // Check bounds
                    if x >= *menu_x && x < *menu_x + menu_width {
                        let rel_y = y.saturating_sub(1);
                        if rel_y < menu.items.len() {
                            // Check if separator
                            if !matches!(menu.items[rel_y], crate::core::menu::MenuItem::Separator)
                            {
                                // Execute!
                                let cmd = menu.items[rel_y].clone(); // clone item to avoid borrow issues
                                // Wait, we need command string.
                                if let crate::core::menu::MenuItem::Action { command, .. } = cmd {
                                    self.menu_bar.close();
                                    self.dirty = true;
                                    return Some(command);
                                }
                            }
                        }
                    }
                }
            }
            // Click outside menu - close
            self.menu_bar.close();
            self.dirty = true;
            return None;
        }

        None
    }

    /// Handle mouse move to update menu selection
    pub fn menu_handle_move(&mut self, x: usize, y: usize) {
        if !self.show_menu_bar {
            return;
        }

        // Handle top bar scrubbing (switching menus if one is already open)
        if y == 0 && self.menu_bar.is_open() {
            let layout = self.menu_bar.layout();
            for (i, (_, start, end)) in layout.iter().enumerate() {
                if x >= *start && x < *end {
                    if self.menu_bar.active_menu != Some(i) {
                        self.menu_bar.open_menu(i);
                        self.dirty = true;
                    }
                    return;
                }
            }
            return;
        }

        // Handle dropdown item hover
        if let Some(active_idx) = self.menu_bar.active_menu {
            let layout = self.menu_bar.layout();
            if let Some((_, menu_x, _)) = layout.get(active_idx) {
                // Access menu mutably
                let menu = &mut self.menu_bar.menus[active_idx];
                let width = menu.render_width();

                if x >= *menu_x && x < *menu_x + width {
                    if y >= 1 {
                        let item_idx = y - 1;
                        if item_idx < menu.items.len() {
                            // Only select if not separator
                            if !matches!(
                                menu.items[item_idx],
                                crate::core::menu::MenuItem::Separator
                            ) {
                                if menu.selected != Some(item_idx) {
                                    menu.selected = Some(item_idx);
                                    self.dirty = true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Get menu bar height (1 if visible, 0 if hidden)
    pub fn menu_bar_height(&self) -> usize {
        if self.show_menu_bar {
            crate::core::menu::MENU_BAR_HEIGHT_CELLS
        } else {
            0
        }
    }

    /// Render menu bar to the back buffer (works for both TUI and GUI)
    pub fn render_menu_bar(&mut self, menu_bar: &crate::core::menu::MenuBar, show: bool) {
        crate::terminal::renderers::MenuRenderer::render(
            &mut self.back_buffer,
            menu_bar,
            &self.theme,
            self.terminal_size.0 as usize,
            self.terminal_size.1 as usize,
            show,
        );
    }

    /// Validate buffer synchronization before swap
    fn validate_buffer_synchronization(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Check that both buffers have the same dimensions
        if self.front_buffer.width != self.back_buffer.width
            || self.front_buffer.height != self.back_buffer.height
        {
            return Err("Buffer dimensions mismatch during synchronization".into());
        }

        // Check that buffer dimensions match terminal size
        if self.front_buffer.width != self.terminal_size.0
            || self.front_buffer.height != self.terminal_size.1
        {
            return Err("Front buffer dimensions don't match terminal size".into());
        }

        if self.back_buffer.width != self.terminal_size.0
            || self.back_buffer.height != self.terminal_size.1
        {
            return Err("Back buffer dimensions don't match terminal size".into());
        }

        // Check for buffer consistency (no invalid cells)
        for (i, cell) in self.back_buffer.cells.iter().enumerate() {
            if cell.ch == '\0' {
                return Err(
                    format!("Invalid null character found in back buffer at index {}", i).into(),
                );
            }
        }

        Ok(())
    }

    /// Swap buffers with validation and proper synchronization
    pub fn swap_buffers(&mut self) {
        self.swap_buffers_with_validation();
        self.dirty = false;
        // Clear dirty tracker AFTER terminal output is complete
        self.dirty_tracker.clear();
    }

    /// Internal swap implementation
    fn swap_buffers_with_validation(&mut self) {
        // Ensure both buffers are the same size before swap
        if self.front_buffer.width != self.back_buffer.width
            || self.front_buffer.height != self.back_buffer.height
        {
            // Resize front buffer to match back buffer if needed
            self.front_buffer
                .resize(self.back_buffer.width, self.back_buffer.height);
        }

        // Perform the swap using mem::swap for atomic operation
        std::mem::swap(&mut self.front_buffer, &mut self.back_buffer);

        // After swap, ensure the back buffer is ready for next frame
        self.back_buffer.clear();
    }
}

// Public accessors for ScreenBuffer - used by display rendering
impl ScreenBuffer {
    /// Get mutable reference to the cell at the given position
    pub fn get_cell_mut(&mut self, x: usize, y: usize) -> Option<&mut Cell> {
        if x < self.width as usize && y < self.height as usize {
            let idx = (y * self.width as usize) + x;
            Some(&mut self.cells[idx])
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_new() {
        // This test may fail in CI without a terminal, so we just check construction
        let config = crate::config::Config::default();
        let result = Display::new_terminal(&config);
        // Don't assert success since terminal may not be available
        let _ = result;
    }

    #[test]
    fn test_truecolor_conversion() {
        // Test RGB to TrueColor (24-bit) conversion
        let rgb_color = Color::Rgb { r: 255, g: 0, b: 0 }; // Pure red
        let fg_code = rgb_color.to_ansi_fg_code();
        assert_eq!(
            fg_code, "38;2;255;0;0",
            "RGB color should use TrueColor format"
        );

        let rgb_color = Color::Rgb { r: 0, g: 255, b: 0 }; // Pure green
        let fg_code = rgb_color.to_ansi_fg_code();
        assert_eq!(
            fg_code, "38;2;0;255;0",
            "RGB color should use TrueColor format"
        );

        let rgb_color = Color::Rgb { r: 0, g: 0, b: 255 }; // Pure blue
        let fg_code = rgb_color.to_ansi_fg_code();
        assert_eq!(
            fg_code, "38;2;0;0;255",
            "RGB color should use TrueColor format"
        );

        // Test background color too
        let rgb_color = Color::Rgb {
            r: 128,
            g: 64,
            b: 32,
        };
        let bg_code = rgb_color.to_ansi_bg_code();
        assert_eq!(
            bg_code, "48;2;128;64;32",
            "RGB background should use TrueColor format"
        );
    }

    #[test]
    fn test_buffer_synchronization() {
        // Test buffer synchronization validation
        let config = crate::config::Config::default();
        let mut display = Display::new(80, 24, &config);

        // Test that buffers start with same dimensions
        assert_eq!(display.front_buffer.width, display.back_buffer.width);
        assert_eq!(display.front_buffer.height, display.back_buffer.height);

        // Test validation passes with synchronized buffers
        let validation_result = display.validate_buffer_synchronization();
        assert!(
            validation_result.is_ok(),
            "Buffer synchronization should be valid initially"
        );

        // Test resize maintains synchronization
        display.update_size(100, 30);
        assert_eq!(display.front_buffer.width, 100);
        assert_eq!(display.front_buffer.height, 30);
        assert_eq!(display.back_buffer.width, 100);
        assert_eq!(display.back_buffer.height, 30);

        let validation_result = display.validate_buffer_synchronization();
        assert!(
            validation_result.is_ok(),
            "Buffer synchronization should be valid after resize"
        );
    }

    #[test]
    fn test_buffer_swap_with_validation() {
        // Test buffer swap with validation
        let config = crate::config::Config::default();
        let mut display = Display::new(80, 24, &config);

        // Set some content in back buffer
        display.back_buffer.set(
            0,
            0,
            Cell::new(
                'A',
                Color::Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                },
                Color::Rgb { r: 0, g: 0, b: 0 },
            ),
        );

        // Perform swap
        display.swap_buffers_with_validation();

        // Verify content was swapped
        if let Some(cell) = display.front_buffer.get(0, 0) {
            assert_eq!(cell.ch, 'A', "Content should be swapped to front buffer");
        } else {
            panic!("Cell should exist after swap");
        }

        // Verify back buffer was cleared
        if let Some(cell) = display.back_buffer.get(0, 0) {
            assert_eq!(cell.ch, ' ', "Back buffer should be cleared after swap");
        }
    }

    #[test]
    fn test_resize_clears_buffers() {
        // Test that resize clears buffers to avoid visual artifacts
        // (full redraw will repopulate them)
        let config = crate::config::Config::default();
        let mut display = Display::new(80, 24, &config);

        // Set some content in both buffers
        display.front_buffer.set(
            10,
            5,
            Cell::new(
                'X',
                Color::Rgb { r: 255, g: 0, b: 0 },
                Color::Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                },
            ),
        );
        display.back_buffer.set(
            10,
            5,
            Cell::new(
                'Y',
                Color::Rgb { r: 0, g: 0, b: 255 },
                Color::Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                },
            ),
        );

        // Resize to larger dimensions
        display.update_size(100, 30);

        // Verify buffers were cleared (not preserved) to avoid artifacts
        if let Some(cell) = display.front_buffer.get(10, 5) {
            assert_eq!(cell.ch, ' ', "Front buffer should be cleared during resize");
        }

        if let Some(cell) = display.back_buffer.get(10, 5) {
            assert_eq!(cell.ch, ' ', "Back buffer should be cleared during resize");
        }

        // Verify needs_full_redraw is set
        assert!(
            display.needs_full_redraw,
            "Full redraw should be triggered after resize"
        );
    }

    #[test]
    fn test_invalid_resize_handling() {
        // Test that invalid resize dimensions are handled gracefully
        let config = crate::config::Config::default();
        let mut display = Display::new(80, 24, &config);

        // Try to resize with invalid dimensions (should be ignored)
        display.update_size(0, 0);

        // Verify dimensions weren't changed
        assert_eq!(
            display.terminal_size,
            (80, 24),
            "Invalid resize should be ignored"
        );
    }

    #[test]
    fn test_color_conversions() {
        // Test color conversions with available variants
        let colors = vec![
            Color::Reset,
            Color::Rgb { r: 0, g: 0, b: 0 },
            Color::Rgb { r: 255, g: 0, b: 0 },
            Color::Rgb { r: 0, g: 255, b: 0 },
            Color::Rgb {
                r: 255,
                g: 255,
                b: 0,
            },
            Color::Rgb { r: 0, g: 0, b: 255 },
            Color::Rgb {
                r: 255,
                g: 0,
                b: 255,
            },
            Color::Rgb {
                r: 0,
                g: 255,
                b: 255,
            },
            Color::Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            Color::Rgb {
                r: 128,
                g: 128,
                b: 128,
            },
        ];

        for color in colors {
            let fg_code = color.to_ansi_fg_code();
            let bg_code = color.to_ansi_bg_code();

            // All color codes should be non-empty strings
            assert!(
                !fg_code.is_empty(),
                "Foreground color code should not be empty"
            );
            assert!(
                !bg_code.is_empty(),
                "Background color code should not be empty"
            );
        }
    }

    #[test]
    fn test_new_color_methods() {
        // Test to_rgba_f32 method
        let colors = vec![
            (Color::Reset, [0.0, 0.0, 0.0, 1.0]),
            (Color::Black, [0.0, 0.0, 0.0, 1.0]),
            (Color::Red, [1.0, 0.0, 0.0, 1.0]),
            (Color::Green, [0.0, 1.0, 0.0, 1.0]),
            (Color::Yellow, [1.0, 1.0, 0.0, 1.0]),
            (Color::Blue, [0.0, 0.0, 1.0, 1.0]),
            (Color::Magenta, [1.0, 0.0, 1.0, 1.0]),
            (Color::Cyan, [0.0, 1.0, 1.0, 1.0]),
            (Color::White, [1.0, 1.0, 1.0, 1.0]),
            (Color::BrightBlack, [0.5, 0.5, 0.5, 1.0]),
            (Color::BrightRed, [1.0, 0.5, 0.5, 1.0]),
            (Color::BrightGreen, [0.5, 1.0, 0.5, 1.0]),
            (Color::BrightYellow, [1.0, 1.0, 0.5, 1.0]),
            (Color::BrightBlue, [0.5, 0.5, 1.0, 1.0]),
            (Color::BrightMagenta, [1.0, 0.5, 1.0, 1.0]),
            (Color::BrightCyan, [0.5, 1.0, 1.0, 1.0]),
            (Color::BrightWhite, [1.0, 1.0, 1.0, 1.0]),
            (
                Color::Rgb {
                    r: 128,
                    g: 64,
                    b: 32,
                },
                [128.0 / 255.0, 64.0 / 255.0, 32.0 / 255.0, 1.0],
            ),
        ];

        for (color, expected) in colors {
            let rgba = color.to_rgba_f32();
            assert_eq!(rgba, expected, "to_rgba_f32 failed for {:?}", color);
        }
    }
}
