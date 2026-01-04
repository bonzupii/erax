//! This module defines the central `EditorApp` structure, which manages the entire state
//! of the erax editor, including all open buffers, visible windows, and global configurations.

use crate::core::buffer::Buffer;
use crate::core::focus::FocusManager;
use crate::core::id::{BufferId, WindowId};
use crate::core::kill_ring::KillRing;
use crate::core::layout::{LayoutNode, SplitDirection};
use crate::core::terminal_host::TerminalHost;
use crate::core::window::Window;
use std::collections::HashMap;
use std::path::Path;

/// EditorApp: The root application state, orchestrating all buffers, windows, and global editor state.
/// It acts as the central hub for managing the editor's internal logic, user interface elements,
/// and interactions with the operating system or external tools.
pub struct EditorApp {
    /// All buffers
    pub buffers: HashMap<BufferId, Buffer>,
    /// All windows
    pub windows: HashMap<WindowId, Window>,
    /// Active window
    pub active_window: WindowId,
    /// Global kill ring (clipboard)
    pub kill_ring: KillRing,
    /// Next buffer ID to allocate
    pub next_buffer_id: usize,
    /// Next window ID to allocate
    pub next_window_id: usize,
    /// Flag: was the last command a kill command?
    pub last_command_was_kill: bool,
    /// Window layout tree
    pub layout: LayoutNode,
    /// Focus manager for input routing
    pub focus_manager: FocusManager,

    // Macro Recording
    pub is_recording_macro: bool,
    pub current_macro: Vec<(String, usize)>, // (command, count)
    pub last_macro: Vec<(String, usize)>,
    // Search History for "Hunt" commands
    pub last_search_query: Option<String>,
    /// Default tab width for new windows
    pub default_tab_width: usize,
    /// Timer for debouncing completion requests
    pub completion_request_timer: Option<std::time::Instant>,
    /// ID of the active completion request
    pub active_completion_request_id: Option<u64>,
    /// Programming calculator
    pub calculator: crate::core::calculator::Calculator,
    /// Buffer-local word completer
    pub word_completer: crate::core::completion::WordCompleter,
    /// Registry of commands implementing the Command pattern
    pub command_registry: std::collections::HashMap<String, Box<dyn crate::core::command::Command>>,
    /// Snippet manager
    pub snippet_manager: crate::core::snippets::SnippetManager,
    /// Universal argument (C-u prefix) - Some(n) means next command runs n times
    pub universal_argument: Option<usize>,
    /// Status/error message to display to user
    pub message: Option<String>,
    /// Active diff session
    pub diff_state: Option<crate::sed::diff::DiffState>,
    /// Terminal host session
    pub terminal_host: Option<TerminalHost>,
    /// Dispatch depth counter for macro recursion prevention
    pub dispatch_depth: usize,
    /// Last yank byte position (for yank-pop)
    pub last_yank_pos: Option<usize>,
    /// Last yank length in bytes (for yank-pop)
    pub last_yank_len: usize,
    /// Was the last command a yank? (for yank-pop chaining)
    pub last_command_was_yank: bool,
}

impl EditorApp {
    /// Creates a new `EditorApp` instance with a single empty buffer and an associated window.
    ///
    /// This is the entry point for initializing the editor's state. It sets up the default
    /// buffer, window, kill ring, and internal analyzer.
    ///
    /// # Returns
    /// A new `EditorApp` instance.
    pub fn new() -> Self {
        let buffer_id = BufferId(0);
        let window_id = WindowId(0);

        let mut buffers = HashMap::new();
        buffers.insert(buffer_id, Buffer::new());

        let mut windows = HashMap::new();
        let initial_tab_width = 4; // Default value for now
        windows.insert(
            window_id,
            Window::new(window_id, buffer_id, initial_tab_width),
        );

        Self {
            buffers,
            windows,
            active_window: window_id,
            kill_ring: KillRing::new(),
            next_buffer_id: 1,
            next_window_id: 1,
            last_command_was_kill: false,
            layout: LayoutNode::new_leaf(window_id),
            focus_manager: FocusManager::new(),
            is_recording_macro: false,
            current_macro: Vec::new(),
            last_macro: Vec::new(),
            last_search_query: None,
            default_tab_width: initial_tab_width,
            completion_request_timer: None,
            active_completion_request_id: None,
            calculator: crate::core::calculator::Calculator::new(),
            word_completer: crate::core::completion::WordCompleter::new(),
            command_registry: std::collections::HashMap::new(),
            snippet_manager: crate::core::snippets::SnippetManager::new(),
            universal_argument: None,
            message: None,
            diff_state: None,
            terminal_host: None,
            dispatch_depth: 0,
            last_yank_pos: None,
            last_yank_len: 0,
            last_command_was_yank: false,
        }
    }

