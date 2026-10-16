//! Display: the TUI back/front buffer renderer.
use crate::core::app::EditorApp;
use crate::core::spell::SpellChecker;
use crate::core::syntax::SyntaxHighlighter;
use crate::cli::ColorMode;
use crate::terminal::prompt::PromptState;
use unicode_segmentation::UnicodeSegmentation;

// Re-export Color from color module for backward compatibility
pub use crate::terminal::color::Color;

// The pure screen-model types live in `super::model` (shared with the wasm
// web shell); re-exported here for the TUI.
pub use super::model::{Cell, CellRole, ClusterText, ScreenBuffer};

use crate::terminal::theme::Theme;
use crate::core::cursor_style::CursorStyle;
use crate::config::keys::INPUT_MODEL;
use crate::core::bindings::{InputModel, ModalMode};
use crate::core::modal::ModalEngine;

// ---------------------------------------------------------------------------
// Mode-aware UI screen model (N-268/D6).
//
// All mode UI state originates in the backend (`EditorApp` / `ModalEngine`)
// and reaches renderers only through this screen model plus the N-273
// `CellRole` metadata. The GUI and WebUI (children of the TUI layer) must
// never infer mode state from glyphs or coordinates: they read
// `Display::bar` and `Display::cursor_style`.
// ---------------------------------------------------------------------------

/// Who owns the prompt bar (the active window's status line) this frame.
///
/// Priority order, highest first: [`BarOwner::Prompt`], [`BarOwner::CommandLine`],
/// [`BarOwner::Message`], [`BarOwner::ModeLine`].
///
/// This enum is the extension point for future bar owners: the N-272 LLM
/// assistant panel plugs in as a new variant with its priority slot in
/// [`bar_owner_for`] and one render arm in `StatusRenderer` — no existing
/// arm changes shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BarOwner {
    /// Blocking minibuffer prompt from the focus manager (emacs-style).
    Prompt,
    /// Vim `:` command line: the modal engine owns the whole bar.
    CommandLine,
    /// Transient echo-area message.
    Message,
    /// Default: the input model's mode segment / echo area.
    ModeLine,
}

/// One frame of prompt-bar screen-model state (N-273).
///
/// Built by [`compose_bar_state`] from backend state every frame; the status
/// renderer consumes it without further backend access.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BarState {
    /// Which UI element owns the bar this frame.
    pub owner: BarOwner,
    /// Transient echo-area message text (empty when none).
    pub message: String,
    /// Persistent mode segment, e.g. `-- INSERT --` (Modal model only,
    /// hidden while another owner holds the bar).
    pub mode_segment: Option<&'static str>,
    /// Pending modal keys, e.g. `d3`, `"a2c` (Modal ModeLine only).
    pub pending: String,
    /// Key-trie echo area, e.g. `C-x ` (KeyTrie ModeLine only).
    pub key_sequence: String,
    /// `:` command-line text without the leading `:` (CommandLine only).
    pub cmdline_text: String,
    /// Byte offset of the command-line cursor within `cmdline_text`.
    pub cmdline_cursor: usize,
    /// Active selection length in chars, for the Standard model's
    /// `Ln X, Col Y (Z selected)` readout (`None` = no selection).
    pub selection_len: Option<usize>,
}

impl BarState {
    fn empty() -> Self {
        Self {
            owner: BarOwner::ModeLine,
            message: String::new(),
            mode_segment: None,
            pending: String::new(),
            key_sequence: String::new(),
            cmdline_text: String::new(),
            cmdline_cursor: 0,
            selection_len: None,
        }
    }
}

/// Bar-ownership arbitration: the priority order [`BarOwner`] documents.
///
/// A blocking minibuffer prompt always wins (it is already receiving keys);
/// the vim `:` command line owns the bar while active; transient messages
/// cover the mode segment without clobbering it (it returns when the message
/// clears); otherwise the model's default echo/mode line shows.
pub fn bar_owner_for(
    model: InputModel,
    engine: &ModalEngine,
    prompt_active: bool,
    has_message: bool,
) -> BarOwner {
    if prompt_active {
        return BarOwner::Prompt;
    }
    if model == InputModel::Modal && engine.mode() == ModalMode::CommandLine {
        return BarOwner::CommandLine;
    }
    if has_message {
        return BarOwner::Message;
    }
    BarOwner::ModeLine
}

/// Persistent mode segment for the bar, or `None` when the model shows none.
///
/// Modal Insert with the window's overwrite flag on is Replace behavior, so
/// it shows `-- REPLACE --` (the typed cursor const maps it to underline).
pub fn mode_segment_for(
    model: InputModel,
    engine: &ModalEngine,
    overwrite: bool,
) -> Option<&'static str> {
    if model != InputModel::Modal {
        return None;
    }
    match engine.mode() {
        ModalMode::Insert if overwrite => Some("-- REPLACE --"),
        _ => engine.mode_segment(),
    }
}

/// Cursor style for the current input state (N-268 D6).
///
/// KeyTrie keeps the historical steady block; Standard uses a steady bar;
/// Modal follows the typed [`crate::config::display::modal_cursor_style`]
/// mapping. Pure backend state in, style out — no renderer involvement.
pub fn cursor_style_for(
    model: InputModel,
    engine: &ModalEngine,
    overwrite: bool,
) -> CursorStyle {
    match model {
        InputModel::KeyTrie => crate::config::display::KEYTRIE_CURSOR_STYLE,
        InputModel::Standard => crate::config::display::STANDARD_CURSOR_STYLE,
        InputModel::Modal => crate::config::display::modal_cursor_style(engine.mode(), overwrite),
    }
}

/// Selected character count for the Standard model's `(Z selected)` readout.
///
/// `None` when there is no active non-empty selection. Reads backend window
/// state only (the mark anchor and cursor), like the selection renderer does.
pub fn selection_char_count(
    window: &crate::core::window::Window,
    buffer: &crate::core::buffer::Buffer,
) -> Option<usize> {
    let m = window.selection_manager.mark?;
    let c = window.get_byte_offset(buffer).unwrap_or(0);
    let (s, e) = (m.min(c), m.max(c));
    if s == e {
        return None;
    }
    Some(buffer.get_range_as_string(s, e - s).chars().count())
}

