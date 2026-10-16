//! Central editor state: buffers, windows, and global configuration.
//!
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
    /// Vim modal engine state (N-268). Only active when INPUT_MODEL is Modal;
    /// the struct is inert otherwise.
    pub modal_engine: crate::core::modal::ModalEngine,
    /// Standard-mode clipboard register, distinct from the emacs kill-ring.
    /// Vim registers are likewise separate; each input model's clipboard
    /// semantics live here, not in the kill-ring.
    pub clipboard: Option<String>,
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
    pub current_macro: Vec<(crate::core::dispatcher::MacroStep, usize)>, // (step, count)
    pub last_macro: Vec<(crate::core::dispatcher::MacroStep, usize)>,
    /// True while a macro is being replayed; nested execute-macro is rejected.
    pub executing_macro: bool,
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
    /// Snippet manager
    pub snippet_manager: crate::core::snippets::SnippetManager,
    /// Universal argument (C-u prefix) - Some(n) means next command runs n times
    pub universal_argument: Option<usize>,
    /// Status/error message to display to user
    pub message: Option<String>,
    /// Active diff session
    pub diff_state: Option<crate::core::commands::diff::DiffNavState>,
    /// Terminal host session
    pub terminal_host: Option<TerminalHost>,
    /// Last yank byte position (for yank-pop)
    pub last_yank_pos: Option<usize>,
    /// Last yank length in bytes (for yank-pop)
    pub last_yank_len: usize,
    /// Was the last command a yank? (for yank-pop chaining)
    pub last_command_was_yank: bool,
    /// Accessibility cue queue: the core emits typed cue events here;
    /// each shell drains it once per frame and maps the cues to its
    /// senses (bell, flash, earcon). See [`crate::core::cue`].
    pub cue_bus: crate::core::cue::CueBus,
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
            modal_engine: crate::core::modal::ModalEngine::new(),
            clipboard: None,
            next_buffer_id: 1,
            next_window_id: 1,
            last_command_was_kill: false,
            layout: LayoutNode::new_leaf(window_id),
            focus_manager: FocusManager::new(),
            is_recording_macro: false,
            current_macro: Vec::new(),
            last_macro: Vec::new(),
            executing_macro: false,
            last_search_query: None,
            default_tab_width: initial_tab_width,
            completion_request_timer: None,
            active_completion_request_id: None,
            calculator: crate::core::calculator::Calculator::new(),
            word_completer: crate::core::completion::WordCompleter::new(),
            snippet_manager: crate::core::snippets::SnippetManager::new(),
            universal_argument: None,
            message: None,
            diff_state: None,
            terminal_host: None,
            last_yank_pos: None,
            last_yank_len: 0,
            last_command_was_yank: false,
            cue_bus: crate::core::cue::CueBus::new(
                crate::config::accessibility::CUES_ENABLED,
            ),
        }
    }

    /// Initialize an EditorApp with the compile-time config and initial files.
    ///
    /// This consolidates the duplicated setup code from run_terminal_mode
    /// and run_gui_mode: applying tab width from the typed config and loading
    /// the initial files.
    pub fn initialize(files: &[std::path::PathBuf]) -> Self {
        let mut app = Self::new();

        // Apply tab width from the typed config.
        app.default_tab_width = crate::config::editor::TAB_WIDTH;
        if let Some(window) = app.windows.get_mut(&WindowId(0)) {
            window.tab_width = crate::config::editor::TAB_WIDTH;
        }

        // Load initial files. A file that fails to open never becomes a
        // silent empty buffer: undecodable bytes are opened through
        // encoding detection (the detection report lands in the message
        // line), and hard failures (IO, invalid path) are reported in the
        // message line. The backend is shared by the TUI, the GUI, and the
        // web server, so all three behave identically.
        for file_path in files.iter().take(1) {
            match crate::core::encoding::load_file_with_encoding(file_path) {
                Ok(loaded) => {
                    let buffer_id = app.alloc_buffer_id();
                    app.buffers.insert(buffer_id, loaded.buffer);
                    if let Some(window) = app.windows.get_mut(&WindowId(0)) {
                        window.buffer_id = buffer_id;
                    }
                    if loaded.had_errors || loaded.byte_exact {
                        let detected = loaded.detected;
                        app.message = Some(format!(
                            "Opened {} as {} ({:?}, {:?} confidence){}; original bytes preserved, saving unmodified writes them back verbatim",
                            file_path.display(),
                            detected.encoding.label(),
                            detected.method,
                            detected.confidence,
                            if loaded.had_errors {
                                "; malformed sequences replaced with U+FFFD"
                            } else {
                                ""
                            },
                        ));
                    }
                }
                Err(e) => {
                    app.message = Some(format!("{}: {e}", file_path.display()));
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

    /// Load a file into a new buffer, byte-safe.
    ///
    /// Valid UTF-8 loads exactly as before. Anything else goes through
    /// encoding detection: the buffer holds the decoded text for display and
    /// editing, keeps the original bytes for byte-exact save while
    /// unmodified, and the user is told what happened via the message line
    /// (reported, never silent). I/O and path failures surface as typed
    /// [`LoadError`]s.
    pub fn load_file(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<BufferId, crate::core::buffer::LoadError> {
        let loaded = crate::core::encoding::load_file_with_encoding(path.as_ref())?;
        if loaded.had_errors || loaded.detected.encoding != crate::core::encoding::Encoding::Utf8
        {
            self.message = Some(format!(
                "{}: opened as {}{}",
                path.as_ref().display(),
                loaded.detected.encoding.label(),
                if loaded.had_errors {
                    "; invalid sequences shown as U+FFFD, original bytes kept for byte-exact save"
                } else {
                    ""
                },
            ));
        }
        let buffer_id = self.alloc_buffer_id();
        self.buffers.insert(buffer_id, loaded.buffer);

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

    /// Reconcile the active window against its buffer after any
    /// user-driven mutation.
    ///
    /// Cursor and mark positions are plain coordinates; a buffer edit
    /// can invalidate them (a shrink edit leaves the cursor past the
    /// line end; a deleted line orphans the mark). Every command and
    /// prompt action flows through a seam that calls this, so a window
    /// is always valid between user actions by construction — no
    /// command needs its own guard, and no reader needs a fallback.
    pub fn reconcile_active_window(&mut self) {
        let active = self.active_window;
        let buffer_id = match self.windows.get(&active) {
            Some(w) => w.buffer_id,
            None => return,
        };
        // Disjoint field borrows: the buffer is read, the window is
        // written; no command needs to do this itself.
        let Some(buffer) = self.buffers.get(&buffer_id) else {
            return;
        };
        let Some(window) = self.windows.get_mut(&active) else {
            return;
        };
        window.ensure_cursor_valid(buffer);
        window.ensure_mark_valid(buffer);
        window.update_visual_cursor(buffer);
    }

    /// Per-window model inputs for the unified geometry computation.
    /// Every consumer (render, resize, mouse hit-testing) builds its
    /// [`crate::core::geometry::FrameLayout`] from these, so the frame
    /// geometry can never diverge between call sites.
    pub fn geom_inputs(&self) -> Vec<crate::core::geometry::WindowGeomInput> {
        self.windows
            .iter()
            .map(|(id, window)| {
                let line_count = self
                    .buffers
                    .get(&window.buffer_id)
                    .map(|b| b.line_count())
                    .unwrap_or(0);
                crate::core::geometry::WindowGeomInput {
                    window: *id,
                    scroll_offset: window.scroll_offset,
                    scroll_x: window.scroll_x,
                    cursor_y: window.cursor_y,
                    visual_cursor_x: window.visual_cursor_x,
                    line_count,
                    content_width: window.cached_content_width,
                }
            })
            .collect()
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

    /// Delete the buffer shown in the active window.
    ///
    /// Every window viewing the buffer is closed through the layout (the
    /// same path as [`Self::delete_window`]), so the layout tree, the
    /// window map, and the active window stay consistent. Two degenerate
    /// cases are refused or repaired instead of corrupting state:
    /// - the last buffer cannot be deleted (the app always has a buffer);
    /// - if every window views the deleted buffer, the active window is
    ///   kept and repointed at another buffer with a reset view, exactly
    ///   like switching buffers.
    pub fn delete_buffer(&mut self) -> Result<(), &'static str> {
        if self.buffers.len() <= 1 {
            return Err("Cannot delete the last buffer");
        }
        let buffer_id = match self.windows.get(&self.active_window) {
            Some(w) => w.buffer_id,
            None => return Err("No active window"),
        };
        self.buffers.remove(&buffer_id);

        let viewing: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, w)| w.buffer_id == buffer_id)
            .map(|(id, _)| *id)
            .collect();
        // The last window is never closed: if every window viewed the
        // deleted buffer, keep the active one and repoint it.
        let keep_active = viewing.len() == self.windows.len();
        for win_id in viewing {
            if keep_active && win_id == self.active_window {
                continue;
            }
            let next_focus = self.layout.remove_window(win_id);
            self.windows.remove(&win_id);
            if self.active_window == win_id {
                self.active_window = next_focus
                    .filter(|id| self.windows.contains_key(id))
                    .or_else(|| self.windows.keys().next().copied())
                    .expect("a window always remains");
            }
        }
        if keep_active
            && let Some(window) = self.windows.get_mut(&self.active_window)
        {
            // Another buffer exists (the last-buffer case returned above).
            // BufferId is Copy: dereference rather than clone.
            let other = *self
                .buffers
                .keys()
                .next()
                .expect("a buffer always remains");
            window.buffer_id = other;
            window.cursor_x = 0;
            window.cursor_y = 0;
            window.scroll_offset = 0;
        }
        Ok(())
    }

    /// Delete the current window
    pub fn delete_window(&mut self) {        // Cannot delete the last window
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

    /// Enlarge the active window's pane against its split sibling.
    pub fn grow_window(&mut self) {
        let active = self.active_window;
        self.layout
            .adjust_split(active, crate::core::layout::SPLIT_ADJUST_STEP);
    }

    /// Shrink the active window's pane in favor of its split sibling.
    pub fn shrink_window(&mut self) {
        let active = self.active_window;
        self.layout
            .adjust_split(active, -crate::core::layout::SPLIT_ADJUST_STEP);
    }

    /// Move cursor to specific byte offset
    pub fn goto_byte(&mut self, byte_offset: usize) {
        if let Some(window) = self.windows.get_mut(&self.active_window)
            && let Some(buffer) = self.buffers.get(&window.buffer_id) {
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

    /// Get the word under the cursor in the active buffer.
    pub fn get_word_under_cursor(&self) -> Option<String> {
        let (window, buffer) = (self.active_window_ref()?, self.active_buffer()?);
        let line_text = buffer.line(window.cursor_y)?;

        let graphemes: Vec<&str> = crate::core::utf8::GraphemeIterator::new(&line_text).collect();

        if window.cursor_x >= graphemes.len() {
            return None; // Cursor is past the end of the line
        }

        let mut word_start = window.cursor_x;
        while word_start > 0 && crate::core::utf8::is_word_grapheme(graphemes[word_start - 1]) {
            word_start -= 1;
        }

        let mut word_end = window.cursor_x;
        while word_end < graphemes.len() && crate::core::utf8::is_word_grapheme(graphemes[word_end]) {
            word_end += 1;
        }

        if word_start < word_end {
            Some(graphemes[word_start..word_end].join(""))
        } else {
            None
        }
    }
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

    #[test]
    fn initialize_opens_torture_test_with_decoding_report() {
        let app = EditorApp::initialize(&[std::path::PathBuf::from(
            "demos/UTF-8-test.txt",
        )]);
        let buffer = app.active_buffer().expect("a buffer is open");
        assert!(
            buffer.filename.is_some(),
            "the torture-test file must open, not fall back to an unnamed buffer"
        );
        let msg = app.message.as_deref().unwrap_or("");
        assert!(
            msg.contains("windows-1252"),
            "decoding must be reported, got: {msg:?}"
        );
    }

    #[test]
    fn initialize_reports_unopenable_file() {
        let app = EditorApp::initialize(&[std::path::PathBuf::from(
            "demos/does-not-exist-erax-probe.txt",
        )]);
        let msg = app.message.as_deref().unwrap_or("");
        assert!(
            msg.contains("does-not-exist-erax-probe.txt"),
            "the failure must name the file, got: {msg:?}"
        );
    }

    #[test]
    fn initialize_clean_utf8_opens_silently() {
        let app =
            EditorApp::initialize(&[std::path::PathBuf::from("demos/UTF-8-demo.txt")]);
        let buffer = app.active_buffer().expect("a buffer is open");
        assert!(buffer.filename.is_some());
        assert_eq!(app.message, None);
    }
}