    /// Initialize an EditorApp with config settings and initial files.
    ///
    /// This consolidates the duplicated setup code from run_terminal_mode
    /// and run_gui_mode: applying tab width, registering commands, and
    /// loading the initial file.
    pub fn initialize_with_config(
        config: &crate::config::Config,
        files: &[std::path::PathBuf],
    ) -> Self {
        use crate::config::ConfigValue;

        let mut app = Self::new();

        // Apply tab width from config
        if let Some(ConfigValue::Int(tab_width)) = config.settings.get("tab_width") {
            app.default_tab_width = *tab_width as usize;
            if let Some(window) = app.windows.get_mut(&WindowId(0)) {
                window.tab_width = *tab_width as usize;
            }
        }

        // Register all commands
        crate::core::commands::register_all(&mut app);

        // Load initial file if provided
        if let Some(file_path) = files.first() {
            if let Ok(buffer_id) = app.load_file(file_path) {
                if let Some(window) = app.windows.get_mut(&WindowId(0)) {
                    window.buffer_id = buffer_id;
                }
            }
        }

        app
    }

    /// Allocate a new buffer ID
    pub fn alloc_buffer_id(&mut self) -> BufferId {
        let id = BufferId(self.next_buffer_id);
        self.next_buffer_id += 1;
        id
    }