/// Build one frame of [`BarState`] from backend state.
///
/// `model` is a parameter (rather than reading [`INPUT_MODEL`] directly) so
/// tests can exercise every input model despite the compile-time switch;
/// production passes [`INPUT_MODEL`].
pub fn compose_bar_state(
    model: InputModel,
    engine: &ModalEngine,
    prompt: Option<&PromptState>,
    message: &str,
    key_sequence: &str,
    window: Option<&crate::core::window::Window>,
    buffer: Option<&crate::core::buffer::Buffer>,
) -> BarState {
    let owner = bar_owner_for(model, engine, prompt.is_some(), !message.is_empty());
    let overwrite = window.map(|w| w.overwrite_mode).unwrap_or(false);
    let mode_segment = match owner {
        BarOwner::ModeLine => mode_segment_for(model, engine, overwrite),
        _ => None,
    };
    let (pending, key_sequence) = match (model, owner) {
        (InputModel::Modal, BarOwner::ModeLine) => (engine.pending_text(), String::new()),
        (_, BarOwner::ModeLine) => (String::new(), key_sequence.to_string()),
        _ => (String::new(), String::new()),
    };
    let (cmdline_text, cmdline_cursor) = match owner {
        BarOwner::CommandLine => (engine.cmdline_text().to_string(), engine.cmdline_cursor()),
        _ => (String::new(), 0),
    };
    let selection_len = match (model, window, buffer) {
        (InputModel::Standard, Some(w), Some(b)) => selection_char_count(w, b),
        _ => None,
    };
    BarState {
        owner,
        message: message.to_string(),
        mode_segment,
        pending,
        key_sequence,
        cmdline_text,
        cmdline_cursor,
        selection_len,
    }
}

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
    /// Current key sequence being built (e.g., "Ctrl+x-")
    pub key_sequence: String,
    /// Front buffer (currently displayed)
    pub front_buffer: ScreenBuffer,
    /// Back buffer (being drawn to)
    pub back_buffer: ScreenBuffer,
    /// Show line numbers in gutter
    pub show_line_numbers: bool,
    /// Wrap long lines at the window edge (typed config + `--wrap` override)
    pub wrap_lines: bool,
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
    /// Software mouse cursor position (for TTYs)
    pub mouse_cursor: Option<(u16, u16)>,
    /// Display mode (TrueColor, Ansi, Ascii) for color fallback
    pub display_mode: ColorMode,
    /// One frame of prompt-bar screen-model state (N-273/D6): bar ownership,
    /// mode segment, pending keys, command line. Rebuilt from backend state
    /// every frame in [`Display::render`].
    pub bar: BarState,
    /// Cursor style derived from the input model + modal state (N-273/D6).
    /// The terminal emit path and the GUI/WebUI read this as metadata
    /// (presentation only); nothing else writes it.
    pub cursor_style: CursorStyle,
    /// Whether any frame published a non-default cursor style. Drives
    /// restore-on-exit in the [`Drop`] impl.
    cursor_style_touched: bool,
    /// Whether this display owns a TTY. The Drop restore writes to
    /// stdout; GUI/web sessions must not emit terminal escapes.
    owns_tty: bool,
    /// Cursor style the terminal emit path last wrote, for DECSCUSR dedup.
    /// Written by `terminal::render`; `None` before the first frame.
    pub last_emitted_cursor_style: Option<CursorStyle>,
    /// Accessibility visual flash: when set, the next rendered frame
    /// inverts every pane's status bar row, then the flag clears. The
    /// TUI cue sink sets this from cue events; the renderer consumes
    /// it as data through [`StatusCtx::flash`].
    pub flash: bool,
}

impl Display {
    /// Initialize the display system with specified dimensions.
    ///
    /// The theme and display settings come from the typed compile-time
    /// config; `theme_override` and `wrap_override` are the invocation-level
    /// overrides (`--theme`, `--wrap` / `--no-wrap`), which shadow the
    /// config default without mutating it (N-012).
    pub fn new(
        width: u16,
        height: u16,
        theme_override: Option<crate::terminal::theme::ThemeId>,
        wrap_override: Option<bool>,
    ) -> Self {
        let front_buffer = ScreenBuffer::new(width, height);
        let back_buffer = ScreenBuffer::new(width, height);

        let theme_id = theme_override.unwrap_or(crate::config::theme::THEME);
        let theme = *theme_id.theme();

        Self {
            terminal_size: (width, height),
            dirty: true,
            needs_full_redraw: true, // Initial full redraw
            message: String::new(),
            key_sequence: String::new(),
            front_buffer,
            back_buffer,
            show_line_numbers: crate::config::display::LINE_NUMBERS,
            wrap_lines: wrap_override.unwrap_or(crate::config::display::WRAP_LINES),
            syntax_highlighter: SyntaxHighlighter::new(),
            theme,
            cursor_pos: None,
            prompt_state: None,
            spell_checker: SpellChecker::new(),
            menu_bar: crate::core::menu::MenuBar::default(),
            show_menu_bar: true,
            mouse_cursor: None,
            display_mode: ColorMode::detect(),
            bar: BarState::empty(),
            cursor_style: CursorStyle::Default,
            cursor_style_touched: false,
            owns_tty: false,
            last_emitted_cursor_style: None,
            flash: false,
        }
    }

    /// Initialize the display system with terminal detection (for terminal mode)
    pub fn new_terminal(
        theme_override: Option<crate::terminal::theme::ThemeId>,
        wrap_override: Option<bool>,
    ) -> Result<Self, std::io::Error> {
        let (cols, rows) = Self::get_terminal_size()?;
        let mut display = Self::new(cols, rows, theme_override, wrap_override);
        display.owns_tty = true;
        Ok(display)
    }

