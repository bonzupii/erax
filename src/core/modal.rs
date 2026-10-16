//! Vim modal editing engine.
//!
//! Real modal semantics behind the [`INPUT_MODEL`](crate::config::keys::INPUT_MODEL)
//! switch: Normal / Insert / Visual / OperatorPending / CommandLine, counts,
//! operator+motion composition, registers, and `.` repeat. Word-end motions
//! (`e`/`E`/`ge`/`gE`) and WORD motions (`W`/`B`) share one word-boundary
//! primitive with the word-start motions; text objects (`iw`/`aw`/`iW`/`aW`,
//! pairs, quotes) are range producers through the same operator/visual
//! machinery; sentence motions (`(`/`)`) are typed commands; Ctrl+R is redo.
//!
//! Motions run as typed [`Command`]s through the dispatcher; operators resolve
//! the motion's byte range and apply it through the
//! [`Buffer`](crate::core::buffer::Buffer) API (`insert`/`delete`), which is
//! undo-recorded. Vim registers live in the engine — the emacs kill ring is
//! never touched, so the per-model clipboard separation on `EditorApp`
//! (`kill_ring` vs `clipboard` vs vim registers) holds.

use crate::core::bindings::{
    ExAction, ExArgAction, FindDir, FindEdge, InputModel, ModalAction, ModalGAction, ModalMode,
    ModalOperator, ModalTextObject,
};
use crate::config::keys::{
    EX_ARG_COMMANDS, EX_COMMANDS, INPUT_MODEL, MODAL_BINDINGS, MODAL_PREFIX_G, MODAL_TEXT_OBJECTS,
};
use crate::core::app::EditorApp;
use crate::core::buffer::Buffer;
use crate::core::command::Command;
use crate::core::dispatcher::{DispatchResult, InputAction, dispatch, dispatch_insert};
use crate::core::input::{Key, KeyPress};
use crate::core::prompt::handle_prompt_action;

/// What happened to a keypress the modal engine saw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalOutcome {
    /// The engine consumed the key.
    Consumed,
    /// Not a modal key (insert-mode special key, ctrl/alt chord): let the
    /// key trie handle it.
    PassThrough,
    /// A command requested application exit (`:q`, `:wq`).
    ExitRequested,
}

/// What the next keypress completes (multi-key sequences).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    None,
    /// `"` seen: the next key names the register.
    Register,
    /// `r` seen: the next key is the replacement character.
    ReplaceChar,
    /// `f`/`F`/`t`/`T` seen: the next key is the character to find.
    FindChar(FindDir, FindEdge),
    /// `g` seen: the next key completes the sequence (`gg`, `ge`, `gE`).
    PrefixG,
    /// `i`/`a` seen with an operator pending or in Visual mode: the next
    /// key names the text object (`iw`, `a(`, `i"`, …).
    TextObject { inner: bool },
}

/// A vim register: yanked/deleted text plus whether it was linewise.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Register {
    text: String,
    linewise: bool,
}

/// A re-executable motion, for operator composition and `.` repeat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Motion {
    /// A typed command used as a motion (h j k l w b 0 $ ^ G { } ( ) %).
    Cmd(Command),
    /// A find-char motion with its character.
    Find(FindDir, FindEdge, char),
    /// Whole lines (`dd`, `yy`, `cc`).
    Line,
    /// Linewise through an inclusive line range (`dgg`, `dG`, `d5G`:
    /// vim's `gg`/`G` are linewise motions).
    LineRange { lo: usize, hi: usize },
    /// Vim paragraph motions (`d}`, `y{`).
    ParagraphForward,
    ParagraphBackward,
    /// To end of line (`D`, `C`).
    ToEndOfLine,
    /// Charwise forward, clamped to the line end (`x`, `s`).
    CharsForward,
    /// Charwise backward (`X`).
    CharsBackward,
    /// A vim word motion with word-start semantics (`w`, `b`).
    WordForward,
    WordBackward,
    /// A vim word motion with word-end semantics (`e`, `ge`): inclusive.
    WordEndForward,
    WordEndBackward,
    /// WORD motions with word-start semantics (`W`, `B`).
    WordForwardBig,
    WordBackwardBig,
    /// WORD motions with word-end semantics (`E`, `gE`): inclusive.
    WordEndForwardBig,
    WordEndBackwardBig,
    /// A vim text object (`iw`, `aw`, `i(`, `a"`, …): a range producer
    /// that composes with operators and visual mode like a motion.
    TextObject(TextObjectKind),
    /// Open a line below / above (`o` / `O`).
    OpenBelow,
    OpenAbove,
}

/// Direction of a vim word motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WordDir {
    Forward,
    Backward,
}

/// Which word boundary a vim word motion targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WordEdge {
    /// `w` `b` `W` `B`: the first character of a word (exclusive motion).
    Start,
    /// `e` `ge` `E` `gE`: the last character of a word (inclusive motion).
    End,
}

/// A vim text object: `iw`/`aw`/`iW`/`aW`, pair objects, quote objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextObjectKind {
    /// `iw` / `aw` / `iW` / `aW`.
    Word { big: bool, inner: bool },
    /// `i(` / `a(` (and `)` / `b` aliases), `i[` / `a[`, `i{` / `a{`,
    /// `i<` / `a<`. `open` is the canonical opening bracket.
    Pair { open: char, inner: bool },
    /// `i"` / `a"` / `i'` / `a'`.
    Quote { q: char, inner: bool },
}

/// Where a completed text object key sequence was started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextObjectCtx {
    /// With an operator pending (`diw`): apply the operator to the range.
    Operator,
    /// In Visual mode (`viw`): select the range.
    Visual,
    /// Plain Normal mode (unreachable through the keymap, but harmless):
    /// move the cursor to the object's start.
    Normal,
}

/// Split an eight-way word [`Motion`] into the shared primitive's
/// (direction, edge, big-word) parameters.
fn word_motion_params(motion: Motion) -> Option<(WordDir, WordEdge, bool)> {
    use WordDir::{Backward, Forward};
    use WordEdge::{End, Start};
    let (dir, edge, big) = match motion {
        Motion::WordForward => (Forward, Start, false),
        Motion::WordBackward => (Backward, Start, false),
        Motion::WordEndForward => (Forward, End, false),
        Motion::WordEndBackward => (Backward, End, false),
        Motion::WordForwardBig => (Forward, Start, true),
        Motion::WordBackwardBig => (Backward, Start, true),
        Motion::WordEndForwardBig => (Forward, End, true),
        Motion::WordEndBackwardBig => (Backward, End, true),
        _ => return None,
    };
    Some((dir, edge, big))
}

/// Split an eight-way word [`ModalAction`] the same way.
fn word_action_params(action: ModalAction) -> Option<(WordDir, WordEdge, bool)> {
    use WordDir::{Backward, Forward};
    use WordEdge::{End, Start};
    let (dir, edge, big) = match action {
        ModalAction::WordForward => (Forward, Start, false),
        ModalAction::WordBackward => (Backward, Start, false),
        ModalAction::WordEndForward => (Forward, End, false),
        ModalAction::WordEndBackward => (Backward, End, false),
        ModalAction::WordForwardBig => (Forward, Start, true),
        ModalAction::WordBackwardBig => (Backward, Start, true),
        ModalAction::WordEndForwardBig => (Forward, End, true),
        ModalAction::WordEndBackwardBig => (Backward, End, true),
        _ => return None,
    };
    Some((dir, edge, big))
}

/// A repeatable change for `.`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ModalChange {
    /// A pure insert session: re-insert the text at the cursor.
    Insert { text: String },
    /// An operator applied to a motion, plus text typed afterwards for Change.
    Operator {
        op: ModalOperator,
        motion: Motion,
        count: usize,
        inserted: String,
    },
}

/// An insert session in progress: what `.` should record when Esc lands.
#[derive(Clone, Debug)]
struct InsertRecord {
    change: ModalChange,
}

/// The vim modal state machine. Owned by `EditorApp`; exactly one exists.
pub struct ModalEngine {
    mode: ModalMode,
    /// Accumulated count (`3` in `3dw`); 0 means none.
    count: usize,
    /// Pending operator; meaningful exactly when mode == OperatorPending.
    pending_operator: Option<ModalOperator>,
    pending: Pending,
    /// Active register for the next yank/put; `'"'` is the unnamed register.
    register: char,
    named: [Option<Register>; 36],
    unnamed: Option<Register>,
    last_find: Option<(FindDir, FindEdge, char)>,
    last_change: Option<ModalChange>,
    insert_record: Option<InsertRecord>,
    cmdline: String,
    /// Visual mode selected whole lines (`V`).
    linewise_visual: bool,
    /// Pending find-char in visual mode: (dir, edge, count).
    pending_visual_find: Option<(FindDir, FindEdge, usize)>,
}

impl ModalEngine {
    pub fn new() -> Self {
        Self {
            mode: ModalMode::Normal,
            count: 0,
            pending_operator: None,
            pending: Pending::None,
            register: '"',
            named: std::array::from_fn(|_| None),
            unnamed: None,
            last_find: None,
            last_change: None,
            insert_record: None,
            cmdline: String::new(),
            linewise_visual: false,
            pending_visual_find: None,
        }
    }

    /// Current modal mode (for tests and status display).
    pub fn mode(&self) -> ModalMode {
        self.mode
    }

    // -- screen-model accessors -----------------------------------------------
    //
    // The TUI screen model and the GUI/WebUI read mode UI state through these
    // small accessors only. They are pure views of engine state: adding them
    // changes no engine behavior.

    /// Persistent mode segment for the status/prompt bar, e.g. `-- INSERT --`.
    ///
    /// `None` for [`ModalMode::CommandLine`]: the `:` command line owns the
    /// whole bar there, so no segment is shown.
    pub fn mode_segment(&self) -> Option<&'static str> {
        match self.mode {
            ModalMode::Normal | ModalMode::OperatorPending => Some("-- NORMAL --"),
            ModalMode::Insert => Some("-- INSERT --"),
            ModalMode::Visual => {
                if self.linewise_visual {
                    Some("-- VISUAL LINE --")
                } else {
                    Some("-- VISUAL --")
                }
            }
            ModalMode::CommandLine => None,
        }
    }

    /// Pending Normal/OperatorPending keys as typed: register (`"a`), count
    /// (`3`), operator (`d`), and any in-progress multi-key prefix (`f`,
    /// `g`, `"` …), e.g. `"a2d`. Empty when nothing is pending.
    pub fn pending_text(&self) -> String {
        let mut s = String::new();
        if self.register != '"' {
            s.push('"');
            s.push(self.register);
        }
        if self.count > 0 {
            s.push_str(&self.count.to_string());
        }
        if let Some(op) = self.pending_operator {
            s.push(op_char(op));
        }
        match self.pending {
            Pending::None => {}
            Pending::Register => s.push('"'),
            Pending::ReplaceChar => s.push('r'),
            Pending::FindChar(FindDir::Forward, FindEdge::On) => s.push('f'),
            Pending::FindChar(FindDir::Backward, FindEdge::On) => s.push('F'),
            Pending::FindChar(FindDir::Forward, FindEdge::Before) => s.push('t'),
            Pending::FindChar(FindDir::Backward, FindEdge::Before) => s.push('T'),
            Pending::PrefixG => s.push('g'),
            Pending::TextObject { inner } => s.push(if inner { 'i' } else { 'a' }),
        }
        s
    }

    /// Accumulated count (`3` in `3dw`); 0 means none.
    pub fn pending_count(&self) -> usize {
        self.count
    }

    /// The operator awaiting its motion; meaningful exactly when
    /// [`ModalMode::OperatorPending`] is active.
    pub fn pending_operator(&self) -> Option<ModalOperator> {
        self.pending_operator
    }

    /// Register the next yank/put uses; `'"'` is the unnamed register.
    pub fn active_register(&self) -> char {
        self.register
    }

    /// `:` command-line text, without the leading `:`.
    pub fn cmdline_text(&self) -> &str {
        &self.cmdline
    }

    /// Byte offset of the command-line cursor within [`Self::cmdline_text`].
    /// The command line only edits at the end today, so this is the text
    /// length; it stays a real cursor for when mid-line editing lands.
    pub fn cmdline_cursor(&self) -> usize {
        self.cmdline.len()
    }

    /// Whether visual mode is selecting whole lines (`V`).
    pub fn is_linewise_visual(&self) -> bool {
        self.linewise_visual
    }

    /// Handle one keypress. Only active when `INPUT_MODEL` is Modal.
    pub fn handle_key(&mut self, app: &mut EditorApp, key: &KeyPress) -> ModalOutcome {
        if INPUT_MODEL != InputModel::Modal {
            return ModalOutcome::PassThrough;
        }
        self.handle_key_inner(app, key)
    }

    /// The state machine without the input-model gate (engine unit tests and
    /// crate-internal screen-model tests drive this directly; production
    /// always goes through [`Self::handle_key`]).
    pub(crate) fn handle_key_inner(&mut self, app: &mut EditorApp, key: &KeyPress) -> ModalOutcome {
        // The cue choke point: every mode transition in the engine
        // funnels through this function, so one before/after
        // comparison catches them all: no per-site instrumentation to
        // rot. The comparison is per keypress: `d` announces
        // OperatorPending and the completing `w` announces Normal, so
        // the cue always names the state the user was left in. A
        // silent mode change is a data-loss vector for non-visual
        // users, so this comparison is the safety-critical line here.
        let before = self.mode;
        let outcome = match self.mode {
            ModalMode::Normal => self.normal_key(app, key),
            ModalMode::Insert => self.insert_key(app, key),
            ModalMode::Visual => self.visual_key(app, key),
            ModalMode::OperatorPending => self.operator_pending_key(app, key),
            ModalMode::CommandLine => self.cmdline_key(app, key),
        };
        if self.mode != before {
            app.cue_bus.emit(crate::core::cue::Cue::ModeEntered(self.mode));
        }
        outcome
    }

    /// Text describing the modal state: pending keys (`"a2d`), the mode
    /// (`-- INSERT --`), or the `:` command line. Shown in the key-sequence
    /// area by the terminal glue.
    ///
    /// Composed from the screen-model accessors ([`Self::mode_segment`],
    /// [`Self::pending_text`]); the output is unchanged.
    pub fn status_text(&self) -> String {
        match self.mode {
            ModalMode::CommandLine => return format!(":{}", self.cmdline),
            ModalMode::Insert => return "-- INSERT --".to_string(),
            ModalMode::Visual => {
                return if self.linewise_visual {
                    "-- VISUAL LINE --".to_string()
                } else {
                    "-- VISUAL --".to_string()
                }
            }
            ModalMode::Normal | ModalMode::OperatorPending => {}
        }
        self.pending_text()
    }

    // -- helpers ----------------------------------------------------------

    /// Take the pending count (default 1) and reset it.
    fn take_count(&mut self) -> usize {
        let c = self.count.max(1);
        self.count = 0;
        c
    }

    /// Reset all transient Normal-mode state.
    fn reset_pending(&mut self) {
        self.count = 0;
        self.pending_operator = None;
        self.pending = Pending::None;
        self.register = '"';
    }

    fn enter_mode(&mut self, app: &mut EditorApp, mode: ModalMode) {
        self.reset_pending();
        self.insert_record = None;
        match mode {
            ModalMode::Visual => {
                self.linewise_visual = false;
                dispatch(app, Command::SetMark, 1);
            }
            ModalMode::CommandLine => {
                self.cmdline.clear();
            }
            ModalMode::Normal | ModalMode::Insert | ModalMode::OperatorPending => {
                clear_mark(app);
                if mode == ModalMode::Normal {
                    self.linewise_visual = false;
                }
            }
        }
        self.mode = mode;
    }

    /// Begin an insert session recording `change` for `.` repeat.
    fn enter_insert(&mut self, app: &mut EditorApp, change: ModalChange) {
        self.reset_pending();
        clear_mark(app);
        self.mode = ModalMode::Insert;
        self.insert_record = Some(InsertRecord { change });
    }

    /// Finish the insert session: Esc stores the change for `.`.
    fn finish_insert(&mut self) {
        if let Some(record) = self.insert_record.take() {
            let keep = match &record.change {
                ModalChange::Insert { text } => !text.is_empty(),
                ModalChange::Operator { .. } => true,
            };
            if keep {
                self.last_change = Some(record.change);
            }
        }
        self.mode = ModalMode::Normal;
        self.reset_pending();
    }

    /// Look up a binding for the current mode in the config table.
    /// Plain keys only: ctrl/alt chords are handled per mode, so the
    /// operator-pending motion lookup never sees them.
    fn lookup(&self, key: &KeyPress) -> Option<ModalAction> {
        if key.ctrl || key.alt {
            return None;
        }
        MODAL_BINDINGS
            .iter()
            .find(|b| b.mode == self.mode && b.key.key == key.key)
            .map(|b| b.action)
    }

    /// Run a dispatch and map `Exit` to the modal outcome. Informational
    /// results surface as the app message — a `:`-invoked command must show
    /// its output just like the run-command prompt does. Prompt-requesting
    /// results enter the
    /// focus manager like the TUI event loop does, so `/` and `?` start
    /// incremental search instead of dying silently.
    fn run(&mut self, app: &mut EditorApp, cmd: Command, count: usize) -> ModalOutcome {
        let result = dispatch(app, cmd, count);
        self.convert_result(app, result)
    }

    /// Surface a dispatch result the way a single command would: Info
    /// reaches the echo area, NeedsInput/AwaitKey open their prompts,
    /// Exit propagates.
    fn convert_result(
        &mut self,
        app: &mut EditorApp,
        result: DispatchResult,
    ) -> ModalOutcome {
        match result {
            DispatchResult::Exit => ModalOutcome::ExitRequested,
            DispatchResult::Info(msg) => {
                app.message = Some(msg);
                ModalOutcome::Consumed
            }
            DispatchResult::NeedsInput { prompt, action } => {
                use crate::core::focus::{FocusState, focus_target_for_action};
                let target = focus_target_for_action(action);
                app.focus_manager.push(FocusState::new(target, &prompt));
                ModalOutcome::Consumed
            }
            DispatchResult::AwaitKey(action) => {
                // describe-key (and quote-character) through the modal
                // engine: the same one-more-keypress focus the TUI event
                // loop pushes. Dropping it left describe-key silently dead
                // in vim mode.
                use crate::core::focus::{FocusState, focus_target_for_await_key};
                let prompt = match &action {
                    InputAction::QuoteCharacter => "Quote next key: ",
                    _ => "Describe Key: ",
                };
                let target = focus_target_for_await_key(action);
                app.focus_manager.push(FocusState::new(target, prompt));
                app.message = Some(prompt.trim_end().to_string());
                ModalOutcome::Consumed
            }
            DispatchResult::FileModified(buffer_id) => {
                crate::core::focus::push_reload_question(app, buffer_id);
                ModalOutcome::Consumed
            }
            DispatchResult::Success | DispatchResult::NotHandled | DispatchResult::Redraw => {
                // Success ran. NotHandled still consumed the key: it
                // mapped to a modal binding. Redraw is subsumed: the
                // caller repaints on Consumed.
                ModalOutcome::Consumed
            }
        }
    }
}