    /// Allocate a new window ID
    pub fn alloc_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;
        id
    }

    /// Load a file into a new buffer
    pub fn load_file(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<BufferId, Box<dyn std::error::Error>> {
        let buffer = Buffer::from_file(path)?;
        let buffer_id = self.alloc_buffer_id();

        self.buffers.insert(buffer_id, buffer);

        Ok(buffer_id)
    }

    /// Create a new window viewing a buffer
    pub fn create_window(&mut self, buffer_id: BufferId) -> WindowId {
        let window_id = self.alloc_window_id();
        let window = Window::new(window_id, buffer_id, self.default_tab_width);
        self.windows.insert(window_id, window);
        window_id
    }

    /// Get the active buffer
    pub fn active_buffer(&self) -> Option<&Buffer> {
        self.windows
            .get(&self.active_window)
            .and_then(|w| self.buffers.get(&w.buffer_id))
    }

    /// Get the active buffer (mutable)
    pub fn active_buffer_mut(&mut self) -> Option<&mut Buffer> {
        let buffer_id = self.windows.get(&self.active_window)?.buffer_id;
        self.buffers.get_mut(&buffer_id)
    }

    /// Get the active window
    pub fn active_window_ref(&self) -> Option<&Window> {
        self.windows.get(&self.active_window)
    }

    /// Get the active window (mutable)
    pub fn active_window_mut(&mut self) -> Option<&mut Window> {
        self.windows.get_mut(&self.active_window)
    }

    /// Reset kill command flag (call when a non-kill command is executed)
    pub fn reset_kill_flag(&mut self) {
        self.last_command_was_kill = false;
    }

    /// Set kill command flag (call when a kill command is executed)
    pub fn set_kill_flag(&mut self) {
        self.last_command_was_kill = true;
    }

    /// Split the current window horizontally
    pub fn split_window_horizontally(&mut self) {
        self.split_window_impl(SplitDirection::Horizontal);
    }

    /// Split the current window vertically
    pub fn split_window_vertically(&mut self) {
        self.split_window_impl(SplitDirection::Vertical);
    }

    fn split_window_impl(&mut self, direction: SplitDirection) {
        let active_id = self.active_window;
        let buffer_id = match self.windows.get(&active_id) {
            Some(window) => window.buffer_id,
            None => return, // No active window, can't split
        };

        // Create new window sharing the same buffer
        let new_window_id = self.create_window(buffer_id);

        // Update layout
        if self
            .layout
            .split_window(active_id, new_window_id, direction)
        {
            // Copy cursor position from active window
            if let Some(active) = self.windows.get(&active_id) {
                let cursor_x = active.cursor_x;
                let cursor_y = active.cursor_y;
                let scroll_offset = active.scroll_offset;

                if let Some(new_window) = self.windows.get_mut(&new_window_id) {
                    new_window.cursor_x = cursor_x;
                    new_window.cursor_y = cursor_y;
                    new_window.scroll_offset = scroll_offset;
                }
            }

            self.active_window = new_window_id;
        }
    }

    /// Add a buffer directly (returns its ID)
    pub fn add_buffer(&mut self, buffer: Buffer) -> BufferId {
        let buffer_id = self.alloc_buffer_id();
        self.buffers.insert(buffer_id, buffer);
        buffer_id
    }

    /// Delete the current window
    pub fn delete_window(&mut self) {
        // Cannot delete the last window
        if self.windows.len() <= 1 {
            return;
        }

        let active_id = self.active_window;

        // Remove from layout and get new focus
        if let Some(next_focus) = self.layout.remove_window(active_id) {
            self.active_window = next_focus;
        } else {
            // Fallback if layout didn't return a neighbor (shouldn't happen for valid tree)
            // Just pick another window
            if let Some(&first_id) = self.windows.keys().find(|&&id| id != active_id) {
                self.active_window = first_id;
            }
        }

        // Remove from windows map
        self.windows.remove(&active_id);
    }

    /// Delete all other windows
    pub fn delete_other_windows(&mut self) {
        let active_id = self.active_window;

        // Reset layout to just this window
        self.layout = LayoutNode::new_leaf(active_id);

        // Remove all other windows
        self.windows.retain(|&id, _| id == active_id);
    }

    /// Move focus to the next window
    pub fn next_window(&mut self) {
        let all_ids = self.layout.window_ids();
        if let Some(pos) = all_ids.iter().position(|&id| id == self.active_window) {
            let next_idx = (pos + 1) % all_ids.len();
            self.active_window = all_ids[next_idx];
        }
    }

    /// Move cursor to specific byte offset
    pub fn goto_byte(&mut self, byte_offset: usize) {
        if let Some(window) = self.windows.get_mut(&self.active_window) {
            if let Some(buffer) = self.buffers.get(&window.buffer_id) {
                let line_idx = buffer.byte_to_line(byte_offset);
                if let Some(line_start_byte) = buffer.line_to_byte(line_idx) {
                    let col_byte_offset = byte_offset.saturating_sub(line_start_byte);

                    // Convert byte offset to grapheme index
                    if let Some(line_text) = buffer.line(line_idx) {
                        let graphemes = crate::core::utf8::GraphemeIterator::new(&line_text);
                        let mut current_byte = 0;
                        let mut col_idx = 0;

                        for g in graphemes {
                            if current_byte >= col_byte_offset {
                                break;
                            }
                            current_byte += g.len();
                            col_idx += 1;
                        }

                        window.cursor_y = line_idx;
                        window.cursor_x = col_idx;
                        if let Some(buffer) = self.buffers.get(&window.buffer_id) {
                            window.update_visual_cursor(buffer);
                            window.ensure_cursor_visible(buffer);
                        }
                    }
                }
            }
        }
    }

    /// Get the word under the cursor in the active buffer.
    pub fn get_word_under_cursor(&self) -> Option<String> {
        let (window, buffer) = (self.active_window_ref()?, self.active_buffer()?);
        let line_text = buffer.line(window.cursor_y)?;

        let graphemes: Vec<&str> = crate::core::utf8::GraphemeIterator::new(&line_text).collect();

        if window.cursor_x >= graphemes.len() {
            return None; // Cursor is past the end of the line
        }

        let mut word_start = window.cursor_x;
        while word_start > 0 && is_word_char(graphemes[word_start - 1]) {
            word_start -= 1;
        }

        let mut word_end = window.cursor_x;
        while word_end < graphemes.len() && is_word_char(graphemes[word_end]) {
            word_end += 1;
        }

        if word_start < word_end {
            Some(graphemes[word_start..word_end].join(""))
        } else {
            None
        }
    }
}

// Helper function to determine if a grapheme is part of a word
fn is_word_char(grapheme: &str) -> bool {
    // Simple definition of a word character: alphanumeric or underscore
    grapheme.chars().all(|c| c.is_alphanumeric() || c == '_')
}

impl Default for EditorApp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_new() {
        let app = EditorApp::new();
        assert_eq!(app.buffers.len(), 1);
        assert_eq!(app.windows.len(), 1);
    }

    #[test]
    fn test_app_active_buffer() {
        let app = EditorApp::new();
        assert!(app.active_buffer().is_some());
    }

    #[test]
    fn test_app_create_window() {
        let mut app = EditorApp::new();
        let buffer_id = BufferId(0);
        let window_id = app.create_window(buffer_id);
        assert_eq!(app.windows.len(), 2);
        assert!(app.windows.contains_key(&window_id));
    }
}