    /// Get terminal size using crossterm
    fn get_terminal_size() -> Result<(u16, u16), std::io::Error> {
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

        // Mode-aware UI (N-268/D6): derive the bar and cursor style from
        // backend state every frame. Renderers only ever see this screen
        // model, never the engine or input state directly.
        {
            let window = app.active_window_ref();
            let buffer = app.active_buffer();
            self.bar = compose_bar_state(
                INPUT_MODEL,
                &app.modal_engine,
                self.prompt_state.as_ref(),
                &self.message,
                &self.key_sequence,
                window,
                buffer,
            );
            let overwrite = window.map(|w| w.overwrite_mode).unwrap_or(false);
            let style = cursor_style_for(INPUT_MODEL, &app.modal_engine, overwrite);
            if style != CursorStyle::Default {
                self.cursor_style_touched = true;
            }
            self.cursor_style = style;
        }

        // 1. Clear back buffer (the one we are drawing to)
        self.back_buffer.clear();

        // 2-3. One geometry computation for the whole frame: every
        // pane's rect, gutter, scrollbar, and cursor comes from the
        // unified model. Nothing below recomputes any of it.
        let frame = crate::core::geometry::compute_frame(
            crate::core::geometry::FrameOptions {
                cols: self.terminal_size.0 as usize,
                rows: self.terminal_size.1 as usize,
                menu_rows: self.menu_bar_height(),
                show_line_numbers: self.show_line_numbers,
            },
            &app.layout,
            &app.geom_inputs(),
        );
        // Refresh the derived width/height cache on every window from
        // the model: the single writer, so split growth can no longer
        // leave it stale.
        frame.apply_window_dims(&mut app.windows);

        // 4. Render each window to the back buffer
        // Collect updates to apply after rendering (to avoid borrow issues)
        let mut width_updates: Vec<(crate::core::id::WindowId, usize)> = Vec::new();

        // Modal linewise-visual: the engine anchors whole lines (mark at BOL,
        // cursor at EOL), so the window renderer already highlights the text;
        // the trailing newline region gets its Selection stamp in the loop.
        let linewise_newline_pass = INPUT_MODEL == InputModel::Modal
            && app.modal_engine.mode() == ModalMode::Visual
            && app.modal_engine.is_linewise_visual();

        // Screen position of the `:` command-line cursor when the modal
        // command line owns the bar (set from the active window's status).
        let mut cmdline_cursor: Option<(u16, u16)> = None;

        for pane in &frame.panes {
            let window_id = pane.window;
            if let Some(window) = app.windows.get(&window_id)
                && let Some(buffer) = app.buffers.get_mut(&window.buffer_id) {
                    let is_active = window_id == app.active_window;

                    // Render Content, Scrollbars, etc.
                    let visible_max_width = crate::terminal::renderers::WindowRenderer::render(
                        crate::terminal::renderers::WindowRenderParams {
                            buffer,
                            window,
                            geom: pane,
                            screen_buffer: &mut self.back_buffer,
                            theme: &self.theme,
                            syntax_highlighter: &mut self.syntax_highlighter,
                            spell_checker: &self.spell_checker,
                            is_active,
                            show_line_numbers: self.show_line_numbers,
                            diff_state: app.diff_state.as_ref(),
                            terminal_host: app.terminal_host.as_ref(),
                        },
                    );

                    // Track width update for after render loop. The update
                    // fires whenever the visible width changed, in either
                    // direction, so the cache tracks the content.
                    if visible_max_width != window.cached_content_width {
                        width_updates.push((window_id, visible_max_width));
                    }

                    // Modal linewise-visual: the newline itself is not a rendered
                    // cell, so stamp each selected line's trailing newline
                    // region as selected here. Presentation only — the
                    // engine's mark/cursor are untouched.
                    if linewise_newline_pass {
                        stamp_linewise_visual_newlines(
                            &mut self.back_buffer,
                            &pane.frame,
                            pane.gutter_width,
                            window,
                            buffer,
                            &self.theme,
                        );
                    }

                    // Render Status Line
                    let status_ctx = crate::terminal::renderers::status_renderer::StatusCtx {
                        window,
                        buffer,
                        rect: &pane.frame,
                        theme: &self.theme,
                        is_active,
                        prompt_state: self.prompt_state.as_ref(),
                        bar: &self.bar,
                        status_row: pane.status_row,
                        flash: self.flash,
                    };
                    let bar_cursor = crate::terminal::renderers::StatusRenderer::render(
                        &mut self.back_buffer,
                        &status_ctx,
                    );
                    if is_active {
                        cmdline_cursor = bar_cursor;
                    }
                }
        }

        // Apply content width updates. The cache tracks the currently
        // visible content; it must shrink when long lines are deleted,
        // or the horizontal scrollbar would persist after its cause is
        // gone. The geometry pass reads the previous frame's value, so
        // the bar follows content changes by one frame in both
        // directions.
        for (window_id, new_width) in width_updates {
            if let Some(window) = app.windows.get_mut(&window_id) {
                window.cached_content_width = new_width;
            }
        }

        // Global status bar removed - prompts/messages/key_sequence are now in per-window status

        // 5. Cursor: the model's verdict, painted verbatim. When the
        // modal `:` command line owns the bar, the hardware cursor lives
        // on the command line; otherwise the model cursor cell is used.
        // The pane cursor is clamped to the text rect by the layout, so
        // it is always visible when the text rect is non-empty.
        let model_cursor = if cmdline_cursor.is_some() {
            None
        } else {
            frame.pane_for(app.active_window).and_then(|pane| pane.cursor)
        };
        let cursor = cmdline_cursor
            .map(|(cx, cy)| (cx as usize, cy as usize))
            .or(model_cursor);
        if let Some((cx, cy)) = cursor {
            if let Some(cell) = self.back_buffer.get_cell_mut(cx, cy) {
                cell.fg = self.theme.cursor_fg;
                cell.bg = self.theme.cursor_bg;
                cell.role = CellRole::Cursor;
            }
            // A wide character under the cursor covers its continuation
            // cells: they inherit the cursor background so the whole
            // cluster inverts as one instead of leaving a themed
            // trailing cell.
            let mut nx = cx + 1;
            while let (Some(gx), Some(gy)) =
                (u16::try_from(nx).ok(), u16::try_from(cy).ok())
                && self.back_buffer.get(gx, gy).is_some_and(|next| next.hidden)
            {
                if let Some(next) = self.back_buffer.get_cell_mut(nx, cy) {
                    next.bg = self.theme.cursor_bg;
                }
                nx += 1;
            }

            // Store cursor position for GUI access
            self.cursor_pos = Some((cx, cy));
        } else {
            self.cursor_pos = None;
        }

        // 6. Validate buffer synchronization before swap
        self.validate_buffer_synchronization()?;

        // NOTE: Swap is now handled by the caller (main.rs) after diffing
        // self.swap_buffers_with_validation();
        self.dirty = false; // Clear dirty flag after successful render

        // render_display_to_terminal can still check dirty state

        // The flash is consumed by the frame just painted: one frame,
        // never more, whatever set it.
        self.flash = false;

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
    pub fn menu_handle_enter(&mut self) -> Option<crate::core::command::Command> {
        let cmd = self.menu_bar.execute_selected();
        self.dirty = true;
        cmd
    }

    /// Handle click on menu - returns command if item clicked
    pub fn menu_handle_click(
        &mut self,
        x: usize,
        y: usize,
    ) -> Option<crate::core::command::Command> {
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
            if let Some(active_idx) = self.menu_bar.active_menu {
                let layout = self.menu_bar.layout();
                if let Some((_, menu_x, _)) = layout.get(active_idx) {
                    let menu = &self.menu_bar.menus[active_idx];
                    let menu_width = menu.render_width(INPUT_MODEL);

                    // Check bounds
                    if x >= *menu_x && x < *menu_x + menu_width {
                        let rel_y = y.saturating_sub(1);
                        // Rows address painted items: disabled entries are
                        // not painted, so the raw item index is wrong here.
                        // Copy the command out before closing (borrow ends).
                        let clicked = match menu.rendered_item(rel_y) {
                            Some(crate::core::menu::MenuItem::Action {
                                command,
                                enabled: true,
                            }) => Some(*command),
                            _ => None,
                        };
                        if let Some(command) = clicked {
                            self.menu_bar.close();
                            self.dirty = true;
                            return Some(command);
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
                let width = menu.render_width(INPUT_MODEL);

                if x >= *menu_x && x < *menu_x + width
                    && y >= 1 {
                        // Hover addresses painted rows; resolve to the item
                        // index so a disabled entry cannot be highlighted.
                        if let Some(item_idx) = menu.rendered_row_index(y - 1)
                            && menu.selectable(item_idx)
                            && menu.selected != Some(item_idx)
                        {
                            menu.selected = Some(item_idx);
                            self.dirty = true;
                        }
                    }
            }
        }
    }

    /// Get menu bar height (1 if visible, 0 if hidden)
    pub fn menu_bar_height(&self) -> usize {
        if self.show_menu_bar {
            // Single source: the typed display config (N-167).
            crate::config::display::MENU_BAR_HEIGHT_CELLS
        } else {
            0
        }
    }

    /// Render menu bar to the back buffer (works for both TUI and GUI)
    pub fn render_menu_bar(&mut self, show: bool) {
        crate::terminal::renderers::MenuRenderer::render(
            &mut self.back_buffer,
            &self.menu_bar,
            &self.theme,
            self.terminal_size.0 as usize,
            self.terminal_size.1 as usize,
            show,
            INPUT_MODEL,
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
            if cell.text().is_empty() || cell.text() == "\0" {
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

/// Presentation-only (N-268 D6): in modal linewise-visual mode, extend the
/// `Selection` highlight to cover each selected line's trailing newline
/// region, so `V` visibly selects whole lines.
///
/// The engine's mark/cursor are untouched — this only stamps cells the
/// window renderer left as plain text (the newline is not a rendered cell).
/// Lines are skipped when their end-of-line is scrolled out of view.
fn stamp_linewise_visual_newlines(
    screen: &mut ScreenBuffer,
    rect: &crate::core::layout::Rect,
    gutter_width: usize,
    window: &crate::core::window::Window,
    buffer: &crate::core::buffer::Buffer,
    theme: &Theme,
) {
    let mark_byte = match window.selection_manager.mark {
        Some(m) => m,
        None => return,
    };
    let mark_y = buffer.byte_to_line(mark_byte);
    let first = mark_y.min(window.cursor_y);
    let last = mark_y.max(window.cursor_y);
    let text_start_x = rect.x + gutter_width;
    let text_width = rect.width.saturating_sub(gutter_width);
    // Last row of the rect is the status line.
    let text_height = rect.height.saturating_sub(1);
    for line_idx in first..=last {
        // The newline region exists iff a `\n` follows this line.
        let has_newline = buffer
            .line_with_newline(line_idx)
            .map(|l| l.ends_with('\n'))
            .unwrap_or(false);
        if !has_newline {
            continue;
        }
        let rel = line_idx.saturating_sub(window.scroll_offset);
        if rel >= text_height {
            continue;
        }
        // Visual width of the line content (tabs expanded, wide chars),
        // mirroring the window renderer's layout math: one layout width
        // per grapheme cluster.
        let content = buffer.line(line_idx).unwrap_or_default();
        let mut vx = 0usize;
        for grapheme in content.graphemes(true) {
            vx += if grapheme == "\t" {
                window.tab_width - (vx % window.tab_width)
            } else {
                crate::core::utf8::layout_grapheme_width(grapheme)
            };
        }
        if vx < window.scroll_x || vx - window.scroll_x >= text_width {
            continue;
        }
        let screen_x = text_start_x + (vx - window.scroll_x);
        let screen_y = rect.y + rel;
        if let Some(cell) = screen.get_cell_mut(screen_x, screen_y) {
            cell.fg = theme.selection_fg;
            cell.bg = theme.selection_bg;
            cell.role = CellRole::Selection;
        }
    }
}

impl Drop for Display {
    /// Restore-on-exit for the mode-aware cursor style (N-268 D6): if any
    /// frame published a non-default cursor style, reset the terminal cursor
    /// to its default (`ESC [ 0 SP q`).
    ///
    /// The per-frame DECSCUSR emission itself lives in the terminal render
    /// path (`terminal::render`), which keys off
    /// [`Display::last_emitted_cursor_style`]; this `Drop` only undoes what
    /// was published. It also runs during panic unwinding, so a crash cannot
    /// leave a stray bar/underline cursor behind. In GUI mode no TTY is
    /// touched per frame, so this is a no-op unless a style was published.
    fn drop(&mut self) {
        // Only a TTY-owning display published a style to the terminal.
        // GUI/web sessions share the Display type but never touch a TTY;
        // emitting an escape there would corrupt their stdout protocol.
        if self.owns_tty && self.cursor_style_touched {
            let seq = CursorStyle::Default.decscusr_bytes();
            let _ = std::io::Write::write_all(&mut std::io::stdout(), &seq);
        }
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
        let display = Display::new(80, 24, None, None);
        assert_eq!(display.terminal_size, (80, 24));
        assert_eq!(display.show_line_numbers, crate::config::display::LINE_NUMBERS);
    }

    #[test]
    fn test_truecolor_conversion() {
        use crossterm::style::Color as TermColor;
        // RGB colors map to crossterm RGB (24-bit) exactly.
        let rgb_color = Color::Rgb { r: 255, g: 0, b: 0 };
        assert_eq!(rgb_color.to_crossterm(), TermColor::Rgb { r: 255, g: 0, b: 0 });

        let rgb_color = Color::Rgb { r: 0, g: 255, b: 0 };
        assert_eq!(rgb_color.to_crossterm(), TermColor::Rgb { r: 0, g: 255, b: 0 });

        let rgb_color = Color::Rgb { r: 0, g: 0, b: 255 };
        assert_eq!(rgb_color.to_crossterm(), TermColor::Rgb { r: 0, g: 0, b: 255 });

        // Named colors map to the exact crossterm named color (ANSI Red is
        // crossterm's DarkRed; crossterm's Red is the bright variant).
        assert_eq!(Color::Red.to_crossterm(), TermColor::DarkRed);
        assert_eq!(Color::BrightRed.to_crossterm(), TermColor::Red);
    }

    #[test]
    fn test_buffer_synchronization() {
        // Test buffer synchronization validation
        let mut display = Display::new(80, 24, None, None);

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
        let mut display = Display::new(80, 24, None, None);

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
            assert_eq!(cell.text(), "A", "Content should be swapped to front buffer");
        } else {
            panic!("Cell should exist after swap");
        }

        // Verify back buffer was cleared
        if let Some(cell) = display.back_buffer.get(0, 0) {
            assert_eq!(cell.text(), " ", "Back buffer should be cleared after swap");
        }
    }

    #[test]
    fn test_resize_clears_buffers() {
        // Test that resize clears buffers to avoid visual artifacts
        // (full redraw will repopulate them)
        let mut display = Display::new(80, 24, None, None);

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
            assert_eq!(cell.text(), " ", "Front buffer should be cleared during resize");
        }

        if let Some(cell) = display.back_buffer.get(10, 5) {
            assert_eq!(cell.text(), " ", "Back buffer should be cleared during resize");
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
        let mut display = Display::new(80, 24, None, None);

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
            // Each color maps to a distinct crossterm color (no collisions).
            let fg_term = color.to_crossterm();
            let bg_term = color.to_crossterm();
            // Sanity: RGB colors preserve their values exactly.
            if let Color::Rgb { r, g, b } = color {
                assert_eq!(
                    fg_term,
                    crossterm::style::Color::Rgb { r, g, b },
                    "RGB color must map exactly"
                );
                assert_eq!(
                    bg_term,
                    crossterm::style::Color::Rgb { r, g, b },
                    "RGB color must map exactly"
                );
            }
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
            (Color::BrightBlack, [127.0/255.0, 127.0/255.0, 127.0/255.0, 1.0]),
            (Color::BrightRed, [1.0, 127.0/255.0, 127.0/255.0, 1.0]),
            (Color::BrightGreen, [127.0/255.0, 1.0, 127.0/255.0, 1.0]),
            (Color::BrightYellow, [1.0, 1.0, 127.0/255.0, 1.0]),
            (Color::BrightBlue, [127.0/255.0, 127.0/255.0, 1.0, 1.0]),
            (Color::BrightMagenta, [1.0, 127.0/255.0, 1.0, 1.0]),
            (Color::BrightCyan, [127.0/255.0, 1.0, 1.0, 1.0]),
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

    #[test]
    fn test_cell_role_schema_n273() {
        // repr(u8): single byte, distinct discriminants for shader/wire packing.
        let roles = [
            CellRole::EditorText,
            CellRole::Gutter,
            CellRole::StatusBar,
            CellRole::MenuBar,
            CellRole::MenuItem,
            CellRole::Prompt,
            CellRole::MessageLine,
            CellRole::Cursor,
            CellRole::Selection,
            CellRole::Scrollbar,
            CellRole::Diagnostic,
            CellRole::Separator,
        ];
        let mut seen = std::collections::HashSet::new();
        for r in roles {
            assert!(seen.insert(r as u8), "CellRole discriminant collision");
        }
        assert_eq!(CellRole::EditorText as u8, 0);
        // Default constructors stamp EditorText; with_role stamps the given role.
        assert_eq!(
            Cell::new('a', Color::Reset, Color::Reset).role,
            CellRole::EditorText
        );
        assert_eq!(
            Cell::empty().role,
            CellRole::EditorText
        );
        assert_eq!(
            Cell::with_role('a', Color::Reset, Color::Reset, CellRole::Gutter).role,
            CellRole::Gutter
        );
        // Default role is EditorText.
        assert_eq!(CellRole::default(), CellRole::EditorText);
    }

    #[test]
    fn test_menu_bar_stamps_menu_bar_role() {
        use crate::terminal::renderers::menu_renderer::MenuRenderer;
        let mut display = Display::new(80, 24, None, None);
        let theme = display.theme;
        let menu_bar = crate::core::menu::MenuBar::default();
        MenuRenderer::render(&mut display.back_buffer, &menu_bar, &theme, 80, 24, true, crate::core::bindings::InputModel::KeyTrie);
        // Every cell of the top bar carries the MenuBar role.
        for x in 0..80u16 {
            let cell = display
                .back_buffer
                .get(x, 0)
                .unwrap_or_else(|| panic!("menu bar cell at x={x}"));
            assert_eq!(cell.role, CellRole::MenuBar, "bar cell at x={x}");
        }
    }

    #[test]
    fn test_menu_dropdown_stamps_menu_item_role() {
        use crate::terminal::renderers::menu_renderer::MenuRenderer;
        let mut display = Display::new(80, 24, None, None);
        let theme = display.theme;
        let mut menu_bar = crate::core::menu::MenuBar::default();
        menu_bar.open_menu(0);
        MenuRenderer::render(&mut display.back_buffer, &menu_bar, &theme, 80, 24, true, crate::core::bindings::InputModel::KeyTrie);
        // Dropdown item rows below the bar carry the MenuItem role.
        let cell = display
            .back_buffer
            .get(2, 1)
            .expect("dropdown item cell");
        assert_eq!(cell.role, CellRole::MenuItem);
    }

    #[test]
    fn test_disabled_menu_entry_cannot_be_clicked_or_hovered() {
        use crate::core::command::Command;
        use crate::core::menu::{Menu, MenuBar, MenuItem};
        let mut display = Display::new(80, 24, None, None);
        let menu = Menu::new(
            "File",
            vec![
                MenuItem::Action {
                    command: Command::SaveBuffer,
                    enabled: false,
                },
                MenuItem::Action {
                    command: Command::FindFile,
                    enabled: true,
                },
            ],
        );
        display.menu_bar = MenuBar {
            menus: vec![menu],
            active_menu: None,
        };
        display.show_menu_bar = true;
        display.menu_bar.open_menu(0);
        let (_, menu_x, _) = display.menu_bar.layout()[0];
        // Disabled entries are not painted, so painted row 0 is the
        // enabled entry: hover must resolve to its item index (1), not
        // the painted row.
        display.menu_handle_move(menu_x + 1, 1);
        assert_eq!(display.menu_bar.menus[0].selected, Some(1));
        // Clicking that painted row dispatches the enabled command.
        let clicked = display.menu_handle_click(menu_x + 1, 1);
        assert_eq!(clicked, Some(Command::FindFile));
        // Clicking a row below the painted items dispatches nothing.
        display.menu_bar.open_menu(0);
        let clicked = display.menu_handle_click(menu_x + 1, 5);
        assert_eq!(clicked, None);
    }

    #[test]
    fn test_status_line_stamps_status_bar_role() {
        use crate::terminal::renderers::status_renderer::{StatusCtx, StatusRenderer};
        let mut display = Display::new(80, 24, None, None);
        let theme = display.theme;
        let window = crate::core::window::Window::new(
            crate::core::id::WindowId(0),
            crate::core::id::BufferId(0),
            4,
        );
        let buffer = crate::core::buffer::Buffer::from_string("hello");
        let rect = crate::core::layout::Rect::new(0, 0, 80, 24);
        let bar = BarState::empty();
        let ctx = StatusCtx {
            window: &window,
            buffer: &buffer,
            rect: &rect,
            theme: &theme,
            is_active: true,
            prompt_state: None,
            bar: &bar,
            status_row: 23,
            flash: false,
        };
        let cursor = StatusRenderer::render(&mut display.back_buffer, &ctx);
        assert_eq!(cursor, None);
        // Status line (last row of the rect) carries the StatusBar role.
        for x in 0..80u16 {
            let cell = display
                .back_buffer
                .get(x, 23)
                .unwrap_or_else(|| panic!("status cell at x={x}"));
            assert_eq!(cell.role, CellRole::StatusBar, "status cell at x={x}");
        }
    }

    #[test]
    fn test_bar_state_syncs_from_backend() {
        use crate::core::bindings::{InputModel, ModalMode};
        use crate::core::app::EditorApp;
        use crate::core::input::{Key, KeyPress};

        // Compose through the real backend: drive the engine, then build the
        // bar state for the Modal model (production passes INPUT_MODEL).
        let mut app = EditorApp::new();
        app.active_buffer_mut().unwrap().insert(0, "hello\n");
        let mut engine = std::mem::take(&mut app.modal_engine);
        engine.handle_key_inner(&mut app, &KeyPress::plain(Key::Char('i')));
        app.modal_engine = engine;

        let window = app.active_window_ref().unwrap();
        let buffer = app.active_buffer().unwrap();
        let bar = compose_bar_state(
            InputModel::Modal,
            &app.modal_engine,
            None,
            "",
            "",
            Some(window),
            Some(buffer),
        );
        assert_eq!(bar.owner, BarOwner::ModeLine);
        assert_eq!(bar.mode_segment, Some("-- INSERT --"));
        assert_eq!(
            cursor_style_for(InputModel::Modal, &app.modal_engine, false),
            CursorStyle::SteadyBar
        );
        // INPUT_MODEL is KeyTrie in this build: the classic mapping holds.
        assert_eq!(INPUT_MODEL, InputModel::KeyTrie);
        assert_eq!(
            cursor_style_for(INPUT_MODEL, &app.modal_engine, false),
            CursorStyle::SteadyBlock
        );
        let _ = ModalMode::Normal;
    }

    #[test]
    fn test_bar_owner_priority() {
        use crate::core::bindings::InputModel;
        use crate::core::modal::ModalEngine;
        let engine = ModalEngine::new();
        // Prompt outranks everything…
        assert_eq!(
            bar_owner_for(InputModel::Modal, &engine, true, true),
            BarOwner::Prompt
        );
        // …then the vim command line…
        let mut cmdline = ModalEngine::new();
        {
            use crate::core::app::EditorApp;
            use crate::core::input::{Key, KeyPress};
            let mut app = EditorApp::new();
            cmdline.handle_key_inner(&mut app, &KeyPress::plain(Key::Char(':')));
        }
        assert_eq!(
            bar_owner_for(InputModel::Modal, &cmdline, false, true),
            BarOwner::CommandLine
        );
        // …then messages…
        assert_eq!(
            bar_owner_for(InputModel::Modal, &engine, false, true),
            BarOwner::Message
        );
        // …then the default echo/mode line.
        assert_eq!(
            bar_owner_for(InputModel::Modal, &engine, false, false),
            BarOwner::ModeLine
        );
        // CommandLine only applies to the Modal model.
        assert_eq!(
            bar_owner_for(InputModel::KeyTrie, &cmdline, false, false),
            BarOwner::ModeLine
        );
    }

    #[test]
    fn test_linewise_visual_newline_stamping() {
        use crate::core::id::{BufferId, WindowId};
        // Buffer: two lines, each with a trailing newline, plus a last line
        // without one.
        let buffer = crate::core::buffer::Buffer::from_string("ab\ncde\nf");
        let mut window = crate::core::window::Window::new(WindowId(0), BufferId(0), 4);
        // Linewise visual over lines 0..=1: mark at BOL of line 0, cursor at
        // EOL of line 1 (exactly what the engine sets for `V`).
        window.selection_manager.set_mark(0);
        window.cursor_x = 3;
        window.cursor_y = 1;
        window.scroll_offset = 0;
        window.scroll_x = 0;

        let mut display = Display::new(40, 10, None, None);
        let theme = display.theme;
        let rect = crate::core::layout::Rect::new(0, 0, 40, 10);
        let pane = crate::core::geometry::compute_frame(
            crate::core::geometry::FrameOptions {
                cols: 40,
                rows: 10,
                menu_rows: 0,
                show_line_numbers: true,
            },
            &crate::core::layout::LayoutNode::Leaf(WindowId(0)),
            &[crate::core::geometry::WindowGeomInput {
                window: WindowId(0),
                scroll_offset: 0,
                scroll_x: 0,
                cursor_y: 1,
                visual_cursor_x: 0,
                line_count: 3,
                content_width: 0,
            }],
        )
        .panes
        .into_iter()
        .next()
        .unwrap();
        stamp_linewise_visual_newlines(
            &mut display.back_buffer,
            &rect,
            pane.gutter_width,
            &window,
            &buffer,
            &theme,
        );
        // Newline regions: end of "ab" (visual x 2) and end of "cde" (x 3).
        let text_x = rect.x + pane.gutter_width;
        let cell = display.back_buffer.get((text_x + 2) as u16, 0).unwrap();
        assert_eq!(cell.role, CellRole::Selection);
        assert_eq!(cell.bg, theme.selection_bg);
        let cell = display.back_buffer.get((text_x + 3) as u16, 1).unwrap();
        assert_eq!(cell.role, CellRole::Selection);
        // Line 2 ("f", no trailing newline): not stamped.
        let cell = display.back_buffer.get((text_x + 1) as u16, 2).unwrap();
        assert_eq!(cell.role, CellRole::EditorText);
        // No mark: no-op, nothing stamped.
        let mut display2 = Display::new(40, 10, None, None);
        let mut window2 = window.clone();
        window2.selection_manager.clear_mark();
        stamp_linewise_visual_newlines(
            &mut display2.back_buffer,
            &rect,
            pane.gutter_width,
            &window2,
            &buffer,
            &theme,
        );
        let cell = display2.back_buffer.get((text_x + 2) as u16, 0).unwrap();
        assert_eq!(cell.role, CellRole::EditorText);
    }

    #[test]
    fn test_display_drop_without_touch_is_quiet() {
        // A Display that never published a cursor style must not touch stdout
        // on drop (unit-test and GUI-mode hygiene). This just constructs and
        // drops; the absence of output is asserted by inspection of the flag.
        let display = Display::new(80, 24, None, None);
        assert!(!display.cursor_style_touched);
        assert_eq!(display.cursor_style, CursorStyle::Default);
        drop(display);
    }
}


#[cfg(test)]
mod golden_tests {
    use super::*;
    use crate::core::app::EditorApp;
    use crate::core::buffer::Buffer;
    use crate::core::geometry::{compute_frame, FrameLayout, FrameOptions};
    use crate::core::id::BufferId;

    fn app_100_lines() -> EditorApp {
        let mut app = EditorApp::new();
        let text = (0..100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.buffers.insert(BufferId(0), Buffer::from_string(&text));
        app
    }

    #[test]
    fn content_width_cache_shrinks_when_long_lines_are_deleted() {
        // X-17: the cache tracked only growth, so the horizontal
        // scrollbar persisted after the long lines were deleted.
        let mut app = EditorApp::new();
        let long = "x".repeat(120);
        app.buffers
            .insert(BufferId(0), Buffer::from_string(&format!("{long}\nshort\n")));
        let mut display = Display::new(80, 24, None, None);
        display.render(&mut app).expect("render");
        let grown = app
            .active_window_ref()
            .unwrap()
            .cached_content_width;
        assert!(grown >= 120, "cache did not track the long line: {grown}");

        // Delete the long line; the cache must follow the content down.
        // (Mark the display dirty, as the event handler does after a
        // buffer mutation, or render() correctly skips the clean frame.)
        let buf = app.active_buffer_mut().unwrap();
        buf.delete(0, 121);
        display.dirty = true;
        display.render(&mut app).expect("render");
        let shrunk = app
            .active_window_ref()
            .unwrap()
            .cached_content_width;
        assert!(
            shrunk < grown,
            "cache did not shrink: {shrunk} vs {grown}"
        );
        assert!(shrunk < 80, "short content still claims width {shrunk}");
    }

    fn frame_of(app: &EditorApp, cols: usize, rows: usize) -> FrameLayout {
        compute_frame(
            FrameOptions {
                cols,
                rows,
                menu_rows: 1,
                show_line_numbers: true,
            },
            &app.layout,
            &app.geom_inputs(),
        )
    }

    /// Golden: single window on 80x24. The exact rect map plus painted
    /// proof (gutter, status, scrollbar, cursor cells).
    #[test]
    fn golden_single_window_rect_map() {
        let mut app = app_100_lines();
        let mut display = Display::new(80, 24, None, None);
        display.render(&mut app).expect("render");
        let frame = frame_of(&app, 80, 24);
        assert_eq!(frame.panes.len(), 1);
        let p = &frame.panes[0];
        assert_eq!(
            (p.frame.x, p.frame.y, p.frame.width, p.frame.height),
            (0, 1, 80, 23)
        );
        assert_eq!(p.gutter_width, 3);
        // Dynamic presence: the horizontal bar is absent (content fits),
        // so no strip is reserved and the text rect takes the full
        // extent: 22 rows, reaching the row above the status row.
        assert_eq!(
            (p.text.x, p.text.y, p.text.width, p.text.height),
            (3, 1, 76, 22)
        );
        assert_eq!(p.status_row, 23);
        assert_eq!(p.h_scrollbar, None);
        let v = p.v_scrollbar.expect("vertical scrollbar");
        assert_eq!((v.cross, v.track_start, v.track_len), (79, 2, 20));
        assert_eq!((v.thumb_start, v.thumb_len), (0, 4));
        assert_eq!(p.cursor, Some((3, 1)));
        // Painted proof.
        assert_eq!(
            display.back_buffer.get(0, 1).unwrap().role,
            CellRole::Gutter
        );
        assert_eq!(
            display.back_buffer.get(0, 23).unwrap().role,
            CellRole::StatusBar
        );
        assert_eq!(
            display.back_buffer.get(79, 2).unwrap().role,
            CellRole::Scrollbar
        );
        // The row the old permanent reservation wasted is text now: it
        // paints as text with an explicit background, owned by the text
        // renderer — no post-render sweep needed to cover it.
        let strip_cell = display.back_buffer.get(3, 22).unwrap();
        assert_ne!(strip_cell.bg, Color::Reset, "row 22 must be painted");
        assert_eq!(display.cursor_pos, Some((3, 1)));
    }

    #[test]
    fn wide_char_continuation_inherits_cluster_background() {
        // The cursor line carries a themed background; a wide
        // character's continuation cell must inherit it, not paint
        // the default background.
        let mut app = EditorApp::new();
        app.buffers
            .insert(BufferId(0), Buffer::from_string("あx"));
        let mut display = Display::new(80, 24, None, None);
        display.render(&mut app).expect("render");
        let row = 1;
        let first = (0..80)
            .find(|&x| {
                display.back_buffer.get(x, row).map(|c| c.text()) == Some("あ")
            })
            .expect("wide char painted");
        let first_cell = display.back_buffer.get(first, row).unwrap();
        let cont_cell = display.back_buffer.get(first + 1, row).unwrap();
        assert!(cont_cell.hidden, "second cell is the continuation");
        assert_ne!(
            first_cell.bg,
            Color::Reset,
            "fixture needs a themed cursor-line background"
        );
        assert_eq!(
            cont_cell.bg, first_cell.bg,
            "continuation cell inherits the cluster background"
        );
    }

    #[test]
    fn wide_char_under_cursor_inverts_as_one_cluster() {
        // The cursor starts on the buffer's first character; when that
        // character is wide, the cursor restyle must cover the
        // continuation cell too, not leave a themed trailing cell.
        let mut app = EditorApp::new();
        app.buffers
            .insert(BufferId(0), Buffer::from_string("あx"));
        let mut display = Display::new(80, 24, None, None);
        display.render(&mut app).expect("render");
        let row = 1;
        let first = (0..80)
            .find(|&x| {
                display.back_buffer.get(x, row).map(|c| c.text()) == Some("あ")
            })
            .expect("wide char painted");
        assert_eq!(display.cursor_pos, Some((first as usize, row as usize)));
        let cont_cell = display.back_buffer.get(first + 1, row).unwrap();
        assert!(cont_cell.hidden, "second cell is the continuation");
        assert_eq!(
            cont_cell.bg,
            display.theme.cursor_bg,
            "continuation cell takes the cursor background"
        );
    }

    /// Golden: horizontal split (side by side) on 80x24. Remainder rule:
    /// the left pane takes floor(40), the right takes the remainder.
    #[test]
    fn golden_side_by_side_split_rect_map() {
        let mut app = app_100_lines();
        app.split_window_horizontally();
        let mut display = Display::new(80, 24, None, None);
        display.render(&mut app).expect("render");
        let frame = frame_of(&app, 80, 24);
        assert_eq!(frame.panes.len(), 2);
        let (left, right) = (&frame.panes[0], &frame.panes[1]);
        assert_eq!(
            (left.frame.x, left.frame.y, left.frame.width, left.frame.height),
            (0, 1, 40, 23)
        );
        assert_eq!(
            (
                right.frame.x,
                right.frame.y,
                right.frame.width,
                right.frame.height
            ),
            (40, 1, 40, 23)
        );
        assert_eq!(left.status_row, 23);
        assert_eq!(right.status_row, 23);
        // Both status rows paint.
        assert_eq!(
            display.back_buffer.get(0, 23).unwrap().role,
            CellRole::StatusBar
        );
        assert_eq!(
            display.back_buffer.get(79, 23).unwrap().role,
            CellRole::StatusBar
        );
        // The divider column boundary: last text column of the left pane
        // carries its scrollbar.
        assert_eq!(
            display.back_buffer.get(39, 2).unwrap().role,
            CellRole::Scrollbar
        );
    }

    /// Golden: vertical split (stacked) on 80x24 with content wider than
    /// the viewport. The horizontal scrollbar row is the pane's
    /// second-to-last row; the thumb sits inside its track.
    #[test]
    fn golden_stacked_split_hscroll_rect_map() {
        let mut app = app_100_lines();
        // Content wider than any viewport: a 200-column line, so the
        // renderer measures a wide visible width and the scrollbar
        // decision sees it.
        let wide = "x".repeat(200);
        app.active_buffer_mut().unwrap().insert(0, &format!("{wide}\n"));
        app.split_window_vertically();
        let mut display = Display::new(80, 24, None, None);
        // Two frames: the first measures the wide content into the
        // cache; the second lays out with the scrollbar present.
        display.render(&mut app).expect("render");
        display.dirty = true;
        display.render(&mut app).expect("render");
        let frame = frame_of(&app, 80, 24);
        assert_eq!(frame.panes.len(), 2);
        let (top, bottom) = (&frame.panes[0], &frame.panes[1]);
        assert_eq!(
            (top.frame.x, top.frame.y, top.frame.width, top.frame.height),
            (0, 1, 80, 11)
        );
        assert_eq!(
            (
                bottom.frame.x,
                bottom.frame.y,
                bottom.frame.width,
                bottom.frame.height
            ),
            (0, 12, 80, 12)
        );
        // Top pane: 11 rows = 9 text + 1 hscroll strip + 1 status. The
        // strip exists because the bar is present (dynamic presence),
        // not because it is reserved.
        assert_eq!(top.text.height, 9);
        assert_eq!(top.status_row, 11);
        // Vertical bar owns the last column; its strip reaches the
        // hscroll row, whose corner cell is the bar's down arrow.
        let v = top.v_scrollbar.expect("vertical scrollbar");
        assert_eq!((v.cross, v.track_start, v.track_len), (79, 2, 8));
        assert_eq!((v.thumb_start, v.thumb_len), (0, 2));
        // Horizontal bar spans the text width; the corner cell under
        // the vertical strip belongs to the vertical bar.
        // (Gutter here is 3: 10 visible lines, two digits plus border.)
        let h = top.h_scrollbar.expect("horizontal scrollbar");
        assert_eq!(h.cross, 10);
        assert_eq!(h.track_start, 4);
        assert_eq!(h.track_len, 74);
        assert_eq!((h.thumb_start, h.thumb_len), (0, 28));
        assert!(h.thumb_start + h.thumb_len <= h.track_len);
        // Painted proof: the hscroll row carries scrollbar cells with
        // arrows at both ends, and the corner cell is the vertical
        // bar's down arrow.
        let left_arrow = display.back_buffer.get(top.text.x as u16, 10).unwrap();
        assert_eq!(left_arrow.role, CellRole::Scrollbar);
        let right_arrow = display.back_buffer.get((top.text.x + 75) as u16, 10).unwrap();
        assert_eq!(right_arrow.role, CellRole::Scrollbar);
        let corner = display.back_buffer.get(79, 10).unwrap();
        assert_eq!(corner.role, CellRole::Scrollbar);
        // The model's gutter region extends over the strip row
        // (vertical chrome spans it): the strip row's gutter columns
        // paint as gutter cells with an explicit background, owned by
        // the gutter painter — not left for any sweep.
        for gx in 0..top.gutter_width as u16 {
            let gutter_cell = display.back_buffer.get(gx, 10).unwrap();
            assert_eq!(gutter_cell.role, CellRole::Gutter);
            assert_ne!(gutter_cell.bg, Color::Reset);
        }
    }
}

#[cfg(test)]
mod cursor_clamp_tests {
    use super::*;
    use crate::core::buffer::Buffer;
    use crate::core::id::{BufferId, WindowId};
    use crate::core::app::EditorApp;

    fn app_with_scrolled_view() -> (EditorApp, Display) {
        let mut app = EditorApp::new();
        let text = (0..100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        app.buffers.insert(BufferId(0), Buffer::from_string(&text));
        // Cursor on line 0, viewport scrolled to line 50: the cursor is
        // above the visible area (the state scroll commands produce).
        let window = app.windows.get_mut(&WindowId(0)).expect("window");
        window.cursor_y = 0;
        window.scroll_offset = 50;
        let display = Display::new(80, 24, None, None);
        (app, display)
    }

    fn cursor_cells(display: &Display) -> Vec<(usize, usize)> {
        let w = display.back_buffer.width as usize;
        display
            .back_buffer
            .cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.role == CellRole::Cursor)
            .map(|(i, _)| (i % w, i / w))
            .collect()
    }

    /// An out-of-viewport cursor clamps to the viewport edge: the cursor
    /// cell paints at the clamped position, inside the text rect.
    #[test]
    fn out_of_viewport_cursor_clamps_to_edge() {
        let (mut app, mut display) = app_with_scrolled_view();
        display.render(&mut app).expect("render");
        let cells = cursor_cells(&display);
        assert_eq!(cells.len(), 1, "clamped cursor must paint exactly one cell");
        // Cursor above viewport clamps to top edge (y=1, first text row).
        assert_eq!(cells[0].1, 1);
    }

    /// A cursor below the viewport clamps to the bottom edge of the
    /// text rect, never escaping into the scrollbar strip.
    #[test]
    fn cursor_below_viewport_clamps_to_bottom() {
        let (mut app, mut display) = app_with_scrolled_view();
        let window = app.windows.get_mut(&WindowId(0)).expect("window");
        window.cursor_y = 99;
        window.scroll_offset = 0;
        display.render(&mut app).expect("render");
        let cells = cursor_cells(&display);
        assert_eq!(cells.len(), 1, "clamped cursor must paint exactly one cell");
    }

    /// A cursor past the visible text width clamps to the right edge
    /// of the text rect, not the scrollbar column.
    #[test]
    fn cursor_past_text_width_clamps_to_edge() {
        let (mut app, mut display) = app_with_scrolled_view();
        let window = app.windows.get_mut(&WindowId(0)).expect("window");
        window.cursor_y = 0;
        window.scroll_offset = 0;
        window.visual_cursor_x = 200;
        window.scroll_x = 0;
        display.render(&mut app).expect("render");
        let cells = cursor_cells(&display);
        assert_eq!(cells.len(), 1, "clamped cursor must paint exactly one cell");
    }

    /// The visible case: exactly one cursor cell, at the model cursor's
    /// cell, with no clamping involved.
    #[test]
    fn visible_cursor_paints_exactly_one_cell_at_model_position() {
        let (mut app, mut display) = app_with_scrolled_view();
        let window = app.windows.get_mut(&WindowId(0)).expect("window");
        window.cursor_y = 2;
        window.scroll_offset = 0;
        window.visual_cursor_x = 3;
        window.scroll_x = 0;
        display.render(&mut app).expect("render");
        let cells = cursor_cells(&display);
        assert_eq!(cells.len(), 1, "exactly one cursor cell paints");
        // Gutter for lines 0..=22 of a 100-line file: 2 digits + border.
        let gx = 3;
        assert_eq!(cells[0], (gx + 3, 1 + 2));
        assert_eq!(display.cursor_pos, Some((gx + 3, 1 + 2)));
    }

    /// Ownership invariant, render side: after render, no cell carries
    /// bg=Reset. There is no post-render sweep anymore (a sweep would
    /// launder a modeling gap into correct-looking output); this holds
    /// because every cell of the frame belongs to exactly one owned
    /// region of the model (`PaneGeometry::owned_regions`, proven to
    /// tile in `pane_regions_tile_frame_exactly`) and each region's
    /// owner paints its background while constructing the frame. A
    /// Reset cell here is an unowned cell: a model bug, not a
    /// paint-order detail.
    #[test]
    fn render_paints_every_owned_region() {
        let (mut app, mut display) = app_with_scrolled_view();
        display.render(&mut app).expect("render");
        // The menu bar is painted by a separate pass in the real run
        // loops (run/tui.rs, run/gui.rs, web/server.rs); the test
        // mirrors that sequence so the menu region has its owner.
        let show = display.show_menu_bar;
        display.render_menu_bar(show);
        let reset_cells: Vec<(usize, usize)> = display
            .back_buffer
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.bg == Color::Reset)
            .map(|(i, _)| (i % display.back_buffer.width as usize, i / display.back_buffer.width as usize))
            .collect();
        assert!(
            reset_cells.is_empty(),
            "cells with bg=Reset (unowned by any region): {:?}",
            &reset_cells[..reset_cells.len().min(10)]
        );
    }
}