// -- Normal mode ------------------------------------------------------------

impl ModalEngine {
    fn normal_key(&mut self, app: &mut EditorApp, key: &KeyPress) -> ModalOutcome {
        // Multi-key sequences in progress.
        match self.pending {
            Pending::Register => return self.complete_register(key),
            Pending::ReplaceChar => return self.complete_replace(app, key),
            Pending::FindChar(dir, edge) => return self.complete_find(app, key, dir, edge, false),
            Pending::PrefixG => return self.complete_prefix_g(app, key),
            Pending::TextObject { inner } => {
                return self.complete_text_object(app, key, inner, TextObjectCtx::Normal);
            }
            Pending::None => {}
        }

        // Esc clears transient state.
        if key.key == Key::Esc && !key.ctrl && !key.alt {
            self.reset_pending();
            return ModalOutcome::Consumed;
        }

        // Ctrl/alt chords consult the modal table before the key trie, so
        // `C-r` redoes in Normal mode (with count) instead of reaching the
        // trie. Unmatched chords still pass through below.
        if (key.ctrl || key.alt)
            && let Some(action) = MODAL_BINDINGS
                .iter()
                .find(|b| {
                    b.mode == ModalMode::Normal
                        && b.key.key == key.key
                        && b.key.ctrl == key.ctrl
                        && b.key.alt == key.alt
                })
                .map(|b| b.action)
        {
            return self.run_normal_action(app, action);
        }

        // Counts: `3dw`. A leading `0` with no count is the `0` motion.
        if let Key::Char(c) = key.key
            && c.is_ascii_digit()
            && !key.ctrl
            && !key.alt
            && !(c == '0' && self.count == 0)
        {
            self.count = self
                .count
                .saturating_mul(10)
                .saturating_add((c as usize) - ('0' as usize));
            return ModalOutcome::Consumed;
        }

        match self.lookup(key) {
            Some(action) => self.run_normal_action(app, action),
            None => {
                if key.ctrl || key.alt {
                    return ModalOutcome::PassThrough;
                }
                app.message = Some(format!("{} is undefined", key));
                ModalOutcome::Consumed
            }
        }
    }

    fn run_normal_action(&mut self, app: &mut EditorApp, action: ModalAction) -> ModalOutcome {
        match action {
            ModalAction::Command(cmd) => {
                let count = self.take_count();
                self.reset_pending();
                self.run(app, cmd, count)
            }
            ModalAction::EnterMode(mode) => {
                self.enter_mode(app, mode);
                ModalOutcome::Consumed
            }
            ModalAction::Operator(op) => {
                self.pending_operator = Some(op);
                self.mode = ModalMode::OperatorPending;
                ModalOutcome::Consumed
            }
            ModalAction::SelectRegister => {
                self.pending = Pending::Register;
                ModalOutcome::Consumed
            }
            ModalAction::ReplaceChar => {
                self.pending = Pending::ReplaceChar;
                ModalOutcome::Consumed
            }
            ModalAction::FindChar(dir, edge) => {
                self.pending = Pending::FindChar(dir, edge);
                ModalOutcome::Consumed
            }
            ModalAction::RepeatFind(forward) => {
                let (dir, edge, ch) = match self.last_find {
                    Some(x) => x,
                    None => {
                        app.message = Some("No previous find".to_string());
                        return ModalOutcome::Consumed;
                    }
                };
                let dir = if forward {
                    dir
                } else {
                    flip_dir(dir)
                };
                self.last_find = Some((dir, edge, ch));
                let count = self.take_count();
                self.apply_find_motion(app, dir, edge, ch, count, None);
                self.reset_pending();
                ModalOutcome::Consumed
            }
            ModalAction::PrefixG => {
                self.pending = Pending::PrefixG;
                ModalOutcome::Consumed
            }
            ModalAction::RepeatLast => {
                let change = self.last_change.clone();
                match change {
                    Some(c) => self.replay_change(app, &c),
                    None => {
                        app.message = Some("Nothing to repeat".to_string());
                        ModalOutcome::Consumed
                    }
                }
            }
            ModalAction::DeleteCharForward => {
                let count = self.take_count();
                let pos = cursor_byte(app);
                let text = delete_chars_forward(app, pos, count);
                self.store_register(text, false);
                clamp_cursor(app);
                ensure_visible(app);
                self.record_simple_change(ModalOperator::Delete, Motion::CharsForward, count);
                self.reset_pending();
                ModalOutcome::Consumed
            }
            ModalAction::DeleteCharBackward => {
                let count = self.take_count();
                let e = cursor_byte(app);
                // The backward move clamps at column 0: delete exactly the
                // characters it covered, not `count` forward from there.
                let s = backward_chars_byte(app, e, count);
                let text = delete_char_range(app, s, e);
                self.store_register(text, false);
                place_cursor_at_byte(app, s);
                ensure_visible(app);
                self.record_simple_change(ModalOperator::Delete, Motion::CharsBackward, count);
                self.reset_pending();
                ModalOutcome::Consumed
            }
            ModalAction::SubstituteChar => {
                let count = self.take_count();
                let pos = cursor_byte(app);
                let text = delete_chars_forward(app, pos, count);
                self.store_register(text, false);
                clamp_cursor(app);
                ensure_visible(app);
                self.enter_insert(
                    app,
                    ModalChange::Operator {
                        op: ModalOperator::Change,
                        motion: Motion::CharsForward,
                        count,
                        inserted: String::new(),
                    },
                );
                ModalOutcome::Consumed
            }
            ModalAction::SubstituteLine => {
                let count = self.take_count();
                self.apply_linewise(app, ModalOperator::Change, count);
                self.enter_insert(
                    app,
                    ModalChange::Operator {
                        op: ModalOperator::Change,
                        motion: Motion::Line,
                        count,
                        inserted: String::new(),
                    },
                );
                ModalOutcome::Consumed
            }
            ModalAction::DeleteToEndOfLine => {
                let count = self.take_count();
                let change = ModalChange::Operator {
                    op: ModalOperator::Delete,
                    motion: Motion::ToEndOfLine,
                    count,
                    inserted: String::new(),
                };
                self.apply_operator(app, ModalOperator::Delete, Motion::ToEndOfLine, count);
                self.last_change = Some(change);
                self.reset_pending();
                ModalOutcome::Consumed
            }
            ModalAction::ChangeToEndOfLine => {
                let count = self.take_count();
                self.apply_operator(app, ModalOperator::Change, Motion::ToEndOfLine, count);
                self.enter_insert(
                    app,
                    ModalChange::Operator {
                        op: ModalOperator::Change,
                        motion: Motion::ToEndOfLine,
                        count,
                        inserted: String::new(),
                    },
                );
                ModalOutcome::Consumed
            }
            ModalAction::Put(before) => {
                self.take_count();
                self.reset_pending();
                self.do_put(app, before)
            }
            ModalAction::WordForward
            | ModalAction::WordBackward
            | ModalAction::WordEndForward
            | ModalAction::WordEndBackward
            | ModalAction::WordForwardBig
            | ModalAction::WordBackwardBig
            | ModalAction::WordEndForwardBig
            | ModalAction::WordEndBackwardBig => {
                let count = self.take_count();
                if let Some((dir, edge, big)) = word_action_params(action)
                    && let Some(target) = vim_word_edge_target(app, dir, edge, big, count)
                {
                    place_cursor_at_byte(app, target);
                }
                self.reset_pending();
                ModalOutcome::Consumed
            }
            // Visual-mode-only; the table never binds it in Normal mode.
            ModalAction::VisualChange => ModalOutcome::Consumed,
            ModalAction::VimLineEnd => {
                let count = self.take_count();
                // `2$` also goes down a line first (vim).
                if count > 1 {
                    self.run(app, Command::NextLine, count - 1);
                }
                vim_line_end(app);
                self.reset_pending();
                ModalOutcome::Consumed
            }
            ModalAction::VimFileEnd => {
                // Raw count (0 = none): `1G` is the first line, plain `G`
                // the last. `take_count` would collapse those.
                let raw = self.count;
                self.reset_pending();
                vim_file_end(app, raw);
                ModalOutcome::Consumed
            }
            ModalAction::VimParagraphForward | ModalAction::VimParagraphBackward => {
                let count = self.take_count();
                let fwd = action == ModalAction::VimParagraphForward;
                for _ in 0..count {
                    let before = cursor_byte(app);
                    if let Some(t) = vim_paragraph_target(app, fwd) {
                        place_cursor_at_byte(app, t);
                    }
                    if cursor_byte(app) == before {
                        break;
                    }
                }
                self.reset_pending();
                ModalOutcome::Consumed
            }
            ModalAction::InsertAtFirstNonBlank => {
                self.take_count();
                self.run(app, Command::SmartHome, 1);
                self.enter_insert(app, ModalChange::Insert { text: String::new() });
                ModalOutcome::Consumed
            }
            ModalAction::AppendAfterCursor => {
                self.take_count();
                self.run(app, Command::ForwardCharacter, 1);
                self.enter_insert(app, ModalChange::Insert { text: String::new() });
                ModalOutcome::Consumed
            }
            ModalAction::AppendAtEndOfLine => {
                self.take_count();
                self.run(app, Command::EndOfLine, 1);
                self.enter_insert(app, ModalChange::Insert { text: String::new() });
                ModalOutcome::Consumed
            }
            ModalAction::OpenLineBelow => {
                let count = self.take_count();
                self.open_line(app, true);
                self.enter_insert(
                    app,
                    ModalChange::Operator {
                        op: ModalOperator::Change,
                        motion: Motion::OpenBelow,
                        count,
                        inserted: String::new(),
                    },
                );
                ModalOutcome::Consumed
            }
            ModalAction::OpenLineAbove => {
                let count = self.take_count();
                self.open_line(app, false);
                self.enter_insert(
                    app,
                    ModalChange::Operator {
                        op: ModalOperator::Change,
                        motion: Motion::OpenAbove,
                        count,
                        inserted: String::new(),
                    },
                );
                ModalOutcome::Consumed
            }
            ModalAction::VisualLine => {
                self.take_count();
                self.reset_pending();
                // Anchor at the start of the line, cursor at its end.
                self.run(app, Command::BeginningOfLine, 1);
                dispatch(app, Command::SetMark, 1);
                self.run(app, Command::EndOfLine, 1);
                self.mode = ModalMode::Visual;
                self.linewise_visual = true;
                ModalOutcome::Consumed
            }
            ModalAction::TextObjectPrefix { .. } => {
                // Unreachable through the Normal keymap (`i`/`a` enter
                // insert there); operator-pending and Visual intercept the
                // keys before the table. Consume defensively.
                self.take_count();
                self.reset_pending();
                ModalOutcome::Consumed
            }
        }
    }

    /// `"` followed by a register name.
    fn complete_register(&mut self, key: &KeyPress) -> ModalOutcome {
        self.pending = Pending::None;
        match key.key {
            Key::Char(c) if c.is_ascii_alphanumeric() || c == '"' => {
                self.register = c;
            }
            _ => {
                self.register = '"';
            }
        }
        ModalOutcome::Consumed
    }

    /// `r` followed by the replacement character.
    fn complete_replace(&mut self, app: &mut EditorApp, key: &KeyPress) -> ModalOutcome {
        self.pending = Pending::None;
        let c = match key.key {
            Key::Char(c) if !key.ctrl && !key.alt => c,
            _ => {
                self.reset_pending();
                return ModalOutcome::Consumed;
            }
        };
        let pos = cursor_byte(app);
        let total = app.active_buffer().map(|b| b.len()).unwrap_or(0);
        if pos < total
            && let Some(buffer) = app.active_buffer_mut()
        {
            buffer.delete_char(pos);
            buffer.insert(pos, &c.to_string());
        }
        ensure_visible(app);
        self.last_change = Some(ModalChange::Operator {
            op: ModalOperator::Change,
            motion: Motion::CharsForward,
            count: 1,
            inserted: c.to_string(),
        });
        self.reset_pending();
        ModalOutcome::Consumed
    }

    /// `g` followed by the second key (`gg`).
    /// `g` followed by the second key: `gg`, `ge`, `gE` — as a pure motion,
    /// or as the pending operator's motion (`dgg`, `dge`).
    /// Complete the `g` prefix from the [`MODAL_PREFIX_G`](crate::config::keys::MODAL_PREFIX_G)
    /// config table. The table selects which key completes the sequence; the
    /// engine keeps the contextual part (operator-pending, counts).
    fn complete_prefix_g(&mut self, app: &mut EditorApp, key: &KeyPress) -> ModalOutcome {
        self.pending = Pending::None;
        let from_operator = self.mode == ModalMode::OperatorPending;
        let count = self.take_count();
        let action = MODAL_PREFIX_G
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, a)| *a);
        match action {
            Some(ModalGAction::GotoFirstLine) => self.gg_after_prefix(app, count, from_operator),
            Some(ModalGAction::WordEndBackward { big }) => {
                self.ge_after_prefix(app, big, count, from_operator)
            }
            Some(ModalGAction::ShowPosition) => {
                // `g C-g`: a mode change has no meaning for a position
                // report, so a pending operator is cancelled.
                if from_operator {
                    self.mode = ModalMode::Normal;
                }
                self.reset_pending();
                self.run(app, Command::WhatCursorPosition, 1)
            }
            None => {
                if from_operator {
                    self.mode = ModalMode::Normal;
                }
                self.reset_pending();
                ModalOutcome::Consumed
            }
        }
    }

    /// Complete `gg` after the `g` prefix: first line, or the pending
    /// operator's linewise motion (`dgg`), or a line number with a count.
    fn gg_after_prefix(
        &mut self,
        app: &mut EditorApp,
        count: usize,
        from_operator: bool,
    ) -> ModalOutcome {
        if from_operator {
            match self.pending_operator.take() {
                Some(op) => {
                    // `dgg`/`ygg`/`cgg`: linewise through the cursor
                    // line (vim's `gg` is a linewise motion).
                    let y = app
                        .active_window_ref()
                        .map(|w| w.cursor_y)
                        .unwrap_or(0);
                    self.finish_operator(app, op, Motion::LineRange { lo: 0, hi: y }, count)
                }
                None => {
                    self.mode = ModalMode::Normal;
                    self.reset_pending();
                }
            }
        } else {
            self.reset_pending();
            if count > 1 {
                // `5gg` goes to line 5.
                let input = count.to_string();
                let _ = handle_prompt_action(app, InputAction::GotoLine, input);
                return ModalOutcome::Consumed;
            }
            return self.run(app, Command::BeginningOfFile, 1);
        }
        ModalOutcome::Consumed
    }

    /// Complete `ge` / `gE` after the `g` prefix: pure motion, or the
    /// pending operator's motion.
    fn ge_after_prefix(
        &mut self,
        app: &mut EditorApp,
        big: bool,
        count: usize,
        from_operator: bool,
    ) -> ModalOutcome {
        let motion = if big {
            Motion::WordEndBackwardBig
        } else {
            Motion::WordEndBackward
        };
        if from_operator {
            match self.pending_operator.take() {
                Some(op) => self.finish_operator(app, op, motion, count),
                None => {
                    self.mode = ModalMode::Normal;
                    self.reset_pending();
                }
            }
        } else {
            if let Some(target) =
                vim_word_edge_target(app, WordDir::Backward, WordEdge::End, big, count)
            {
                place_cursor_at_byte(app, target);
            }
            self.reset_pending();
        }
        ModalOutcome::Consumed
    }

    /// `i`/`a` followed by the object key: resolve the text object from the
    /// [`MODAL_TEXT_OBJECTS`](crate::config::keys::MODAL_TEXT_OBJECTS)
    /// config table and apply it for the context — the pending operator's
    /// range, the visual selection, or (Normal) a cursor move to the
    /// object's start. Whether the object is inner or around comes from the
    /// pending `i`/`a` key, not the table.
    fn complete_text_object(
        &mut self,
        app: &mut EditorApp,
        key: &KeyPress,
        inner: bool,
        ctx: TextObjectCtx,
    ) -> ModalOutcome {
        self.pending = Pending::None;
        let selected = MODAL_TEXT_OBJECTS
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, o)| *o);
        let kind = match selected {
            Some(ModalTextObject::Word { big }) => TextObjectKind::Word { big, inner },
            Some(ModalTextObject::Pair { open }) => TextObjectKind::Pair { open, inner },
            Some(ModalTextObject::Quote { q }) => TextObjectKind::Quote { q, inner },
            None => {
                // Not an object key: cancel the sequence (and the operator).
                if ctx == TextObjectCtx::Operator {
                    self.mode = ModalMode::Normal;
                }
                self.reset_pending();
                return ModalOutcome::Consumed;
            }
        };
        let count = self.take_count();
        let (s, e) = match text_object_range(app, kind, count) {
            Some(r) => r,
            None => {
                // No object found (unmatched bracket, …): cancel quietly.
                if ctx == TextObjectCtx::Operator {
                    self.mode = ModalMode::Normal;
                }
                self.reset_pending();
                return ModalOutcome::Consumed;
            }
        };
        match ctx {
            TextObjectCtx::Operator => {
                if self.pending_operator.is_some() {
                    let op = self.pending_operator.take().unwrap_or(ModalOperator::Delete);
                    // Yank placement (cursor to the object's start) happens
                    // in `apply_operator`'s TextObject arm.
                    self.finish_operator(app, op, Motion::TextObject(kind), count);
                } else {
                    self.mode = ModalMode::Normal;
                    self.reset_pending();
                }
            }
            TextObjectCtx::Visual => {
                set_mark_at_byte(app, s);
                place_cursor_at_byte(app, e);
                ensure_visible(app);
                self.reset_pending();
            }
            TextObjectCtx::Normal => {
                place_cursor_at_byte(app, s);
                ensure_visible(app);
                self.reset_pending();
            }
        }
        ModalOutcome::Consumed
    }

    /// `f`/`F`/`t`/`T` followed by the character: a motion, or the motion of
    /// the pending operator.
    fn complete_find(
        &mut self,
        app: &mut EditorApp,
        key: &KeyPress,
        dir: FindDir,
        edge: FindEdge,
        _from_operator: bool,
    ) -> ModalOutcome {
        let c = match key.key {
            Key::Char(c) if !key.ctrl && !key.alt => c,
            _ => {
                self.pending = Pending::None;
                if self.mode == ModalMode::OperatorPending {
                    self.mode = ModalMode::Normal;
                    self.pending_operator = None;
                }
                self.reset_pending();
                return ModalOutcome::Consumed;
            }
        };
        self.pending = Pending::None;
        let count = self.take_count();
        self.last_find = Some((dir, edge, c));
        let op = if self.mode == ModalMode::OperatorPending {
            self.pending_operator.take()
        } else {
            None
        };
        if self.mode == ModalMode::OperatorPending {
            self.mode = ModalMode::Normal;
        }
        self.apply_find_motion(app, dir, edge, c, count, op);
        match op {
            Some(ModalOperator::Change) => self.enter_insert(
                app,
                ModalChange::Operator {
                    op: ModalOperator::Change,
                    motion: Motion::Find(dir, edge, c),
                    count,
                    inserted: String::new(),
                },
            ),
            Some(op) => {
                self.last_change = Some(ModalChange::Operator {
                    op,
                    motion: Motion::Find(dir, edge, c),
                    count,
                    inserted: String::new(),
                });
                self.reset_pending();
            }
            None => self.reset_pending(),
        }
        ModalOutcome::Consumed
    }

    /// Move the cursor to the find-char target (pure motion).
    fn apply_find_motion(
        &mut self,
        app: &mut EditorApp,
        dir: FindDir,
        edge: FindEdge,
        ch: char,
        count: usize,
        op: Option<ModalOperator>,
    ) {
        let target = find_char_target(app, dir, edge, ch, count);
        let target = match target {
            Some(t) => t,
            None => return,
        };
        match op {
            None => {
                // Pure motion: place the cursor on the target byte.
                place_cursor_at_byte(app, target);
            }
            Some(op) => {
                let pre = cursor_byte(app);
                let (pre_x, pre_y) = cursor_xy(app);
                // Operators use the `f`/`F` position (probed 2026-10-15):
                // `df` takes the found character, `dt` stops before it
                // but includes the `t` target; backward mirrors this.
                let f_pos = find_char_target(app, dir, FindEdge::On, ch, count);
                let f_pos = match f_pos {
                    Some(p) => p,
                    None => return,
                };
                let (s, e) = match (dir, edge) {
                    (FindDir::Forward, FindEdge::On) => (pre, f_pos + char_len_at(app, f_pos)),
                    (FindDir::Forward, FindEdge::Before) => (pre, f_pos),
                    (FindDir::Backward, FindEdge::On) => (f_pos, pre),
                    (FindDir::Backward, FindEdge::Before) => (f_pos + char_len_at(app, f_pos), pre),
                };
                let (s, e) = (s.min(e), s.max(e));
                let restore = if s == pre { Some((pre_x, pre_y)) } else { None };
                self.apply_range(app, op, s, e, false, restore);
                if s != pre {
                    // Backward motion: the cursor ends at the start of the
                    // changed/yanked text (vim).
                    place_cursor_at_byte(app, s);
                    vim_clamp_to_char(app);
                }
            }
        }
    }

    /// Record a simple (non-insert) change for `.` repeat.
    fn record_simple_change(&mut self, op: ModalOperator, motion: Motion, count: usize) {
        self.last_change = Some(ModalChange::Operator {
            op,
            motion,
            count,
            inserted: String::new(),
        });
    }

    /// Apply a pending operator with its motion, record it for `.` repeat,
    /// and enter Insert for Change. The shared tail of every completed
    /// operator+motion sequence.
    fn finish_operator(
        &mut self,
        app: &mut EditorApp,
        op: ModalOperator,
        motion: Motion,
        count: usize,
    ) {
        self.mode = ModalMode::Normal;
        self.apply_operator(app, op, motion, count);
        let change = ModalChange::Operator {
            op,
            motion,
            count,
            inserted: String::new(),
        };
        if op == ModalOperator::Change {
            self.enter_insert(app, change);
        } else {
            self.last_change = Some(change);
            self.reset_pending();
        }
    }
}

// -- Operator-pending mode --------------------------------------------------

impl ModalEngine {
    fn operator_pending_key(&mut self, app: &mut EditorApp, key: &KeyPress) -> ModalOutcome {
        // Multi-key sequences complete here too (`dfx`, `dge`, `diw`).
        match self.pending {
            Pending::FindChar(dir, edge) => return self.complete_find(app, key, dir, edge, true),
            Pending::PrefixG => return self.complete_prefix_g(app, key),
            Pending::TextObject { inner } => {
                return self.complete_text_object(app, key, inner, TextObjectCtx::Operator);
            }
            Pending::Register | Pending::ReplaceChar => {
                self.pending = Pending::None;
            }
            Pending::None => {}
        }

        if key.key == Key::Esc && !key.ctrl && !key.alt {
            self.mode = ModalMode::Normal;
            self.reset_pending();
            return ModalOutcome::Consumed;
        }

        // Counts accumulate before the motion (`d3w`). A leading `0`
        // is the `0` motion, not a count (`d0`).
        if let Key::Char(c) = key.key
            && c.is_ascii_digit()
            && !key.ctrl
            && !key.alt
            && !(c == '0' && self.count == 0)
        {
            self.count = self
                .count
                .saturating_mul(10)
                .saturating_add((c as usize) - ('0' as usize));
            return ModalOutcome::Consumed;
        }

        // `i` / `a` begin a text object when an operator is pending (`diw`,
        // `ca"`). In Normal mode the same keys enter insert; the binding
        // table below would do that, so intercept them first. The operator
        // stays pending for `complete_text_object`.
        if !key.ctrl && !key.alt {
            match key.key {
                Key::Char('i') => {
                    self.pending = Pending::TextObject { inner: true };
                    return ModalOutcome::Consumed;
                }
                Key::Char('a') => {
                    self.pending = Pending::TextObject { inner: false };
                    return ModalOutcome::Consumed;
                }
                _ => {}
            }
        }

        let op = match self.pending_operator.take() {
            Some(op) => op,
            None => {
                self.mode = ModalMode::Normal;
                return ModalOutcome::Consumed;
            }
        };
        // Motions are the Normal-mode bindings; reuse the same table.
        let saved_mode = self.mode;
        self.mode = ModalMode::Normal;
        let action = self.lookup(key);
        self.mode = saved_mode;

        // Raw count (0 = none): `d5G` needs the 5, not `max(1)`.
        let raw_count = self.count;
        let count = self.take_count();
        match action {
            // The same operator key twice is linewise (`dd`, `yy`, `cc`).
            Some(ModalAction::Operator(op2)) if op2 == op => {
                self.finish_operator(app, op, Motion::Line, count);
                ModalOutcome::Consumed
            }
            // Vim `$` as a motion: through the last character (`d$`).
            Some(ModalAction::VimLineEnd) => {
                self.finish_operator(app, op, Motion::ToEndOfLine, count);
                ModalOutcome::Consumed
            }
            // Vim `G` as a motion: linewise (`dG`, `d5G`).
            Some(ModalAction::VimFileEnd) => {
                let y = app.active_window_ref().map(|w| w.cursor_y).unwrap_or(0);
                let last = vim_last_line(app).unwrap_or(y);
                let target = if raw_count == 0 {
                    last
                } else {
                    raw_count.saturating_sub(1).min(last)
                };
                let (lo, hi) = (y.min(target), y.max(target));
                self.finish_operator(app, op, Motion::LineRange { lo, hi }, count);
                ModalOutcome::Consumed
            }
            // Vim `}`/`{` as motions (`d}`, `y{`).
            Some(ModalAction::VimParagraphForward) => {
                self.finish_operator(app, op, Motion::ParagraphForward, count);
                ModalOutcome::Consumed
            }
            Some(ModalAction::VimParagraphBackward) => {
                self.finish_operator(app, op, Motion::ParagraphBackward, count);
                ModalOutcome::Consumed
            }
            Some(ModalAction::Command(cmd)) => {
                self.finish_operator(app, op, Motion::Cmd(cmd), count);
                ModalOutcome::Consumed
            }
            Some(ModalAction::FindChar(dir, edge)) => {
                // The character comes next; stay in OperatorPending.
                self.pending_operator = Some(op);
                self.pending = Pending::FindChar(dir, edge);
                ModalOutcome::Consumed
            }
            Some(ModalAction::PrefixG) => {
                // `dge`, `dgE`, `dgg`: the second key comes next; the
                // operator stays pending for `complete_prefix_g`.
                self.pending_operator = Some(op);
                self.pending = Pending::PrefixG;
                ModalOutcome::Consumed
            }
            Some(
                ModalAction::WordForward
                | ModalAction::WordBackward
                | ModalAction::WordEndForward
                | ModalAction::WordEndBackward
                | ModalAction::WordForwardBig
                | ModalAction::WordBackwardBig
                | ModalAction::WordEndForwardBig
                | ModalAction::WordEndBackwardBig,
            ) => {
                let motion = match action {
                    Some(ModalAction::WordForward) => Motion::WordForward,
                    Some(ModalAction::WordBackward) => Motion::WordBackward,
                    Some(ModalAction::WordEndForward) => Motion::WordEndForward,
                    Some(ModalAction::WordEndBackward) => Motion::WordEndBackward,
                    Some(ModalAction::WordForwardBig) => Motion::WordForwardBig,
                    Some(ModalAction::WordBackwardBig) => Motion::WordBackwardBig,
                    Some(ModalAction::WordEndForwardBig) => Motion::WordEndForwardBig,
                    _ => Motion::WordEndBackwardBig,
                };
                self.finish_operator(app, op, motion, count);
                ModalOutcome::Consumed
            }
            _ => {
                app.message = Some("Operator needs a motion".to_string());
                self.mode = ModalMode::Normal;
                self.reset_pending();
                ModalOutcome::Consumed
            }
        }
    }
}

// -- Visual mode ------------------------------------------------------------

impl ModalEngine {
    fn visual_key(&mut self, app: &mut EditorApp, key: &KeyPress) -> ModalOutcome {
        // Pending multi-key sequences complete before the binding table:
        // `ge`/`gE` after `g`, and the object key after `i`/`a`.
        match self.pending {
            Pending::TextObject { inner } => {
                return self.complete_text_object(app, key, inner, TextObjectCtx::Visual);
            }
            Pending::PrefixG => return self.complete_prefix_g(app, key),
            _ => {}
        }
        match self.lookup(key) {
            Some(ModalAction::Command(Command::KillRegion)) => {
                // `d` / `x`: delete the selection into a register.
                let count = self.take_count();
                let _ = count;
                self.visual_apply(app, ModalOperator::Delete);
                self.enter_mode(app, ModalMode::Normal);
                ModalOutcome::Consumed
            }
            Some(ModalAction::Command(Command::CopyRegion)) => {
                // `y`: yank the selection into a register.
                self.take_count();
                self.visual_apply(app, ModalOperator::Yank);
                self.enter_mode(app, ModalMode::Normal);
                ModalOutcome::Consumed
            }
            Some(ModalAction::VisualChange) => {
                // `c`: delete the selection and enter Insert.
                self.take_count();
                self.visual_apply(app, ModalOperator::Change);
                self.enter_insert(
                    app,
                    ModalChange::Insert { text: String::new() },
                );
                ModalOutcome::Consumed
            }
            Some(ModalAction::Command(cmd)) => {
                let count = self.take_count();
                let r = self.run(app, cmd, count);
                // In visual mode, a forward sentence motion includes its
                // target character (the selection is cursor-exclusive).
                if cmd == Command::ForwardSentence {
                    let pos = cursor_byte(app);
                    let total = app.active_buffer().map(|b| b.len()).unwrap_or(0);
                    if pos < total {
                        place_cursor_at_byte(app, pos + char_len_at(app, pos));
                    }
                }
                r
            }
            Some(ModalAction::EnterMode(ModalMode::Normal)) => {
                // `v` toggles off, Esc cancels: clear the mark and go Normal.
                self.take_count();
                self.enter_mode(app, ModalMode::Normal);
                ModalOutcome::Consumed
            }
            Some(
                a @ (ModalAction::WordForward
                | ModalAction::WordBackward
                | ModalAction::WordEndForward
                | ModalAction::WordEndBackward
                | ModalAction::WordForwardBig
                | ModalAction::WordBackwardBig
                | ModalAction::WordEndForwardBig
                | ModalAction::WordEndBackwardBig),
            ) => {
                let count = self.take_count();
                if let Some((dir, edge, big)) = word_action_params(a)
                    && let Some(target) = vim_word_edge_target(app, dir, edge, big, count)
                {
                    // The engine's visual selection is cursor-exclusive
                    // (`vlll` selects 3 chars), so a forward motion steps
                    // one char past its target: `ve` covers the word end
                    // and `vw` covers the next word's first character, as
                    // in vim.
                    let at = if dir == WordDir::Forward {
                        target + char_len_at(app, target)
                    } else {
                        target
                    };
                    place_cursor_at_byte(app, at);
                    ensure_visible(app);
                }
                ModalOutcome::Consumed
            }
            Some(ModalAction::FindChar(dir, edge)) => {
                let count = self.take_count();
                // In visual mode the find extends the selection; the next key
                // is the character. Reuse pending, staying in Visual.
                self.pending = Pending::FindChar(dir, edge);
                self.pending_visual_find = Some((dir, edge, count));
                ModalOutcome::Consumed
            }
            Some(ModalAction::PrefixG) => {
                // `vge` / `vgE`: the second key completes the sequence.
                self.pending = Pending::PrefixG;
                ModalOutcome::Consumed
            }
            Some(
                a @ (ModalAction::VimParagraphForward | ModalAction::VimParagraphBackward),
            ) => {
                // `v}` / `v{`: extend the selection to the paragraph target.
                let count = self.take_count();
                let fwd = a == ModalAction::VimParagraphForward;
                for _ in 0..count {
                    let before = cursor_byte(app);
                    if let Some(t) = vim_paragraph_target(app, fwd) {
                        place_cursor_at_byte(app, t);
                    }
                    if cursor_byte(app) == before {
                        break;
                    }
                }
                ensure_visible(app);
                ModalOutcome::Consumed
            }
            Some(ModalAction::TextObjectPrefix { inner }) => {
                // `viw`, `va(`, `vi"`, …: the object key comes next.
                self.pending = Pending::TextObject { inner };
                ModalOutcome::Consumed
            }
            Some(_) => {
                app.message = Some("Not available in visual mode".to_string());
                ModalOutcome::Consumed
            }
            None => {
                // The pending find-char character completes here.
                if let Pending::FindChar(dir, edge) = self.pending {
                    let (dir, edge, count) = self.pending_visual_find.take().unwrap_or((dir, edge, 1));
                    self.pending = Pending::None;
                    if let Key::Char(c) = key.key
                        && !key.ctrl && !key.alt
                    {
                        self.last_find = Some((dir, edge, c));
                        if let Some(target) = find_char_target(app, dir, edge, c, count) {
                            place_cursor_at_byte(app, target);
                            ensure_visible(app);
                        }
                        return ModalOutcome::Consumed;
                    }
                    return ModalOutcome::Consumed;
                }
                if key.ctrl || key.alt {
                    return ModalOutcome::PassThrough;
                }
                app.message = Some(format!("{} is undefined", key));
                ModalOutcome::Consumed
            }
        }
    }

    /// Apply Delete/Yank/Change to the visual selection (mark..cursor).
    fn visual_apply(&mut self, app: &mut EditorApp, op: ModalOperator) {
        let (s, e) = match selection_bytes(app) {
            Some(x) => x,
            None => return,
        };
        let linewise = self.linewise_visual;
        let (s, e) = if linewise {
            expand_to_lines(app, s, e)
        } else {
            (s, e)
        };
        self.apply_range(app, op, s, e, linewise, None);
        // Like vim, the cursor ends at the start of the changed/yanked text.
        place_cursor_at_byte(app, s);
        if op != ModalOperator::Yank {
            vim_clamp_to_char(app);
        }
        if op == ModalOperator::Change {
            // enter_insert is called by the caller for visual `c`.
        }
    }
}

// -- Insert mode ------------------------------------------------------------

impl ModalEngine {
    fn insert_key(&mut self, app: &mut EditorApp, key: &KeyPress) -> ModalOutcome {
        if key.key == Key::Esc && !key.ctrl && !key.alt {
            self.finish_insert();
            // Vim backs the cursor up one when leaving insert.
            self.run(app, Command::BackwardCharacter, 1);
            return ModalOutcome::Consumed;
        }
        match key.key {
            Key::Char(c) if !key.ctrl && !key.alt => {
                let r = dispatch_insert(app, c, 1);
                if r == DispatchResult::Exit {
                    return ModalOutcome::ExitRequested;
                }
                if let Some(record) = self.insert_record.as_mut() {
                    match &mut record.change {
                        ModalChange::Insert { text } => text.push(c),
                        ModalChange::Operator { inserted, .. } => inserted.push(c),
                    }
                }
                ModalOutcome::Consumed
            }
            _ => ModalOutcome::PassThrough,
        }
    }
}

// -- Command-line mode ------------------------------------------------------

impl ModalEngine {
    fn cmdline_key(&mut self, app: &mut EditorApp, key: &KeyPress) -> ModalOutcome {
        match key.key {
            Key::Esc if !key.ctrl && !key.alt => {
                self.cmdline.clear();
                self.enter_mode(app, ModalMode::Normal);
                ModalOutcome::Consumed
            }
            Key::Enter if !key.ctrl && !key.alt => {
                let line: String = std::mem::take(&mut self.cmdline);
                self.enter_mode(app, ModalMode::Normal);
                self.execute_cmdline(app, &line)
            }
            Key::Backspace if !key.ctrl && !key.alt => {
                self.cmdline.pop();
                ModalOutcome::Consumed
            }
            Key::Char(c) if !key.ctrl && !key.alt => {
                self.cmdline.push(c);
                ModalOutcome::Consumed
            }
            _ => ModalOutcome::Consumed,
        }
    }

    fn execute_cmdline(&mut self, app: &mut EditorApp, line: &str) -> ModalOutcome {
        let line = line.trim();
        if line.is_empty() {
            return ModalOutcome::Consumed;
        }
        // `:<number>` goes to that line.
        if line.chars().all(|c| c.is_ascii_digit()) {
            let _ = handle_prompt_action(app, InputAction::GotoLine, line.to_string());
            return ModalOutcome::Consumed;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("").trim();
        // Commands that take a path argument read it from the config
        // table; the engine only splits command from argument.
        if !arg.is_empty()
            && let Some((_, action)) = EX_ARG_COMMANDS.iter().find(|(name, _)| *name == cmd)
        {
            match action {
                ExArgAction::SaveAs => {
                    let _ = handle_prompt_action(app, InputAction::SaveAs, arg.to_string());
                }
                ExArgAction::Open => {
                    let _ = handle_prompt_action(app, InputAction::OpenFile, arg.to_string());
                }
            }
            return ModalOutcome::Consumed;
        }
        // The ex vocabulary is config data; the engine interprets the
        // action variants. `:q`'s modified-buffer refusal is a state
        // decision, so it stays here rather than in the table.
        match EX_COMMANDS.iter().find(|(name, _)| *name == cmd).map(|(_, a)| *a) {
            Some(ExAction::Run(command)) => self.run(app, command, 1),
            Some(ExAction::RunSequence(commands)) => {
                // A sequence stops at the first step that does not
                // cleanly succeed: `:wq` with a failed or impossible
                // save must not run the quit. Only the last outcome
                // used to survive, so a NeedsInput (SaveAs prompt) or
                // an Info ("Save failed") from the save was silently
                // overwritten by the quit that followed it.
                for command in commands.iter() {
                    let result = dispatch(app, *command, 1);
                    let stop = !matches!(
                        result,
                        DispatchResult::Success
                            | DispatchResult::Redraw
                            | DispatchResult::NotHandled
                    );
                    let outcome = self.convert_result(app, result);
                    if stop {
                        return outcome;
                    }
                }
                ModalOutcome::Consumed
            }
            Some(ExAction::ConditionalQuit) => {
                let modified = app.buffers.values().any(|b| b.modified);
                if modified {
                    app.message = Some("No write since last change (add ! to override)".to_string());
                    ModalOutcome::Consumed
                } else {
                    self.run(app, Command::ExitErax, 1)
                }
            }
            // Every other name resolves through the typed command table, so
            // the vim model reaches every command through its native idiom
            // (the `:` line), the same capability Emacs gets from M-x.
            None => match Command::from_name(cmd) {
                // ToggleMenu is intercepted by the event loop before
                // dispatch; the modal engine owns no display state, so it
                // cannot run it from here.
                Some(Command::ToggleMenu) => {
                    app.message = Some("toggle-menu needs the event loop; use F10".to_string());
                    ModalOutcome::Consumed
                }
                Some(command) => self.run(app, command, 1),
                None => {
                    app.message = Some(format!("Not an editor command: {}", cmd));
                    ModalOutcome::Consumed
                }
            },
        }
    }
}

// -- Operator application ---------------------------------------------------

impl ModalEngine {
    /// Apply an operator to a motion: resolve the byte range, then act.
    fn apply_operator(
        &mut self,
        app: &mut EditorApp,
        op: ModalOperator,
        motion: Motion,
        count: usize,
    ) {
        let count = count.max(1);
        match motion {
            Motion::Line => self.apply_linewise(app, op, count),
            Motion::LineRange { lo, hi } => self.apply_linewise_range(app, op, lo, hi),
            Motion::ParagraphForward | Motion::ParagraphBackward => {
                self.apply_paragraph_operator(app, op, motion == Motion::ParagraphForward, count);
            }
            Motion::ToEndOfLine => {
                let s = cursor_byte(app);
                let e = line_end_byte(app, s);
                self.apply_range(app, op, s, e, false, None);
                if op == ModalOperator::Delete {
                    // The cursor ends at the start of the deleted text (vim).
                    place_cursor_at_byte(app, s);
                    vim_clamp_to_char(app);
                } else if op == ModalOperator::Change {
                    // Insert begins at the deletion point (past-end is fine
                    // in Insert mode).
                    place_cursor_at_byte(app, s);
                }
            }
            Motion::Cmd(cmd) => {
                let (pre_x, pre_y) = cursor_xy(app);
                let pre = cursor_byte(app);
                dispatch(app, Command::SetMark, 1);
                dispatch(app, cmd, count);
                let post = cursor_byte(app);
                let (s, e) = (pre.min(post), pre.max(post));
                // Restore the cursor to the start of the changed text: for a
                // forward motion that is the pre-motion position.
                let restore = if s == pre { Some((pre_x, pre_y)) } else { None };
                self.apply_range(app, op, s, e, false, restore);
                if op == ModalOperator::Yank && s != pre {
                    // A yanked range leaves the cursor at its start (vim).
                    place_cursor_at_byte(app, s);
                }
            }
            Motion::Find(dir, edge, ch) => {
                let pre = cursor_byte(app);
                let (pre_x, pre_y) = cursor_xy(app);
                // Operators use the `f`/`F` position (see apply_find_motion).
                if let Some(f_pos) = find_char_target(app, dir, FindEdge::On, ch, count) {
                    let (s, e) = match (dir, edge) {
                        (FindDir::Forward, FindEdge::On) => (pre, f_pos + char_len_at(app, f_pos)),
                        (FindDir::Forward, FindEdge::Before) => (pre, f_pos),
                        (FindDir::Backward, FindEdge::On) => (f_pos, pre),
                        (FindDir::Backward, FindEdge::Before) => {
                            (f_pos + char_len_at(app, f_pos), pre)
                        }
                    };
                    let (s, e) = (s.min(e), s.max(e));
                    let restore = if s == pre { Some((pre_x, pre_y)) } else { None };
                    self.apply_range(app, op, s, e, false, restore);
                    if s != pre {
                        place_cursor_at_byte(app, s);
                        vim_clamp_to_char(app);
                    }
                }
            }
            Motion::CharsForward => {
                let s = cursor_byte(app);
                let e = forward_chars_byte(app, s, count);
                self.apply_range(app, op, s, e, false, None);
            }
            Motion::CharsBackward => {
                let e = cursor_byte(app);
                let s = backward_chars_byte(app, e, count);
                self.apply_range(app, op, s, e, false, None);
            }
            Motion::WordForward
            | Motion::WordBackward
            | Motion::WordEndForward
            | Motion::WordEndBackward
            | Motion::WordForwardBig
            | Motion::WordBackwardBig
            | Motion::WordEndForwardBig
            | Motion::WordEndBackwardBig => {
                let (dir, edge, big) = match word_motion_params(motion) {
                    Some(p) => p,
                    None => return,
                };
                let pre = cursor_byte(app);
                let (pre_x, pre_y) = cursor_xy(app);
                let nonblank = char_at(app, pre).is_some_and(|c| !c.is_whitespace());
                // `cw`/`cW` special case (vim, probed 2026-10-15): on a
                // non-blank, count 1 changes to the end of the CURRENT
                // word (inclusive) without advancing like `ce` does;
                // with a count it behaves like `ce`/`cE`. On a blank,
                // `cw` keeps `w` semantics (like `dw`); delete and yank
                // never take this path.
                let cw_current = op == ModalOperator::Change
                    && dir == WordDir::Forward
                    && edge == WordEdge::Start
                    && count == 1
                    && nonblank;
                let cw_like_ce = op == ModalOperator::Change
                    && dir == WordDir::Forward
                    && edge == WordEdge::Start
                    && count > 1
                    && nonblank;
                let (target, eff_edge) = if cw_current {
                    match current_word_end(app, pre, big) {
                        Some(t) => (t, WordEdge::End),
                        None => return,
                    }
                } else {
                    let e = if cw_like_ce { WordEdge::End } else { edge };
                    match vim_word_edge_target(app, dir, e, big, count) {
                        Some(t) => (t, e),
                        // A forward word-end motion that goes nowhere (the
                        // cursor is on the last word's last character) still
                        // covers that character for the operator (`de` on
                        // `a` deletes it, like vim).
                        None
                            if dir == WordDir::Forward
                                && e == WordEdge::End
                                && nonblank =>
                        {
                            (pre, WordEdge::End)
                        }
                        None => return,
                    }
                };
                // Word-end motions (`e`, `ge`, …) are inclusive: extend
                // the range past the covered character.
                let (s, e) = match eff_edge {
                    WordEdge::Start => {
                        let mut e = pre.max(target);
                        // `w` that finds no next word falls back to the last
                        // character; the operator includes it (`dw` on
                        // trailing whitespace deletes it, like vim).
                        if dir == WordDir::Forward && no_word_after(app, pre, big) {
                            e += char_len_at(app, e);
                        }
                        (pre.min(target), e)
                    }
                    WordEdge::End if dir == WordDir::Forward => {
                        (pre, target + char_len_at(app, target))
                    }
                    WordEdge::End => (target, pre + char_len_at(app, pre)),
                };
                let restore = if s == pre { Some((pre_x, pre_y)) } else { None };
                self.apply_range(app, op, s, e, false, restore);
                if s != pre {
                    // Backward motion: the cursor ends at the start of the
                    // changed/yanked text (vim); a forward motion is
                    // already there via `restore`.
                    place_cursor_at_byte(app, s);
                    vim_clamp_to_char(app);
                }
            }
            Motion::TextObject(kind) => {
                if let Some((s, e)) = text_object_range(app, kind, count) {
                    self.apply_range(app, op, s, e, false, None);
                    // Like vim, the cursor ends at the start of the
                    // yanked/changed text.
                    place_cursor_at_byte(app, s);
                    if op != ModalOperator::Yank {
                        vim_clamp_to_char(app);
                    }
                }
            }
            Motion::OpenBelow | Motion::OpenAbove => {
                self.open_line(app, motion == Motion::OpenBelow);
            }
        }
    }

    /// `d}`/`d{`/`y}`/`y{`/`c}`/`c{`: vim's exclusive paragraph motion
    /// with its linewise promotion, probed against vim 9.1 (2026-10-15).
    ///
    /// The plain-motion target is always at column 0 (an empty line) or
    /// on the last byte of the buffer. Starting from column 0, the
    /// operation is linewise; otherwise charwise:
    ///
    /// - forward, target at column 0: `[s, t)` pulls back to `[s, t-1)`
    ///   so the newline survives;
    /// - forward, target at end-of-buffer: inclusive (`[s, t+1)`), except
    ///   yank from column 0, which stays exclusive (`[s, t)`);
    /// - backward: `[t, s)`, no adjustment.
    ///
    /// Afterwards the cursor is on the start of the changed text (for
    /// yank, it does not move).
    fn apply_paragraph_operator(
        &mut self,
        app: &mut EditorApp,
        op: ModalOperator,
        forward: bool,
        count: usize,
    ) {
        let (pre, pre_x, pre_y, total) = match app.active_buffer() {
            Some(_) => {
                let (x, y) = cursor_xy(app);
                (cursor_byte(app), x, y, app.active_buffer().map(|b| b.len()).unwrap_or(0))
            }
            None => return,
        };
        // Walk the motion `count` times from the cursor line.
        let t = {
            let b = match app.active_buffer() {
                Some(b) => b,
                None => return,
            };
            let mut line = pre_y;
            let mut target = pre;
            for _ in 0..count.max(1) {
                match paragraph_target_from(app, b, line, forward) {
                    Some(nt) => {
                        let nline = b.byte_to_line(nt);
                        if nt == target {
                            break;
                        }
                        target = nt;
                        line = nline;
                    }
                    None => break,
                }
            }
            target
        };
        if t == pre {
            return;
        }
        let t_line = app
            .active_buffer()
            .map(|b| b.byte_to_line(t))
            .unwrap_or(pre_y);
        let t_at_eob = total > 0 && t == total - 1;
        let t_at_col0 = !t_at_eob
            && app
                .active_buffer()
                .and_then(|b| b.line_to_byte(t_line))
                == Some(t);
        // Linewise promotion: from column 0 with the target at column 0,
        // or (for delete/change) with the target at end-of-buffer.
        let linewise =
            pre_x == 0 && (t_at_col0 || (t_at_eob && op != ModalOperator::Yank));
        if linewise {
            let (lo, hi) = if forward {
                if t_at_eob {
                    (pre_y.min(t_line), pre_y.max(t_line))
                } else {
                    let lo = pre_y.min(t_line);
                    (lo, pre_y.max(t_line).saturating_sub(1).max(lo))
                }
            } else {
                let lo = pre_y.min(t_line);
                (lo, pre_y.max(t_line).saturating_sub(1).max(lo))
            };
            self.apply_linewise_range(app, op, lo, hi);
            return;
        }
        // Charwise.
        if forward {
            let e = if t_at_eob { t + 1 } else { t.saturating_sub(1).max(pre) };
            self.apply_range(app, op, pre, e, false, None);
            if op != ModalOperator::Yank {
                place_cursor_at_byte(app, pre);
                vim_clamp_to_char(app);
            }
        } else {
            self.apply_range(app, op, t, pre, false, None);
            if op != ModalOperator::Yank {
                place_cursor_at_byte(app, t);
                vim_clamp_to_char(app);
            }
        }
    }

    /// The core edit: store text in the register, then delete / yank / change
    /// the byte range `[s, e)`. Cursor placement: `restore` is the pre-motion
    /// cursor position used when the range starts there (forward motions).
    fn apply_range(
        &mut self,
        app: &mut EditorApp,
        op: ModalOperator,
        s: usize,
        e: usize,
        linewise: bool,
        restore: Option<(usize, usize)>,
    ) {
        if s == e && !linewise {
            clear_mark(app);
            return;
        }
        let text = app
            .active_buffer()
            .map(|b| b.get_range_as_string(s, e.saturating_sub(s)))
            .unwrap_or_default();
        self.store_register(text, linewise);
        match op {
            ModalOperator::Delete => {
                if let Some(buffer) = app.active_buffer_mut() {
                    buffer.delete(s, e.saturating_sub(s));
                }
                if let Some((x, y)) = restore
                    && let Some(window) = app.active_window_mut()
                {
                    window.cursor_x = x;
                    window.cursor_y = y;
                }
                clear_mark(app);
                clamp_cursor(app);
                ensure_visible(app);
            }
            ModalOperator::Yank => {
                // The text is in the register; the buffer is untouched and the
                // cursor stays at the motion's end. Just drop the mark.
                clear_mark(app);
            }
            ModalOperator::Change => {
                if let Some(buffer) = app.active_buffer_mut() {
                    buffer.delete(s, e.saturating_sub(s));
                }
                if let Some((x, y)) = restore
                    && let Some(window) = app.active_window_mut()
                {
                    window.cursor_x = x;
                    window.cursor_y = y;
                }
                clear_mark(app);
                clamp_cursor(app);
                ensure_visible(app);
                // The caller enters insert mode with the change record.
            }
        }
    }

    /// Linewise `dd` / `yy` / `cc` over `count` lines from the cursor line.
    fn apply_linewise(&mut self, app: &mut EditorApp, op: ModalOperator, count: usize) {
        let y = app.active_window_ref().map(|w| w.cursor_y).unwrap_or(0);
        let n = app.active_buffer().map(|b| b.line_count()).unwrap_or(0);
        if y >= n {
            return;
        }
        let hi = (y + count).saturating_sub(1).min(n.saturating_sub(1));
        self.apply_linewise_range(app, op, y, hi);
    }

    /// Linewise `op` over the inclusive line range `[lo_y, hi_y]`
    /// (`dgg`, `dG`). The shared tail of `apply_linewise`.
    fn apply_linewise_range(
        &mut self,
        app: &mut EditorApp,
        op: ModalOperator,
        lo_y: usize,
        hi_y: usize,
    ) {
        let (n, total) = match app.active_buffer() {
            Some(b) => (b.line_count(), b.len()),
            None => return,
        };
        if lo_y >= n {
            return;
        }
        let hi_y = hi_y.min(n.saturating_sub(1));
        let last = hi_y + 1;
        let mut first_byte = app
            .active_buffer()
            .and_then(|b| b.line_to_byte(lo_y))
            .unwrap_or(0);
        let mut end_byte = app
            .active_buffer()
            .and_then(|b| {
                if last < n {
                    b.line_to_byte(last)
                } else {
                    Some(total)
                }
            })
            .unwrap_or(total);
        // `cc`/`cG` change the lines but keep an empty line behind: the
        // trailing newline is not deleted (vim). The register still gets
        // the full linewise text.
        let reg_end_byte = end_byte;
        if op == ModalOperator::Change {
            end_byte = app
                .active_buffer()
                .and_then(|b| {
                    let ls = b.line_to_byte(hi_y)?;
                    Some(ls + b.line(hi_y).map(|t| t.len()).unwrap_or(0))
                })
                .unwrap_or(end_byte)
                .min(end_byte);
        }
        // Deleting the final line(s) of a file without trailing newline:
        // swallow the preceding newline so no empty line is left behind.
        // (Delete only; Change keeps the line.)
        if op != ModalOperator::Change && end_byte == total && total > 0 && lo_y > 0 {
            let ends_with_nl = app
                .active_buffer()
                .and_then(|b| b.line(n.saturating_sub(1)))
                .map(|_| {
                    app.active_buffer()
                        .map(|b| b.get_range_as_string(total.saturating_sub(1), 1))
                        .unwrap_or_default()
                        == "\n"
                })
                .unwrap_or(false);
            if !ends_with_nl {
                first_byte = first_byte.saturating_sub(1);
                end_byte = total;
            }
        }
        let text = app
            .active_buffer()
            .map(|b| b.get_range_as_string(first_byte, reg_end_byte.saturating_sub(first_byte)))
            .unwrap_or_default();
        self.store_register(text, true);
        match op {
            ModalOperator::Delete | ModalOperator::Change => {
                if let Some(buffer) = app.active_buffer_mut() {
                    buffer.delete(first_byte, end_byte.saturating_sub(first_byte));
                }
                let new_n = app.active_buffer().map(|b| b.line_count()).unwrap_or(1);
                if let Some(window) = app.active_window_mut() {
                    window.cursor_y = lo_y.min(new_n.saturating_sub(1));
                    window.cursor_x = 0;
                }
                clear_mark(app);
                dispatch(app, Command::SmartHome, 1);
                ensure_visible(app);
            }
            ModalOperator::Yank => {
                dispatch(app, Command::SmartHome, 1);
            }
        }
    }

    /// `p` / `P`: put register text after / before the cursor.
    fn do_put(&mut self, app: &mut EditorApp, before: bool) -> ModalOutcome {
        let reg = match self.get_register() {
            Some(r) => r.clone(),
            None => {
                app.message = Some("Nothing in register".to_string());
                return ModalOutcome::Consumed;
            }
        };
        if reg.text.is_empty() {
            return ModalOutcome::Consumed;
        }
        if reg.linewise {
            let y = app.active_window_ref().map(|w| w.cursor_y).unwrap_or(0);
            let (ins_at, text) = self.linewise_put_text(app, y, before, &reg.text);
            if let Some(buffer) = app.active_buffer_mut() {
                buffer.insert(ins_at, &text);
            }
            if let Some(window) = app.active_window_mut() {
                window.cursor_y = if before { y } else { y + 1 };
                window.cursor_x = 0;
            }
            clear_mark(app);
            dispatch(app, Command::SmartHome, 1);
            ensure_visible(app);
            return ModalOutcome::Consumed;
        }
        // Charwise put.
        let pos = cursor_byte(app);
        let ins_at = if before {
            pos
        } else {
            // After the grapheme cluster under the cursor. Clusters
            // (ZWJ sequences, combining marks) exceed 4 bytes and span
            // multiple chars; a fixed 4-byte peek splits them.
            let total = app.active_buffer().map(|b| b.len()).unwrap_or(0);
            if pos < total {
                let cluster_len = app
                    .active_buffer()
                    .and_then(|b| {
                        let line_idx = b.byte_to_line(pos);
                        let line_start = b.line_to_byte(line_idx)?;
                        let line = b.line(line_idx)?;
                        let rel = pos.saturating_sub(line_start);
                        crate::core::utf8::GraphemeIterator::new(&line)
                            .scan(0usize, |offset, g| {
                                let start = *offset;
                                *offset += g.len();
                                Some((start, g.len()))
                            })
                            .find(|(start, _)| *start >= rel)
                            .map(|(_, len)| len)
                    })
                    .unwrap_or(0);
                pos + cluster_len
            } else {
                pos
            }
        };
        if let Some(buffer) = app.active_buffer_mut() {
            buffer.insert(ins_at, &reg.text);
        }
        // The cursor lands on the last grapheme cluster of the put
        // text (vim), for both `p` and `P`.
        let last_start = ins_at
            + crate::core::utf8::GraphemeIterator::new(&reg.text)
                .scan(0usize, |offset, g| {
                    let start = *offset;
                    *offset += g.len();
                    Some(start)
                })
                .last()
                .unwrap_or(0);
        place_cursor_at_byte(app, last_start);
        clear_mark(app);
        ensure_visible(app);
        ModalOutcome::Consumed
    }

    /// Compute the insertion point and text for a linewise put.
    fn linewise_put_text(
        &self,
        app: &EditorApp,
        y: usize,
        before: bool,
        text: &str,
    ) -> (usize, String) {
        let buffer = match app.active_buffer() {
            Some(b) => b,
            None => return (0, String::new()),
        };
        let n = buffer.line_count();
        let total = buffer.len();
        if before {
            let at = buffer.line_to_byte(y.min(n.saturating_sub(1))).unwrap_or(0);
            (at, text.to_string())
        } else {
            let last = (y + 1).min(n);
            match buffer.line_to_byte(last) {
                Some(at) => (at, text.to_string()),
                None => {
                    // Past the last line: if the buffer ends with a newline
                    // the text slots in as-is; otherwise it needs a leading
                    // newline so no line is joined.
                    let ends_nl = buffer
                        .get_range_as_string(total.saturating_sub(1), 1)
                        == "\n";
                    if ends_nl || total == 0 {
                        (total, text.to_string())
                    } else {
                        (total, format!("\n{}", text.trim_end_matches('\n')))
                    }
                }
            }
        }
    }

    /// `o` / `O`: open a line below / above via the Buffer API (one undo unit).
    fn open_line(&mut self, app: &mut EditorApp, below: bool) {
        let y = app.active_window_ref().map(|w| w.cursor_y).unwrap_or(0);
        let (at, new_y) = {
            let buffer = match app.active_buffer() {
                Some(b) => b,
                None => return,
            };
            if below {
                (line_end_byte(app, cursor_byte(app)), y + 1)
            } else {
                (
                    buffer.line_to_byte(y).unwrap_or(0),
                    y,
                )
            }
        };
        if let Some(buffer) = app.active_buffer_mut() {
            buffer.insert(at, "\n");
        }
        if let Some(window) = app.active_window_mut() {
            window.cursor_y = new_y;
            window.cursor_x = 0;
        }
        clear_mark(app);
        ensure_visible(app);
    }

    /// `.`: replay the last recorded change.
    fn replay_change(&mut self, app: &mut EditorApp, change: &ModalChange) -> ModalOutcome {
        match change {
            ModalChange::Insert { text } => {
                let pos = cursor_byte(app);
                if let Some(buffer) = app.active_buffer_mut() {
                    buffer.insert(pos, text);
                }
                // The cursor ends after the inserted text, as if typed.
                let end = pos + text.len();
                move_cursor_to_byte(app, end);
                ensure_visible(app);
                ModalOutcome::Consumed
            }
            ModalChange::Operator {
                op,
                motion,
                count,
                inserted,
            } => {
                let motion = *motion;
                let count = *count;
                let inserted = inserted.clone();
                let op = *op;
                self.apply_operator(app, op, motion, count);
                if op == ModalOperator::Change && !inserted.is_empty() {
                    let pos = cursor_byte(app);
                    if let Some(buffer) = app.active_buffer_mut() {
                        buffer.insert(pos, &inserted);
                    }
                    // Vim leaves the cursor on the last inserted
                    // character for a repeated change (cf. the plain
                    // Insert arm, which moves past the text).
                    let last_start = pos
                        + crate::core::utf8::GraphemeIterator::new(&inserted)
                            .scan(0usize, |offset, g| {
                                let start = *offset;
                                *offset += g.len();
                                Some(start)
                            })
                            .last()
                            .unwrap_or(0);
                    move_cursor_to_byte(app, last_start);
                    ensure_visible(app);
                }
                // The replay itself becomes the repeatable change.
                self.last_change = Some(change.clone());
                ModalOutcome::Consumed
            }
        }
    }

    // -- registers ----------------------------------------------------------

    fn store_register(&mut self, text: String, linewise: bool) {
        let reg = Register { text, linewise };
        if self.register == '"' {
            self.unnamed = Some(reg);
        } else {
            if let Some(i) = register_index(self.register) {
                self.named[i] = Some(reg.clone());
            }
            self.unnamed = Some(reg);
        }
    }

    fn get_register(&self) -> Option<&Register> {
        if self.register == '"' {
            self.unnamed.as_ref()
        } else {
            register_index(self.register)
                .and_then(|i| self.named[i].as_ref())
                .or(self.unnamed.as_ref())
        }
    }
}

/// Clamp the cursor onto a character: vim's Normal mode never rests
/// past the last character of a line.
fn vim_clamp_to_char(app: &mut EditorApp) {
    let id = app.active_window;
    let (buffer_id, x, y) = match app.windows.get(&id) {
        Some(w) => (w.buffer_id, w.cursor_x, w.cursor_y),
        None => return,
    };
    let max_x = match app.buffers.get(&buffer_id) {
        Some(b) => {
            crate::core::utf8::grapheme_count(&b.line(y).unwrap_or_default()).saturating_sub(1)
        }
        None => return,
    };
    if x > max_x
        && let Some(window) = app.windows.get_mut(&id)
    {
        window.cursor_x = max_x;
    }
    ensure_visible(app);
}

// -- free helpers -----------------------------------------------------------

fn op_char(op: ModalOperator) -> char {
    match op {
        ModalOperator::Delete => 'd',
        ModalOperator::Yank => 'y',
        ModalOperator::Change => 'c',
    }
}

fn flip_dir(dir: FindDir) -> FindDir {
    match dir {
        FindDir::Forward => FindDir::Backward,
        FindDir::Backward => FindDir::Forward,
    }
}

/// Named register index: a-z -> 0-25, 0-9 -> 26-35.
fn register_index(c: char) -> Option<usize> {
    match c {
        'a'..='z' => Some((c as usize) - ('a' as usize)),
        '0'..='9' => Some(26 + (c as usize) - ('0' as usize)),
        _ => None,
    }
}

fn cursor_xy(app: &EditorApp) -> (usize, usize) {
    app.active_window_ref()
        .map(|w| (w.cursor_x, w.cursor_y))
        .unwrap_or((0, 0))
}

/// Byte offset of the cursor in the active buffer.
/// Move the active window's cursor to a buffer byte offset.
fn move_cursor_to_byte(app: &mut EditorApp, byte: usize) {
    let window_id = app.active_window;
    let buffer_id = match app.windows.get(&window_id) {
        Some(w) => w.buffer_id,
        None => return,
    };
    // Disjoint field borrows: the window and its buffer.
    if let (Some(window), Some(buffer)) = (
        app.windows.get_mut(&window_id),
        app.buffers.get(&buffer_id),
    ) {
        window.set_cursor_to_byte(buffer, byte);
    }
}

fn cursor_byte(app: &EditorApp) -> usize {
    let window = match app.active_window_ref() {
        Some(w) => w,
        None => return 0,
    };
    let buffer = match app.active_buffer() {
        Some(b) => b,
        None => return 0,
    };
    window.get_byte_offset(buffer).unwrap_or(0)
}

/// Byte offset of the mark, via a temporary window like `kill_region` does.
fn mark_byte(app: &EditorApp) -> Option<usize> {
    let window = app.active_window_ref()?;
    window.selection_manager.mark
}

/// Normalized (start, end) byte range of mark..cursor.
fn selection_bytes(app: &EditorApp) -> Option<(usize, usize)> {
    let m = mark_byte(app)?;
    let c = cursor_byte(app);
    Some((m.min(c), m.max(c)))
}

fn clear_mark(app: &mut EditorApp) {
    if let Some(window) = app.active_window_mut() {
        window.selection_manager.clear_mark();
    }
}

fn ensure_visible(app: &mut EditorApp) {
    let id = app.active_window;
    let buffer_id = match app.windows.get(&id) {
        Some(w) => w.buffer_id,
        None => return,
    };
    if let Some(buffer) = app.buffers.get(&buffer_id)
        && let Some(window) = app.windows.get_mut(&id)
    {
        window.ensure_cursor_visible(buffer);
    }
}

/// Clamp the cursor onto the buffer after an edit shrank the line.
fn clamp_cursor(app: &mut EditorApp) {
    let id = app.active_window;
    let (x, y) = match app.windows.get(&id) {
        Some(w) => (w.cursor_x, w.cursor_y),
        None => return,
    };
    let buffer_id = match app.windows.get(&id) {
        Some(w) => w.buffer_id,
        None => return,
    };
    let max_x = app
        .buffers
        .get(&buffer_id)
        .and_then(|b| b.line(y))
        .map(|line| crate::core::utf8::byte_to_grapheme_col(&line, line.len()))
        .unwrap_or(0);
    if x > max_x
        && let Some(window) = app.windows.get_mut(&id)
    {
        window.cursor_x = max_x;
    }
}

/// Byte offset of the end of the line containing `byte` (excluding newline).
fn line_end_byte(app: &EditorApp, byte: usize) -> usize {
    let buffer = match app.active_buffer() {
        Some(b) => b,
        None => return byte,
    };
    let line_idx = buffer.byte_to_line(byte);
    let start = buffer.line_to_byte(line_idx).unwrap_or(0);
    let text = buffer.line(line_idx).unwrap_or_default();
    start + text.len()
}

/// Byte offset `count` characters forward from `pos`, clamped to the line end.
fn forward_chars_byte(app: &EditorApp, pos: usize, count: usize) -> usize {
    let buffer = match app.active_buffer() {
        Some(b) => b,
        None => return pos,
    };
    let line_end = line_end_byte(app, pos);
    let head = buffer.get_range_as_string(pos, line_end.saturating_sub(pos));
    let mut byte = pos;
    for c in head.chars().take(count) {
        byte += c.len_utf8();
    }
    byte.min(line_end)
}

/// Byte offset `count` characters backward from `pos`.
fn backward_chars_byte(app: &EditorApp, pos: usize, count: usize) -> usize {
    let buffer = match app.active_buffer() {
        Some(b) => b,
        None => return pos,
    };
    let start = buffer.line_to_byte(buffer.byte_to_line(pos)).unwrap_or(0);
    let head = buffer.get_range_as_string(start, pos.saturating_sub(start));
    let chars: Vec<(usize, char)> = head.char_indices().collect();
    if chars.is_empty() {
        return pos;
    }
    let idx = chars.len().saturating_sub(count);
    start + chars.get(idx).map(|(o, _)| *o).unwrap_or(0)
}

/// Delete the characters in byte range `[s, e)`, one at a time at `s`.
/// The count is derived from the range itself, so this always terminates.
fn delete_char_range(app: &mut EditorApp, s: usize, e: usize) -> String {
    let text = app
        .active_buffer()
        .map(|b| b.get_range_as_string(s, e.saturating_sub(s)))
        .unwrap_or_default();
    let n = text.chars().count();
    if let Some(buffer) = app.active_buffer_mut() {
        for _ in 0..n {
            buffer.delete_char(s);
        }
    }
    text
}

/// Delete up to `count` characters forward from `pos`, never crossing the
/// line end (`x`/`s` stop at the newline, like vim). The range is clamped
/// before deleting, so huge counts (`999999999x`) terminate instead of
/// spinning on a no-op past the end. Returns the deleted text.
fn delete_chars_forward(app: &mut EditorApp, pos: usize, count: usize) -> String {
    delete_char_range(app, pos, forward_chars_byte(app, pos, count))
}

/// Expand a byte range to whole lines (for linewise visual mode).
fn expand_to_lines(app: &EditorApp, s: usize, e: usize) -> (usize, usize) {
    let buffer = match app.active_buffer() {
        Some(b) => b,
        None => return (s, e),
    };
    let first = buffer.byte_to_line(s);
    let last = buffer.byte_to_line(e.saturating_sub(1).max(s));
    let n = buffer.line_count();
    let start = buffer.line_to_byte(first).unwrap_or(s);
    let end = if last + 1 < n {
        buffer.line_to_byte(last + 1).unwrap_or(e)
    } else {
        buffer.len()
    };
    (start, end)
}

/// Place the cursor on a byte offset (same line; used by find-char motions).
fn place_cursor_at_byte(app: &mut EditorApp, byte: usize) {
    let (line_idx, col) = match app.active_buffer() {
        Some(b) => b.byte_to_position(byte),
        None => return,
    };
    if let Some(window) = app.active_window_mut() {
        window.cursor_y = line_idx;
        window.cursor_x = col;
    }
    ensure_visible(app);
}

/// Vim `$`: the cursor onto the last character of the current line
/// (column 0 on an empty line). The shared end-of-line stops one past
/// the last character, which is not a Normal-mode cursor.
fn vim_line_end(app: &mut EditorApp) {
    let id = app.active_window;
    let (buffer_id, y) = match app.windows.get(&id) {
        Some(w) => (w.buffer_id, w.cursor_y),
        None => return,
    };
    let x = match app.buffers.get(&buffer_id) {
        Some(b) => {
            crate::core::utf8::grapheme_count(&b.line(y).unwrap_or_default()).saturating_sub(1)
        }
        None => return,
    };
    if let Some(window) = app.windows.get_mut(&id)
        && let Some(buffer) = app.buffers.get(&buffer_id)
    {
        window.cursor_x = x;
        window.update_visual_cursor(buffer);
        window.ensure_cursor_visible(buffer);
    }
}

/// 0-based index of vim's last line: the last content line. A trailing
/// newline is not a vim line (the buffer `a\n` has one vim line, not two).
fn vim_last_line(app: &EditorApp) -> Option<usize> {
    let b = app.active_buffer()?;
    let n = b.line_count();
    if n == 0 {
        return None;
    }
    let total = b.len();
    let ends_nl = total > 0 && b.get_range_as_string(total - 1, 1) == "\n";
    Some(
        n.saturating_sub(1)
            .saturating_sub(usize::from(ends_nl && n > 1)),
    )
}

/// Byte offset of the last character of a line (the line start itself
/// for an empty line).
fn line_last_char_byte(b: &Buffer, line: usize) -> usize {
    let ls = b.line_to_byte(line).unwrap_or(0);
    let text = b.line(line).unwrap_or_default();
    match text.chars().next_back() {
        Some(c) => ls + text.len() - c.len_utf8(),
        None => ls,
    }
}

/// Vim `}`/`{` target as a byte offset, probed against vim 9.1
/// (2026-10-15). Forward: step off a blank line, then go to the next
/// empty line strictly after (column 0); with none, to the last
/// character of the last content line. Backward mirrors it. A line
/// counts as blank only when truly empty — whitespace-only lines are
/// not paragraph boundaries.
fn vim_paragraph_target(app: &EditorApp, forward: bool) -> Option<usize> {
    let b = app.active_buffer()?;
    let n = b.line_count();
    if n == 0 {
        return None;
    }
    let y = app
        .active_window_ref()
        .map(|w| w.cursor_y)
        .unwrap_or(0)
        .min(n - 1);
    paragraph_target_from(app, b, y, forward)
}

/// The line-based core of [`vim_paragraph_target`]: the `}`/`{` target
/// from line `y`.
fn paragraph_target_from(
    app: &EditorApp,
    b: &Buffer,
    y: usize,
    forward: bool,
) -> Option<usize> {
    let n = b.line_count();
    let is_blank = |l: usize| b.line(l).map(|t| t.is_empty()).unwrap_or(true);
    if forward {
        let i = if is_blank(y) { y + 1 } else { y };
        let mut l = i + 1;
        while l < n {
            if is_blank(l) {
                return b.line_to_byte(l);
            }
            l += 1;
        }
        let last = vim_last_line(app)?;
        Some(line_last_char_byte(b, last))
    } else {
        let i = y.saturating_sub(usize::from(is_blank(y)));
        let mut l = i.saturating_sub(1);
        loop {
            if is_blank(l) {
                return b.line_to_byte(l);
            }
            if l == 0 {
                break;
            }
            l -= 1;
        }
        b.line_to_byte(0)
    }
}

/// Vim `G`: to the last content line — a trailing newline is not a vim
/// line — on the first non-blank (vim's `startofline`). `raw` is the
/// pending count with 0 meaning "none": `5G` goes to line 5.
fn vim_file_end(app: &mut EditorApp, raw: usize) {
    let id = app.active_window;
    let buffer_id = match app.windows.get(&id) {
        Some(w) => w.buffer_id,
        None => return,
    };
    let last = match vim_last_line(app) {
        Some(l) => l,
        None => return,
    };
    let y = if raw == 0 {
        last
    } else {
        raw.saturating_sub(1).min(last)
    };
    let x = match app.buffers.get(&buffer_id) {
        Some(b) => {
            let text = b.line(y).unwrap_or_default();
            text.char_indices()
                .find(|(_, c)| !c.is_whitespace())
                .map(|(i, _)| crate::core::utf8::byte_to_grapheme_col(&text, i))
                .unwrap_or(0)
        }
        None => return,
    };
    if let Some(window) = app.windows.get_mut(&id)
        && let Some(buffer) = app.buffers.get(&buffer_id)
    {
        window.cursor_y = y;
        window.cursor_x = x;
        window.update_visual_cursor(buffer);
        window.ensure_cursor_visible(buffer);
    }
}

/// Vim word class: 0 = blank, 1 = word char (alnum or `_`), 2 = punctuation
/// (any other non-blank). `w`/`b` move between word starts where a "word" is
/// a run of class 1 or a run of class 2.
fn word_class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

/// Word spans of one line: maximal runs of word characters as
/// `(start, end)` char indices (end exclusive). Small words group by
/// [`word_class`]; big WORDs group any non-blank run.
///
/// This is the single word-boundary primitive: `w`/`b`/`e`/`ge` and
/// `W`/`B`/`E`/`gE` are all extremal span boundaries relative to the
/// cursor, so the eight motions cannot drift apart.
fn word_spans(line: &[char], big: bool) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < line.len() {
        if word_class(line[i]) == 0 {
            i += 1;
            continue;
        }
        let start = i;
        if big {
            while i < line.len() && word_class(line[i]) != 0 {
                i += 1;
            }
        } else {
            let cls = word_class(line[i]);
            while i < line.len() && word_class(line[i]) == cls {
                i += 1;
            }
        }
        spans.push((start, i));
    }
    spans
}

/// Char length of buffer line `li`.
fn line_char_len(buffer: &Buffer, li: usize) -> usize {
    buffer.line(li).map(|t| t.chars().count()).unwrap_or(0)
}

/// Word spans of buffer line `li`.
fn spans_of(buffer: &Buffer, li: usize, big: bool) -> Vec<(usize, usize)> {
    let text = buffer.line(li).unwrap_or_default();
    word_spans(&text.chars().collect::<Vec<_>>(), big)
}

/// `(line, spans)` of the nearest line at or after `from` that has a word.
fn next_line_with_spans(
    buffer: &Buffer,
    from: usize,
    n_lines: usize,
    big: bool,
) -> Option<(usize, Vec<(usize, usize)>)> {
    let mut li = from;
    while li < n_lines {
        let spans = spans_of(buffer, li, big);
        if !spans.is_empty() {
            return Some((li, spans));
        }
        li += 1;
    }
    None
}

/// `(line, spans)` of the nearest line at or before `from` that has a word.
fn prev_line_with_spans(
    buffer: &Buffer,
    from: usize,
    big: bool,
) -> Option<(usize, Vec<(usize, usize)>)> {
    let mut li = from as isize;
    while li >= 0 {
        let spans = spans_of(buffer, li as usize, big);
        if !spans.is_empty() {
            return Some((li as usize, spans));
        }
        li -= 1;
    }
    None
}

/// Byte target of a vim word motion, over the one shared primitive.
///
/// - `w`/`W` (Forward/Start): smallest word start strictly after the
///   cursor — exclusive. Never fails: at end of buffer it rests on the
///   last line's end, like vim's `w`.
/// - `e`/`E` (Forward/End): smallest word end strictly after the cursor —
///   inclusive. Returns `None` past the last word (vim beeps).
/// - `b`/`B` (Backward/Start): greatest word start strictly before the
///   cursor — exclusive. Never fails; rests at buffer start.
/// - `ge`/`gE` (Backward/End): greatest word end strictly before the
///   cursor — inclusive. No movement at buffer start.
///
/// Byte offset of the last character of the word containing `byte`
/// (`cw` with count 1 stops at the current word's end without advancing
/// like `ce` does). Returns `None` when `byte` is not on a word.
fn current_word_end(app: &EditorApp, byte: usize, big: bool) -> Option<usize> {
    let b = app.active_buffer()?;
    let li = b.byte_to_line(byte);
    let ls = b.line_to_byte(li)?;
    let text = b.line(li)?;
    let ci = text.get(..byte.saturating_sub(ls)).map(|s| s.chars().count())?;
    let spans = spans_of(b, li, big);
    let span = spans.iter().find(|s| s.0 <= ci && ci < s.1)?;
    let mut out = ls;
    for c in text.chars().take(span.1 - 1) {
        out += c.len_utf8();
    }
    Some(out)
}

/// Whether a forward word-start motion from `byte` finds no next word
/// (so `w` falls back to the last character).
fn no_word_after(app: &EditorApp, byte: usize, big: bool) -> bool {
    let b = match app.active_buffer() {
        Some(b) => b,
        None => return true,
    };
    let li = b.byte_to_line(byte);
    let ls = b.line_to_byte(li).unwrap_or(0);
    let text = b.line(li).unwrap_or_default();
    let ci = text
        .get(..byte.saturating_sub(ls))
        .map(|s| s.chars().count())
        .unwrap_or(0);
    if spans_of(b, li, big).iter().any(|s| s.0 > ci) {
        return false;
    }
    next_line_with_spans(b, li + 1, b.line_count(), big).is_none()
}

/// Each count step moves monotonically and stops at the first step that
/// makes no progress, so huge counts (`999999999w`) terminate in
/// O(words), not O(count).
fn vim_word_edge_target(
    app: &EditorApp,
    dir: WordDir,
    edge: WordEdge,
    big: bool,
    count: usize,
) -> Option<usize> {
    let buffer = app.active_buffer()?;
    let pos = cursor_byte(app);
    let n_lines = buffer.line_count();
    if n_lines == 0 {
        return None;
    }
    let mut li = buffer.byte_to_line(pos);
    let line_start = buffer.line_to_byte(li).unwrap_or(0);
    let line_text = buffer.line(li).unwrap_or_default();
    let mut ci = line_text
        .get(..pos.saturating_sub(line_start))
        .map(|s| s.chars().count())
        .unwrap_or(0);
    let (li0, ci0) = (li, ci);

    for _ in 0..count.max(1) {
        let before = (li, ci);
        let progressed = match (dir, edge) {
            (WordDir::Forward, WordEdge::Start) => {
                match spans_of(buffer, li, big).iter().find(|s| s.0 > ci) {
                    Some(s) => {
                        ci = s.0;
                        true
                    }
                    None => match next_line_with_spans(buffer, li + 1, n_lines, big) {
                        Some((nl, ns)) => {
                            li = nl;
                            ci = ns[0].0;
                            true
                        }
                        // Vim's `w` never fails: rest on the last character
                        // of the last content line (not the phantom line,
                        // not past the end).
                        None => {
                            li = vim_last_line(app).unwrap_or(0);
                            ci = line_char_len(buffer, li).saturating_sub(1);
                            true
                        }
                    },
                }
            }
            (WordDir::Forward, WordEdge::End) => {
                match spans_of(buffer, li, big).iter().find(|s| s.1 - 1 > ci) {
                    Some(s) => {
                        ci = s.1 - 1;
                        true
                    }
                    None => match next_line_with_spans(buffer, li + 1, n_lines, big) {
                        Some((nl, ns)) => {
                            li = nl;
                            ci = ns[0].1 - 1;
                            true
                        }
                        None => false,
                    },
                }
            }
            (WordDir::Backward, WordEdge::Start) => {
                loop {
                    match spans_of(buffer, li, big).iter().rev().find(|s| s.0 < ci) {
                        Some(s) => {
                            ci = s.0;
                            break;
                        }
                        None if li > 0 => match prev_line_with_spans(buffer, li - 1, big) {
                            Some((nl, ns)) => {
                                li = nl;
                                ci = ns.last().map(|s| s.0).unwrap_or(0);
                                break;
                            }
                            // No word above: rescan line 0 from its end.
                            None => {
                                li = 0;
                                ci = usize::MAX;
                            }
                        },
                        None => {
                            ci = ci.min(line_char_len(buffer, li));
                            break;
                        }
                    }
                }
                true
            }
            (WordDir::Backward, WordEdge::End) => {
                let mut moved = false;
                match spans_of(buffer, li, big)
                    .iter()
                    .rev()
                    .find(|s| s.1 - 1 < ci)
                {
                    Some(s) => {
                        ci = s.1 - 1;
                        moved = true;
                    }
                    None if li > 0 => {
                        if let Some((nl, ns)) = prev_line_with_spans(buffer, li - 1, big) {
                            li = nl;
                            ci = ns.last().map(|s| s.1 - 1).unwrap_or(0);
                            moved = true;
                        }
                    }
                    None => {}
                }
                moved
            }
        };
        if !progressed || (li, ci) == before {
            break;
        }
    }

    // Forward/End past the last word is the only failing motion, and only
    // when no step moved at all (a later step failing is partial success).
    if dir == WordDir::Forward && edge == WordEdge::End && (li, ci) == (li0, ci0) {
        return None;
    }

    let start = buffer.line_to_byte(li).unwrap_or(0);
    let text = buffer.line(li).unwrap_or_default();
    let mut byte = start;
    for c in text.chars().take(ci) {
        byte += c.len_utf8();
    }
    Some(byte)
}

/// Byte length of the character starting at `byte` (0 past the end).
fn char_len_at(app: &EditorApp, byte: usize) -> usize {
    char_at(app, byte).map(|c| c.len_utf8()).unwrap_or(0)
}

/// The character starting at `byte` (`None` past the end).
fn char_at(app: &EditorApp, byte: usize) -> Option<char> {
    let buffer = app.active_buffer()?;
    let len = buffer.len();
    if byte >= len {
        return None;
    }
    buffer
        .get_range_as_string(byte, (len - byte).min(4))
        .chars()
        .next()
}

/// Set the visual mark on a buffer byte offset.
fn set_mark_at_byte(app: &mut EditorApp, byte: usize) {
    if let Some(window) = app.active_window_mut() {
        window.selection_manager.set_mark(byte);
    }
}

/// Byte offsets of each char in `chars`, plus a sentinel for the end.
fn char_byte_offsets(chars: &[char]) -> Vec<usize> {
    let mut off = Vec::with_capacity(chars.len() + 1);
    let mut b = 0;
    for c in chars {
        off.push(b);
        b += c.len_utf8();
    }
    off.push(b);
    off
}

/// Byte range `[s, e)` of a text object at the cursor, or `None` when no
/// object is found (unmatched bracket, no quotes on the line, …).
fn text_object_range(
    app: &EditorApp,
    kind: TextObjectKind,
    count: usize,
) -> Option<(usize, usize)> {
    match kind {
        TextObjectKind::Word { big, inner } => word_object_range(app, big, inner, count),
        TextObjectKind::Pair { open, inner } => pair_object_range(app, open, inner, count),
        TextObjectKind::Quote { q, inner } => quote_object_range(app, q, inner, count),
    }
}

/// `iw` / `aw` / `iW` / `aW`: line-restricted, like vim.
///
/// - On a word: `iw` is the word; `aw` is the word plus trailing
///   whitespace, or plus leading whitespace when the word is at end of
///   line. Count covers that many words.
/// - On whitespace: `iw` is the whitespace run; `aw` extends to the next
///   word (or the previous word when there is no next one).
fn word_object_range(
    app: &EditorApp,
    big: bool,
    inner: bool,
    count: usize,
) -> Option<(usize, usize)> {
    let buffer = app.active_buffer()?;
    let pos = cursor_byte(app);
    let li = buffer.byte_to_line(pos);
    let line_start = buffer.line_to_byte(li).unwrap_or(0);
    let line_text = buffer.line(li).unwrap_or_default();
    let chars: Vec<char> = line_text.chars().collect();
    let coff = char_byte_offsets(&chars);
    let ci = line_text
        .get(..pos.saturating_sub(line_start))
        .map(|s| s.chars().count())
        .unwrap_or(0);
    let spans = word_spans(&chars, big);
    let count = count.max(1);

    // The anchor word: the span containing the cursor — or, when the cursor
    // sits just past the line's last character (e.g. after `$`), the span
    // ending there.
    let anchor = spans
        .iter()
        .position(|s| s.0 <= ci && ci < s.1)
        .or_else(|| {
            (ci > 0 && ci == chars.len())
                .then(|| spans.iter().position(|s| s.0 < ci && ci - 1 < s.1))
                .flatten()
        });

    if let Some(idx) = anchor {
        let last = (idx + count - 1).min(spans.len() - 1);
        let (mut s, mut e) = (spans[idx].0, spans[last].1);
        if !inner {
            // `aw`: trailing whitespace; at end of line, leading instead.
            let mut te = e;
            while te < chars.len() && word_class(chars[te]) == 0 {
                te += 1;
            }
            if te > e {
                e = te;
            } else {
                while s > 0 && word_class(chars[s - 1]) == 0 {
                    s -= 1;
                }
            }
        }
        return Some((line_start + coff[s], line_start + coff[e]));
    }

    // On whitespace (or a wordless line).
    let mut ws = ci;
    while ws > 0 && word_class(chars[ws - 1]) == 0 {
        ws -= 1;
    }
    let mut we = ci;
    while we < chars.len() && word_class(chars[we]) == 0 {
        we += 1;
    }
    if ws == we {
        return None;
    }
    if inner {
        return Some((line_start + coff[ws], line_start + coff[we]));
    }
    // `aw` on whitespace: the run plus the following word (but not the
    // word's trailing whitespace, like vim). No following word: fail.
    if let Some(idx) = spans.iter().position(|s| s.0 >= we) {
        let last = (idx + count - 1).min(spans.len() - 1);
        let (s, e) = (spans[idx].0.min(ws), spans[last].1);
        Some((line_start + coff[s], line_start + coff[e]))
    } else {
        // No following word: `aw` on trailing whitespace fails (vim).
        // (`iw` already returned the whitespace run above.)
        None
    }
}

/// `i(`/`a(` (and `)`/`b` aliases), `i[`/`a[`, `i{`/`a{`, `i<`/`a<`:
/// the innermost bracket pair containing the cursor, buffer-wide like vim.
/// A count selects outward: `2i(` is the enclosing pair.
///
/// The scan runs on bytes through the buffer's chunk iterator, never
/// materializing the whole buffer: brackets are ASCII, so byte comparison
/// is exactly equivalent to char comparison, and a multi-byte char's
/// continuation bytes (>= 0x80) can never match, which also makes chunk
/// boundaries irrelevant. The backward search is a forward open-bracket
/// stack over [0, limit): pushing opens and popping closes leaves the
/// innermost still-open bracket on top, the same bracket the old
/// backward depth scan found. Owned leaves scan zero-copy; mapped leaves
/// copy once per visited leaf through the pager (unavoidable); a pager
/// failure ends the scan and yields no pair.
fn pair_object_range(
    app: &EditorApp,
    open: char,
    inner: bool,
    count: usize,
) -> Option<(usize, usize)> {
    let close = match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        _ => return None,
    };
    let (open_b, close_b) = (open as u8, close as u8);
    let buffer = app.active_buffer()?;
    let total = buffer.len();
    let mut cursor = cursor_byte(app).min(total);
    let mut result = None;
    for _ in 0..count.max(1) {
        // Byte under the cursor: a close bracket there is the pair's own
        // end, so the backward search excludes it (vim `di(` on `)`).
        // Anything else is included.
        let mut under = None;
        scan_bytes(buffer, cursor, cursor + 1, |_, b| {
            under = Some(b);
            false
        });
        let limit = if under == Some(close_b) {
            cursor
        } else {
            cursor + 1
        };

        // Backward search as a forward stack: the top on exit is the
        // innermost unclosed bracket at or before the cursor.
        let mut stack: Vec<usize> = Vec::new();
        scan_bytes(buffer, 0, limit, |off, b| {
            if b == open_b {
                stack.push(off);
            } else if b == close_b {
                stack.pop();
            }
            true
        });
        let mut j = stack.pop();
        if j.is_none() {
            // Forward fallback (vim): no unclosed bracket precedes the
            // cursor, so use the next opening bracket at or after it.
            scan_bytes(buffer, cursor, total, |off, b| {
                if b == open_b {
                    j = Some(off);
                    false
                } else {
                    true
                }
            });
        }
        let j = j?;
        // Forward: the bracket's match.
        let mut depth = 0usize;
        let mut kk = None;
        scan_bytes(buffer, j + 1, total, |off, b| {
            if b == open_b {
                depth += 1;
            } else if b == close_b {
                if depth == 0 {
                    kk = Some(off);
                    return false;
                }
                depth -= 1;
            }
            true
        });
        let kk = kk?;
        result = Some(if inner {
            (j + 1, kk)
        } else {
            (j, kk + 1)
        });
        if j == 0 {
            break;
        }
        // A count selects outward: the next search ends before this pair's
        // opening bracket.
        cursor = j - 1;
    }
    result
}

/// Run `f` over buffer bytes in `[start, end)`, in order, with absolute
/// offsets. Stops early when `f` returns false, and on a pager failure.
/// Never materializes more than the visited chunks.
fn scan_bytes(buffer: &Buffer, start: usize, end: usize, mut f: impl FnMut(usize, u8) -> bool) {
    let end = end.min(buffer.len());
    let mut base: usize = 0;
    for chunk in buffer.chunks() {
        let bytes = match chunk {
            Ok(c) => c,
            Err(_) => return,
        };
        let bytes = bytes.as_bytes();
        let chunk_start = base;
        base += bytes.len();
        if base <= start || chunk_start >= end {
            continue;
        }
        let lo = start.saturating_sub(chunk_start);
        let hi = (end - chunk_start).min(bytes.len());
        for (i, &b) in bytes[lo..hi].iter().enumerate() {
            if !f(chunk_start + lo + i, b) {
                return;
            }
        }
    }
}

/// `i"`/`a"`/`i'`/`a'`: the quote pair around the cursor on its line, like
/// vim. A quote under the cursor counts as the opening one; a count
/// selects outward (`2i"` is the enclosing pair). If no pair precedes the
/// cursor, the next pair forward is used (vim). `a"`/`a'` take trailing
/// whitespace, or leading when at end of line (like `aw`).
fn quote_object_range(
    app: &EditorApp,
    q: char,
    inner: bool,
    count: usize,
) -> Option<(usize, usize)> {
    let buffer = app.active_buffer()?;
    let pos = cursor_byte(app);
    let li = buffer.byte_to_line(pos);
    let line_start = buffer.line_to_byte(li).unwrap_or(0);
    let line_text = buffer.line(li).unwrap_or_default();
    let chars: Vec<char> = line_text.chars().collect();
    let boff = char_byte_offsets(&chars);
    let ci = line_text
        .get(..pos.saturating_sub(line_start))
        .map(|s| s.chars().count())
        .unwrap_or(0);

    let mut search = ci;
    let mut first = true;
    let mut result = None;
    for _ in 0..count.max(1) {
        // The opening quote: at the cursor, or the nearest one before it.
        // If none precedes, the next one forward (vim).
        let open: Option<usize> = if first && search < chars.len() && chars[search] == q {
            Some(search)
        } else if let Some(j) = (0..search).rev().find(|&j| chars[j] == q) {
            Some(j)
        } else {
            (search..chars.len()).find(|&j| chars[j] == q)
        };
        let o = open?;
        let close = ((o + 1)..chars.len()).find(|&k| chars[k] == q)?;
        // Char indices of the range (before byte conversion).
        let (cs, ce) = if inner {
            (o + 1, close)
        } else {
            (o, close + 1)
        };
        let (cs, ce) = if !inner {
            // `a"`/`a'`: trailing whitespace, or leading at end of line
            // (like `aw`, probed 2026-10-15).
            let mut te = ce;
            while te < chars.len() && chars[te].is_whitespace() {
                te += 1;
            }
            if te > ce {
                (cs, te)
            } else {
                let mut ts = cs;
                while ts > 0 && chars[ts - 1].is_whitespace() {
                    ts -= 1;
                }
                (ts, ce)
            }
        } else {
            (cs, ce)
        };
        // Convert char indices to byte offsets.
        let s = boff.get(cs).copied().unwrap_or(line_text.len());
        let e = boff.get(ce).copied().unwrap_or(line_text.len());
        result = Some((line_start + s, line_start + e));
        first = false;
        if o == 0 {
            break;
        }
        search = o;
    }
    result
}

/// Target byte of a find-char motion on the cursor's line, or `None` when the
/// character is not found (the motion then does nothing).
fn find_char_target(
    app: &EditorApp,
    dir: FindDir,
    edge: FindEdge,
    ch: char,
    count: usize,
) -> Option<usize> {
    let buffer = app.active_buffer()?;
    let pos = cursor_byte(app);
    let line_idx = buffer.byte_to_line(pos);
    let line_start = buffer.line_to_byte(line_idx)?;
    let line = buffer.line(line_idx)?;
    let col = pos.saturating_sub(line_start);

    // (byte index in line, char) in search order.
    let mut found: Option<usize> = None;
    let mut remaining = count.max(1);
    match dir {
        FindDir::Forward => {
            for (i, c) in line.char_indices() {
                if i <= col || c != ch {
                    continue;
                }
                remaining -= 1;
                if remaining == 0 {
                    found = Some(i);
                    break;
                }
            }
        }
        FindDir::Backward => {
            let mut indices: Vec<(usize, char)> = line
                .char_indices()
                .filter(|(i, c)| *i < col && *c == ch)
                .collect();
            // char_indices is ascending; take from the end for nearest-first.
            while remaining > 0 && !indices.is_empty() {
                let (i, _) = indices.pop().unwrap_or((0, '\0'));
                remaining -= 1;
                if remaining == 0 {
                    found = Some(i);
                    break;
                }
            }
        }
    }
    let i = found?;
    let target = match (dir, edge) {
        (_, FindEdge::On) => i,
        (FindDir::Forward, FindEdge::Before) => {
            // One character before the match; no movement if already there.
            let before: Vec<(usize, char)> =
                line.char_indices().filter(|(j, _)| *j < i).collect();
            before.last().map(|(j, _)| *j).unwrap_or(i)
        }
        (FindDir::Backward, FindEdge::Before) => {
            let matched = line[i..].chars().next().unwrap_or(ch);
            i + matched.len_utf8()
        }
    };
    Some(line_start + target)
}

impl Default for ModalEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kp(c: char) -> KeyPress {
        KeyPress::plain(Key::Char(c))
    }

    fn esc() -> KeyPress {
        KeyPress::plain(Key::Esc)
    }

    fn enter() -> KeyPress {
        KeyPress::plain(Key::Enter)
    }

    /// App with `text` in the active buffer, cursor at (0,0), engine ready.
    /// Drives `handle_key_inner` directly: the INPUT_MODEL gate is
    /// compile-time KeyTrie in test builds, so tests exercise the engine
    /// itself (the one-line gate is covered by inspection).
    fn app_with(text: &str) -> EditorApp {
        let mut app = EditorApp::new();
        app.active_buffer_mut().unwrap().insert(0, text);
        // Fresh undo history so tests observe only engine-driven edits.
        app.active_buffer_mut().unwrap().undo_stack.clear();
        app.active_buffer_mut().unwrap().redo_stack.clear();
        app
    }

    fn press(app: &mut EditorApp, key: KeyPress) -> ModalOutcome {
        let mut engine = std::mem::take(&mut app.modal_engine);
        let outcome = engine.handle_key_inner(app, &key);
        app.modal_engine = engine;
        outcome
    }

    fn seq(app: &mut EditorApp, keys: &[KeyPress]) {
        for k in keys {
            press(app, *k);
        }
    }

    fn text(app: &EditorApp) -> String {
        let b = app.active_buffer().unwrap();
        b.get_range_as_string(0, b.len())
    }

    fn cursor(app: &EditorApp) -> (usize, usize) {
        let w = app.active_window_ref().unwrap();
        (w.cursor_x, w.cursor_y)
    }

    fn set_cursor(app: &mut EditorApp, x: usize, y: usize) {
        let w = app.active_window_mut().unwrap();
        w.cursor_x = x;
        w.cursor_y = y;
    }

    #[test]
    fn motions_move_cursor() {
        let mut app = app_with("hello world\nsecond line\n");
        press(&mut app, kp('l'));
        assert_eq!(cursor(&app), (1, 0));
        press(&mut app, kp('w'));
        assert_eq!(cursor(&app), (6, 0));
        press(&mut app, kp('j'));
        assert_eq!(cursor(&app), (6, 1));
        press(&mut app, kp('0'));
        assert_eq!(cursor(&app), (0, 1));
        press(&mut app, kp('$'));
        assert!(cursor(&app).0 > 0);
        press(&mut app, kp('G'));
        assert!(cursor(&app).1 >= 1);
        press(&mut app, kp('g'));
        press(&mut app, kp('g'));
        assert_eq!(cursor(&app), (0, 0));
    }

    #[test]
    fn count_prefix_multiplies_motion() {
        let mut app = app_with("a b c d e\n");
        seq(&mut app, &[kp('3'), kp('w')]);
        assert_eq!(cursor(&app), (6, 0));
        seq(&mut app, &[kp('2'), kp('l')]);
        assert_eq!(cursor(&app), (8, 0));
    }

    #[test]
    fn delete_word_operator() {
        let mut app = app_with("hello world\n");
        seq(&mut app, &[kp('d'), kp('w')]);
        assert_eq!(text(&app), "world\n");
        assert_eq!(cursor(&app), (0, 0));
    }

    #[test]
    fn delete_linewise_dd() {
        let mut app = app_with("one\ntwo\nthree\n");
        set_cursor(&mut app, 0, 1);
        seq(&mut app, &[kp('d'), kp('d')]);
        assert_eq!(text(&app), "one\nthree\n");
        assert_eq!(cursor(&app).1, 1);
    }

    #[test]
    fn count_dd_deletes_n_lines() {
        let mut app = app_with("one\ntwo\nthree\nfour\n");
        seq(&mut app, &[kp('2'), kp('d'), kp('d')]);
        assert_eq!(text(&app), "three\nfour\n");
    }

    #[test]
    fn yank_and_put_round_trip() {
        let mut app = app_with("hello world\n");
        // `yw` yanks "hello " (to next word start) and leaves the cursor at
        // the start (vim); `p` puts it after the cursor.
        seq(&mut app, &[kp('y'), kp('w')]);
        assert_eq!(text(&app), "hello world\n");
        press(&mut app, kp('p'));
        assert_eq!(text(&app), "hhello ello world\n");
    }

    #[test]
    fn linewise_yank_put_below() {
        let mut app = app_with("one\ntwo\n");
        set_cursor(&mut app, 0, 0);
        seq(&mut app, &[kp('y'), kp('y')]);
        press(&mut app, kp('j'));
        press(&mut app, kp('p'));
        assert_eq!(text(&app), "one\ntwo\none\n");
    }

    #[test]
    fn named_register_yank_put() {
        let mut app = app_with("alpha beta\n");
        // "ayw yanks "alpha " into register a; p uses the unnamed register
        // (also "alpha "), "ap puts from a explicitly.
        seq(&mut app, &[kp('"'), kp('a'), kp('y'), kp('w')]);
        press(&mut app, kp('$'));
        seq(&mut app, &[kp('"'), kp('a'), kp('p')]);
        assert!(text(&app).contains("alpha "));
        // unnamed register also got the yank
        assert_eq!(text(&app).matches("alpha ").count(), 2);
    }

    #[test]
    fn change_word_enters_insert_and_records() {
        let mut app = app_with("hello world\n");
        seq(&mut app, &[kp('c'), kp('w')]);
        // `cw` on a word is `ce`: the trailing space survives.
        assert_eq!(text(&app), " world\n");
        assert_eq!(app.modal_engine.mode(), ModalMode::Insert);
        seq(&mut app, &[kp('X'), kp('Y'), esc()]);
        assert_eq!(text(&app), "XY world\n");
        assert_eq!(app.modal_engine.mode(), ModalMode::Normal);
    }

    #[test]
    fn dot_repeats_delete_word() {
        let mut app = app_with("aa bb cc\n");
        seq(&mut app, &[kp('d'), kp('w')]);
        assert_eq!(text(&app), "bb cc\n");
        press(&mut app, kp('.'));
        assert_eq!(text(&app), "cc\n");
    }

    #[test]
    fn dot_repeats_change_with_inserted_text() {
        let mut app = app_with("aa bb\n");
        seq(&mut app, &[kp('c'), kp('w'), kp('Z'), esc()]);
        // `cw` on a word is `ce`: only `aa` goes.
        assert_eq!(text(&app), "Z bb\n");
        // `.` replays `cw` on `b`: `ce` deletes `bb`, inserts `Z`.
        set_cursor(&mut app, 2, 0);
        press(&mut app, kp('.'));
        assert_eq!(text(&app), "Z Z\n");
    }

    #[test]
    fn x_deletes_char_under_cursor() {
        let mut app = app_with("hello\n");
        seq(&mut app, &[kp('3'), kp('x')]);
        assert_eq!(text(&app), "lo\n");
    }

    #[test]
    fn capital_d_deletes_to_end_of_line() {
        let mut app = app_with("hello world\n");
        set_cursor(&mut app, 5, 0);
        press(&mut app, kp('D'));
        assert_eq!(text(&app), "hello\n");
    }

    #[test]
    fn replace_char() {
        let mut app = app_with("hello\n");
        seq(&mut app, &[kp('r'), kp('J')]);
        assert_eq!(text(&app), "Jello\n");
        // and `.` repeats the replacement on the new char under the cursor
        press(&mut app, kp('l'));
        press(&mut app, kp('.'));
        assert_eq!(text(&app), "JJllo\n");
    }

    #[test]
    fn find_char_motion() {
        let mut app = app_with("hello world\n");
        seq(&mut app, &[kp('f'), kp('o')]);
        // first 'o' after cursor is at index 4
        assert_eq!(cursor(&app), (4, 0));
        // `;` repeats forward to the next 'o' at index 7
        press(&mut app, kp(';'));
        assert_eq!(cursor(&app), (7, 0));
    }

    #[test]
    fn delete_to_find_char() {
        let mut app = app_with("hello world\n");
        seq(&mut app, &[kp('d'), kp('f'), kp('o')]);
        // `df` takes the found character (vim).
        assert_eq!(text(&app), " world\n");
    }

    #[test]
    fn visual_delete_selection() {
        let mut app = app_with("hello world\n");
        press(&mut app, kp('v'));
        assert_eq!(app.modal_engine.mode(), ModalMode::Visual);
        seq(&mut app, &[kp('l'), kp('l'), kp('l')]);
        press(&mut app, kp('d'));
        assert_eq!(text(&app), "lo world\n");
        assert_eq!(app.modal_engine.mode(), ModalMode::Normal);
    }

    #[test]
    fn visual_yank_keeps_text() {
        let mut app = app_with("hello world\n");
        press(&mut app, kp('v'));
        seq(&mut app, &[kp('w'), kp('y')]);
        assert_eq!(text(&app), "hello world\n");
        // `vw` selects through the next word's first character (vim), so
        // yanked "hello w" is in the unnamed register: p duplicates it.
        press(&mut app, kp('0'));
        press(&mut app, kp('p'));
        assert!(text(&app).starts_with("hhello wello world"));
    }

    #[test]
    fn insert_mode_types_and_esc_returns() {
        let mut app = app_with("bc\n");
        press(&mut app, kp('i'));
        assert_eq!(app.modal_engine.mode(), ModalMode::Insert);
        press(&mut app, kp('a'));
        press(&mut app, esc());
        assert_eq!(app.modal_engine.mode(), ModalMode::Normal);
        assert_eq!(text(&app), "abc\n");
    }

    #[test]
    fn append_commands_position_cursor() {
        let mut app = app_with("hello\n");
        press(&mut app, kp('A'));
        assert_eq!(app.modal_engine.mode(), ModalMode::Insert);
        press(&mut app, kp('!'));
        press(&mut app, esc());
        assert_eq!(text(&app), "hello!\n");
    }

    #[test]
    fn open_line_below_and_above() {
        let mut app = app_with("one\ntwo\n");
        press(&mut app, kp('o'));
        seq(&mut app, &[kp('x'), esc()]);
        assert_eq!(text(&app), "one\nx\ntwo\n");
        set_cursor(&mut app, 0, 2);
        press(&mut app, kp('O'));
        seq(&mut app, &[kp('y'), esc()]);
        assert_eq!(text(&app), "one\nx\ny\ntwo\n");
    }

    #[test]
    fn undo_after_modal_delete() {
        let mut app = app_with("hello world\n");
        seq(&mut app, &[kp('d'), kp('w')]);
        assert_eq!(text(&app), "world\n");
        press(&mut app, kp('u'));
        assert_eq!(text(&app), "hello world\n");
    }

    #[test]
    fn cmdline_writes_and_quits() {
        let mut app = app_with("hello\n");
        // :<number> goes to that line (no crash on scratch buffer)
        seq(&mut app, &[kp(':'), kp('1'), enter()]);
        assert_eq!(app.modal_engine.mode(), ModalMode::Normal);
        // unknown command reports honestly
        seq(&mut app, &[kp(':'), kp('z'), kp('z'), kp('z'), enter()]);
        assert!(app
            .message
            .as_deref()
            .unwrap_or("")
            .contains("Not an editor command"));
    }

    #[test]
    fn cmdline_resolves_named_commands() {
        let mut app = app_with("hello world\n");
        // A typed command with no vim default binding runs through `:`.
        let keys: Vec<KeyPress> = ":word-count"
            .chars()
            .map(kp)
            .chain(std::iter::once(enter()))
            .collect();
        seq(&mut app, &keys);
        assert!(app
            .message
            .as_deref()
            .unwrap_or("")
            .contains("Words: 2"));
        // Display-intercepted commands report instead of silently dying.
        let keys: Vec<KeyPress> = ":toggle-menu"
            .chars()
            .map(kp)
            .chain(std::iter::once(enter()))
            .collect();
        seq(&mut app, &keys);
        assert!(app
            .message
            .as_deref()
            .unwrap_or("")
            .contains("event loop"));
    }

    #[test]
    fn slash_and_question_start_search() {
        use crate::core::focus::FocusTarget;
        // `/` must enter incremental search, not die silently: the engine
        // routes NeedsInput through the focus manager like the event loop.
        let mut app = app_with("hello world\n");
        press(&mut app, kp('/'));
        assert_eq!(
            app.focus_manager
                .current_state()
                .map(|f| f.target),
            Some(FocusTarget::ISearch)
        );
        // `?` (reverse search) prompts through the same path.
        let mut app = app_with("hello world\n");
        press(&mut app, kp('?'));
        assert!(app.focus_manager.current_state().is_some());
    }

    #[test]
    fn cmdline_quit_refuses_when_modified() {
        let mut app = app_with("hello\n");
        seq(&mut app, &[kp('d'), kp('w')]);
        seq(&mut app, &[kp(':'), kp('q'), enter()]);
        assert!(app
            .message
            .as_deref()
            .unwrap_or("")
            .contains("No write since last change"));
    }

    #[test]
    fn colon_w_raises_real_reload_question() {
        use crate::core::focus::FocusTarget;
        use crate::terminal::display::Display;
        use crate::terminal::event_handler::process_terminal_event;
        use crate::terminal::events::EditorEvent;
        use crate::terminal::keybinds::KeyBindingManager;

        let path = std::env::temp_dir().join(format!("erax-reload-vim-{}", std::process::id()));
        std::fs::write(&path, "original\n").unwrap();
        let mut app = EditorApp::new();
        let bid = app.load_file(&path).unwrap();
        let win = app.active_window;
        app.windows.get_mut(&win).unwrap().buffer_id = bid;
        // External change with a guaranteed-different mtime.
        std::fs::write(&path, "changed\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(30))
            .unwrap();

        // `:w` dispatches save-buffer through the modal engine.
        seq(&mut app, &[kp(':'), kp('w'), enter()]);
        assert_eq!(
            app.focus_manager.current_target(),
            FocusTarget::ConfirmReload,
            ":w must raise the real reload question"
        );
        // The focus machinery owns the answer in every model: the modal
        // engine never sees the key.
        let mut display = Display::new(80, 24, None, None);
        let mut kbm = KeyBindingManager::new();
        let exit = process_terminal_event(&mut app, &mut display, &mut kbm, EditorEvent::Input(kp('y')))
            .unwrap();
        assert!(!exit);
        let buffer = app.buffers.get(&bid).unwrap();
        let text: String = (0..buffer.line_count())
            .filter_map(|i| buffer.line(i))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(text, "changed\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn esc_clears_pending_operator() {
        let mut app = app_with("hello world\n");
        press(&mut app, kp('d'));
        assert_eq!(app.modal_engine.mode(), ModalMode::OperatorPending);
        press(&mut app, esc());
        assert_eq!(app.modal_engine.mode(), ModalMode::Normal);
        assert_eq!(text(&app), "hello world\n");
    }

    #[test]
    fn kill_ring_untouched_by_vim_yank() {
        let mut app = app_with("hello world\n");
        let before = app.kill_ring.peek().map(|s| s.to_string());
        seq(&mut app, &[kp('y'), kp('y')]);
        seq(&mut app, &[kp('d'), kp('w')]);
        assert_eq!(app.kill_ring.peek().map(|s| s.to_string()), before);
    }

    #[test]
    fn status_text_shows_pending_state() {
        let mut app = app_with("hello\n");
        press(&mut app, kp('2'));
        press(&mut app, kp('d'));
        assert_eq!(app.modal_engine.status_text(), "2d");
        press(&mut app, esc());
        assert_eq!(app.modal_engine.status_text(), "");
        press(&mut app, kp('i'));
        assert_eq!(app.modal_engine.status_text(), "-- INSERT --");
        press(&mut app, esc());
    }

    #[test]
    fn mode_segment_per_mode() {
        let mut app = app_with("hello\n");
        assert_eq!(app.modal_engine.mode_segment(), Some("-- NORMAL --"));
        press(&mut app, kp('i'));
        assert_eq!(app.modal_engine.mode_segment(), Some("-- INSERT --"));
        press(&mut app, esc());
        press(&mut app, kp('v'));
        assert_eq!(app.modal_engine.mode_segment(), Some("-- VISUAL --"));
        press(&mut app, esc());
        press(&mut app, kp('V'));
        assert_eq!(app.modal_engine.mode_segment(), Some("-- VISUAL LINE --"));
        assert!(app.modal_engine.is_linewise_visual());
        press(&mut app, esc());
        press(&mut app, kp('d'));
        // OperatorPending is a sub-state of Normal for the segment.
        assert_eq!(app.modal_engine.mode(), ModalMode::OperatorPending);
        assert_eq!(app.modal_engine.mode_segment(), Some("-- NORMAL --"));
        press(&mut app, esc());
        press(&mut app, kp(':'));
        assert_eq!(app.modal_engine.mode_segment(), None);
        press(&mut app, esc());
    }

    #[test]
    fn pending_text_count_operator_register() {
        let mut app = app_with("hello world\n");
        // `d3w`: operator, then count.
        press(&mut app, kp('d'));
        press(&mut app, kp('3'));
        assert_eq!(app.modal_engine.pending_text(), "3d");
        assert_eq!(app.modal_engine.pending_count(), 3);
        assert_eq!(
            app.modal_engine.pending_operator(),
            Some(ModalOperator::Delete)
        );
        press(&mut app, esc());
        // `"a2c`: register, count, operator.
        seq(&mut app, &[kp('"'), kp('a'), kp('2'), kp('c')]);
        assert_eq!(app.modal_engine.pending_text(), "\"a2c");
        assert_eq!(app.modal_engine.active_register(), 'a');
        assert_eq!(app.modal_engine.pending_count(), 2);
        assert_eq!(
            app.modal_engine.pending_operator(),
            Some(ModalOperator::Change)
        );
        press(&mut app, esc());
        assert_eq!(app.modal_engine.pending_text(), "");
        assert_eq!(app.modal_engine.active_register(), '"');
    }

    #[test]
    fn cmdline_text_and_cursor_accessors() {
        let mut app = app_with("hello\n");
        seq(&mut app, &[kp(':'), kp('w'), kp('q')]);
        assert_eq!(app.modal_engine.mode(), ModalMode::CommandLine);
        assert_eq!(app.modal_engine.cmdline_text(), "wq");
        assert_eq!(app.modal_engine.cmdline_cursor(), 2);
        press(&mut app, esc());
        assert_eq!(app.modal_engine.cmdline_text(), "");
        assert_eq!(app.modal_engine.cmdline_cursor(), 0);
    }

    // -- Real-vim differential corpus ------------------------------------
    //
    // `tests/fixtures/vim_corpus.json` holds (buffer, keys, text, cursor,
    // register) triples recorded by driving real vim 9.1
    // (`tests/gen_vim_corpus.py`). This test replays every key sequence
    // through the modal engine and asserts identical observable state:
    // buffer text, cursor position, and the unnamed register. A mismatch
    // is a vim-fidelity bug, not a test bug — the corpus is the oracle.

    #[derive(serde::Deserialize)]
    struct VimCase {
        name: String,
        buffer: Option<String>,
        keys: Option<Vec<String>>,
        vim_text: Option<String>,
        /// [line, col], 0-based (converted at generation).
        vim_cursor: Option<[usize; 2]>,
        vim_register: Option<String>,
        error: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct VimCorpus {
        cases: Vec<VimCase>,
    }

    fn parse_corpus_key(token: &str) -> KeyPress {
        match token {
            "<Esc>" => KeyPress::plain(Key::Esc),
            "<CR>" => KeyPress::plain(Key::Enter),
            "<C-R>" => KeyPress::ctrl(Key::Char('r')),
            s => {
                let mut chars = s.chars();
                let c = chars.next().unwrap_or('?');
                assert!(chars.next().is_none(), "bad key token: {s}");
                KeyPress::plain(Key::Char(c))
            }
        }
    }

    #[test]
    fn vim_differential_corpus() {
        const CORPUS: &str = include_str!("../../tests/fixtures/vim_corpus.json");
        let corpus: VimCorpus =
            serde_json::from_str(CORPUS).expect("vim corpus JSON must parse");
        assert!(!corpus.cases.is_empty(), "corpus is empty");

        let mut failures = Vec::new();
        let mut ran = 0;
        for case in &corpus.cases {
            if let Some(err) = &case.error {
                failures.push(format!("{}: vim run failed: {err}", case.name));
                continue;
            }
            let (Some(buffer), Some(keys), Some(want_text), Some(want_cursor), Some(want_reg)) = (
                case.buffer.as_ref(),
                case.keys.as_ref(),
                case.vim_text.as_ref(),
                case.vim_cursor,
                case.vim_register.as_ref(),
            ) else {
                failures.push(format!("{}: corpus case missing fields", case.name));
                continue;
            };
            ran += 1;
            let mut app = app_with(buffer);
            let keypresses: Vec<KeyPress> = keys.iter().map(|k| parse_corpus_key(k)).collect();
            seq(&mut app, &keypresses);

            let got_text = text(&app);
            let got_cursor = cursor(&app);
            let got_reg = app
                .modal_engine
                .unnamed
                .as_ref()
                .map(|r| r.text.clone())
                .unwrap_or_default();
            // Engine cursor is (x, y); the corpus stores [line, col].
            let want_cursor_xy = (want_cursor[1], want_cursor[0]);

            let mut diffs = Vec::new();
            if got_text != *want_text {
                diffs.push(format!(
                    "text:\n  erax {got_text:?}\n  vim  {want_text:?}"
                ));
            }
            if got_cursor != want_cursor_xy {
                diffs.push(format!(
                    "cursor: erax {got_cursor:?}, vim {want_cursor_xy:?}"
                ));
            }
            if got_reg != *want_reg {
                diffs.push(format!("register: erax {got_reg:?}, vim {want_reg:?}"));
            }
            if !diffs.is_empty() {
                failures.push(format!("{}:\n  {}", case.name, diffs.join("\n  ")));
            }
        }
        assert!(ran > 50, "corpus too small to be meaningful: {ran}");
        assert!(
            failures.is_empty(),
            "{} corpus mismatches (vim is the oracle):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    /// Bracket text objects, driven directly: cursor_x is a grapheme
    /// column, results are byte offsets.
    fn pair_at(
        text: &str,
        x: usize,
        open: char,
        inner: bool,
        count: usize,
    ) -> Option<(usize, usize)> {
        let mut app = app_with(text);
        app.active_window_mut().unwrap().cursor_x = x;
        pair_object_range(&app, open, inner, count)
    }

    #[test]
    fn pair_basic_inner_and_around() {
        assert_eq!(pair_at("a(b)c", 2, '(', true, 1), Some((2, 3)));
        assert_eq!(pair_at("a(b)c", 2, '(', false, 1), Some((1, 4)));
    }

    #[test]
    fn pair_nested_and_count_outward() {
        // a(b(c)d)e: inner pair (3..5), outer pair (1..7) in bytes.
        assert_eq!(pair_at("a(b(c)d)e", 4, '(', true, 1), Some((4, 5)));
        assert_eq!(pair_at("a(b(c)d)e", 4, '(', true, 2), Some((2, 7)));
        assert_eq!(pair_at("a(b(c)d)e", 4, '(', false, 2), Some((1, 8)));
    }

    #[test]
    fn pair_cursor_on_brackets() {
        // A close bracket under the cursor is the pair's own end (vim).
        assert_eq!(pair_at("a(b)", 3, '(', false, 1), Some((1, 4)));
        // An open bracket under the cursor starts the pair there.
        assert_eq!(pair_at("a(b)", 1, '(', false, 1), Some((1, 4)));
    }

    #[test]
    fn pair_forward_fallback_and_missing() {
        // No unclosed bracket before the cursor: next `(` forward.
        assert_eq!(pair_at("ab(cd)", 0, '(', false, 1), Some((2, 6)));
        assert_eq!(pair_at("ab(cd", 0, '(', false, 1), None);
        assert_eq!(pair_at("abc", 1, '(', false, 1), None);
        assert_eq!(pair_at("a[b]c", 2, '[', true, 1), Some((2, 3)));
        assert_eq!(pair_at("a{b}c", 2, '{', false, 1), Some((1, 4)));
    }

    #[test]
    fn pair_multibyte_text() {
        // Byte offsets stay exact around multi-byte chars.
        assert_eq!(pair_at("é(b)é", 1, '(', false, 1), Some((2, 5)));
        assert_eq!(pair_at("é(b)é", 1, '(', true, 1), Some((3, 4)));
    }

    #[test]
    fn pair_far_from_buffer_start() {
        // The pair sits past 1 MiB of text: the scan walks chunks, it
        // never materializes the whole buffer to find it.
        let mut text = "x".repeat(1 << 20);
        text.push_str("(target)");
        text.push_str(&"y".repeat(100));
        let mut app = app_with(&text);
        app.active_window_mut().unwrap().cursor_x = (1 << 20) + 2;
        assert_eq!(
            pair_object_range(&app, '(', false, 1),
            Some(((1 << 20), (1 << 20) + 8))
        );
    }

    /// Mode transitions emit exactly one `ModeEntered` cue carrying
    /// the new mode; keys that do not change the mode emit nothing.
    /// The cue bus is the safety-critical path for non-visual vim
    /// users (a silent mode change is a data-loss vector).
    #[test]
    fn mode_transitions_emit_exactly_one_cue() {
        use crate::core::cue::{Cue, CueBus};
        let mut app = app_with("hello");
        app.cue_bus = CueBus::new(true);

        // Normal -> Insert: one cue.
        press(&mut app, kp('i'));
        let cues: Vec<Cue> = app.cue_bus.drain().collect();
        assert_eq!(cues, vec![Cue::ModeEntered(ModalMode::Insert)]);

        // Typing in Insert mode changes no mode: silence.
        press(&mut app, kp('x'));
        assert_eq!(app.cue_bus.pending(), 0);

        // Insert -> Normal via Esc: one cue.
        press(&mut app, esc());
        let cues: Vec<Cue> = app.cue_bus.drain().collect();
        assert_eq!(cues, vec![Cue::ModeEntered(ModalMode::Normal)]);

        // Normal -> Visual: one cue.
        press(&mut app, kp('v'));
        let cues: Vec<Cue> = app.cue_bus.drain().collect();
        assert_eq!(cues, vec![Cue::ModeEntered(ModalMode::Visual)]);
    }

    #[test]
    fn operator_pending_transition_emits_its_mode() {
        use crate::core::cue::{Cue, CueBus};
        let mut app = app_with("hello");
        app.cue_bus = CueBus::new(true);
        press(&mut app, kp('d'));
        let cues: Vec<Cue> = app.cue_bus.drain().collect();
        assert_eq!(
            cues,
            vec![Cue::ModeEntered(ModalMode::OperatorPending)]
        );
    }

    #[test]
    fn completed_operator_motion_emits_both_transitions() {
        // Each keypress is handled independently: `d` leaves the user
        // in OperatorPending (the safety-critical announcement), and
        // `w` completes the motion back to Normal. Both transitions
        // are announced; the cue names the state each key leaves.
        use crate::core::cue::{Cue, CueBus};
        let mut app = app_with("hello world");
        app.cue_bus = CueBus::new(true);
        press(&mut app, kp('d'));
        let cues: Vec<Cue> = app.cue_bus.drain().collect();
        assert_eq!(
            cues,
            vec![Cue::ModeEntered(ModalMode::OperatorPending)]
        );
        press(&mut app, kp('w'));
        let cues: Vec<Cue> = app.cue_bus.drain().collect();
        assert_eq!(cues, vec![Cue::ModeEntered(ModalMode::Normal)]);
        assert_eq!(app.modal_engine.mode(), ModalMode::Normal);
    }
}

/// End-to-end parity for the modal input model.
///
/// Production reaches the engine through [`ModalEngine::handle_key`],
/// which passes the compile-time `INPUT_MODEL` gate in Modal builds and
/// delegates to `handle_key_inner`. Test builds compile `INPUT_MODEL` as
/// KeyTrie, so these tests drive `handle_key_inner` directly — the real
/// engine path, modulo the one-line gate (covered by inspection, as the
/// existing engine tests do).
///
/// The contract mirrors `tests/parity.rs`: the expectation for each
/// command is derived from one real dispatch on a fresh app, and the
/// engine path must produce the matching observable — `ExitRequested`
/// for `Exit`, a pushed prompt for `NeedsInput`, the describe-key focus
/// for `AwaitKey`, the message for `Info`, consumption otherwise.
#[cfg(test)]
mod parity_end_to_end {
    use super::*;
    use crate::core::dispatcher::dispatch;

    enum Expect {
        Exit,
        Prompt,
        AwaitKey(crate::core::focus::FocusTarget),
        Message(String),
        Consumed,
    }

    fn expect(cmd: Command) -> Expect {
        let mut app = EditorApp::new();
        match dispatch(&mut app, cmd, 1) {
            DispatchResult::Exit => Expect::Exit,
            DispatchResult::NeedsInput { .. } => Expect::Prompt,
            DispatchResult::AwaitKey(action) => Expect::AwaitKey(
                crate::core::focus::focus_target_for_await_key(action),
            ),
            DispatchResult::Info(m) => Expect::Message(m),
            _ => Expect::Consumed,
        }
    }

    fn fresh_engine_app() -> (ModalEngine, EditorApp) {
        (ModalEngine::new(), EditorApp::new())
    }

    #[test]
    fn wq_stops_when_save_needs_a_filename() {
        // `:wq` on an untitled modified buffer: the save returns
        // NeedsInput (SaveAs prompt). The sequence must stop there;
        // the old code ran the quit anyway and overwrote the outcome.
        let (mut engine, mut app) = fresh_engine_app();
        app.active_buffer_mut().unwrap().insert(0, "unsaved");
        let outcome = engine.execute_cmdline(&mut app, "wq");
        assert_ne!(outcome, ModalOutcome::ExitRequested);
        assert!(app
            .focus_manager
            .current_state()
            .is_some_and(|s| s.prompt == "Save as: "));
        assert!(app.active_buffer().is_some_and(|b| b.modified));
    }

    #[test]
    fn wq_quits_when_save_succeeds() {
        // `:wq` on a named modified buffer: the save succeeds and the
        // quit runs.
        let (mut engine, mut app) = fresh_engine_app();
        let path = std::env::temp_dir().join(format!("erax-wq-{}.txt", std::process::id()));
        std::fs::write(&path, "").expect("write temp file");
        if let Some(buffer) = app.active_buffer_mut() {
            buffer.filename = Some(path.clone());
            buffer.insert(0, "saved by :wq");
        }
        let outcome = engine.execute_cmdline(&mut app, "wq");
        assert_eq!(outcome, ModalOutcome::ExitRequested);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            "saved by :wq"
        );
        let _ = std::fs::remove_file(&path);
    }

    fn feed(engine: &mut ModalEngine, app: &mut EditorApp, keys: &[KeyPress]) -> ModalOutcome {
        let mut outcome = ModalOutcome::Consumed;
        for key in keys {
            outcome = engine.handle_key_inner(app, key);
            if outcome == ModalOutcome::ExitRequested {
                break;
            }
        }
        outcome
    }

    fn assert_observation(app: &EditorApp, cmd: Command, outcome: ModalOutcome, path: &str) {
        let name = cmd.name();
        match expect(cmd) {
            Expect::Exit => assert_eq!(
                outcome,
                ModalOutcome::ExitRequested,
                "{path}: {name} must surface ExitRequested"
            ),
            Expect::Prompt => assert!(
                app.focus_manager.current_state().is_some(),
                "{path}: {name} must push a prompt on the focus manager"
            ),
            Expect::AwaitKey(target) => assert_eq!(
                app.focus_manager.current_target(),
                target,
                "{path}: {name} must push the focus its action maps to"
            ),
            Expect::Message(m) => assert_eq!(
                app.message.as_deref(),
                Some(m.as_str()),
                "{path}: {name} must surface its message"
            ),
            Expect::Consumed => assert_ne!(
                outcome,
                ModalOutcome::ExitRequested,
                "{path}: {name} must not raise the exit signal"
            ),
        }
    }

    fn cmdline_entry_key() -> KeyPress {
        MODAL_BINDINGS
            .iter()
            .find(|b| b.action == ModalAction::EnterMode(ModalMode::CommandLine))
            .expect("modal table must bind a command-line entry")
            .key
    }

    #[test]
    fn modal_bindings_drive_real_dispatch() {
        let mut failures = Vec::new();
        for binding in MODAL_BINDINGS
            .iter()
            .filter(|b| b.mode == ModalMode::Normal)
        {
            let ModalAction::Command(cmd) = binding.action else {
                continue;
            };
            let (mut engine, mut app) = fresh_engine_app();
            let outcome = feed(&mut engine, &mut app, &[binding.key]);
            // A tabled Normal-mode binding must be claimed by the engine,
            // never passed through to the trie underneath.
            if outcome == ModalOutcome::PassThrough {
                failures.push((cmd.name(), "passed through"));
                continue;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                assert_observation(&app, cmd, outcome, "modal");
            }));
            if result.is_err() {
                failures.push((cmd.name(), "distorted"));
            }
        }
        assert!(
            failures.is_empty(),
            "modal: invocation path failed for: {failures:?}"
        );
    }

    #[test]
    fn cmdline_reaches_every_command_end_to_end() {
        let entry = cmdline_entry_key();
        let mut failures = Vec::new();
        for cmd in Command::all() {
            // toggle-menu is intercepted by the event loop before dispatch;
            // the modal engine owns no display state, so the `:` line
            // reports the display-bound key instead. It stays reachable
            // through F10.
            if *cmd == Command::ToggleMenu {
                let (mut engine, mut app) = fresh_engine_app();
                let mut keys = vec![entry];
                keys.extend(cmd.name().chars().map(|c| KeyPress::plain(Key::Char(c))));
                keys.push(KeyPress::plain(Key::Enter));
                let outcome = feed(&mut engine, &mut app, &keys);
                assert_eq!(outcome, ModalOutcome::Consumed);
                assert!(
                    app.message.is_some(),
                    "cmdline: toggle-menu must explain the display-bound key"
                );
                continue;
            }
            let (mut engine, mut app) = fresh_engine_app();
            let mut keys = vec![entry];
            keys.extend(cmd.name().chars().map(|c| KeyPress::plain(Key::Char(c))));
            keys.push(KeyPress::plain(Key::Enter));
            let outcome = feed(&mut engine, &mut app, &keys);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                assert_observation(&app, *cmd, outcome, "cmdline");
            }));
            if result.is_err() {
                failures.push(cmd.name());
            }
        }
        assert!(
            failures.is_empty(),
            "cmdline: invocation path distorted the dispatch result for: {failures:?}"
        );
    }

}
