//! Keybindings and input model: typed `pub const`s (N-008, N-270).
//!
//! Every binding is a [`KeyBinding`]: a typed key sequence to a typed
//! [`Command`]. There is one canonical key representation (N-010, N-060) —
//! ESC-prefix normalization happens once in the keybinding trie, so the old
//! duplicated `"ESC-x"` / `"Esc x"` spellings are gone: each binding is
//! written once with [`KeyPress::alt`].
//!
//! A binding that is both a leaf command and a prefix of a longer binding
//! declares its [`PrefixPolicy`] explicitly (N-013): `^C` waits for a possible
//! `^C Tab`, while every other leaf executes immediately.
//!
//! The input model is a compile-time switch (N-268): [`INPUT_MODEL`] selects
//! the key-trie model or the modal model. Both are real, implemented input
//! paths behind the one switch.
//!
//! Binding doctrine: every key, key sequence, and command-line word that
//! invokes a command is data in this module. Engines interpret the tables
//! and never hardcode a key-to-command or word-to-command mapping. A
//! command with no key idiom in a model stays reachable through that
//! model's named facility (M-x, the `:` line, the command palette); the
//! end-to-end parity test drives those facilities, so reachability never
//! rests on a table entry alone. The facility-only commands are also
//! recorded as data in [`FACILITY_BINDINGS`], so this module is the
//! complete inventory of bindings.

use crate::core::command::Command;
use crate::core::input::{Key, KeyPress, PrefixPolicy};

/// A single typed keybinding: a key sequence, its command, and the
/// leaf/prefix policy (N-008, N-013).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyBinding {
    /// The key sequence; usually one key, sometimes several (`^X ^F`).
    pub keys: &'static [KeyPress],
    /// The typed command to run.
    pub command: Command,
    /// What to do when this binding is both a leaf and a prefix (N-013).
    pub policy: PrefixPolicy,
}

/// Compile-time input model switch (N-268).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputModel {
    /// Prefix/sequence keybindings through the trie (the default, uemacs-faithful).
    KeyTrie,
    /// Modal editing: Normal, Insert, Visual, Operator-pending, Command-line.
    Modal,
    /// Modeless "standard" editing (Notepad/VS Code style): typing inserts,
    /// CUA bindings (Ctrl+C/X/V, Ctrl+S, Ctrl+Z/Y), Shift+arrows select,
    /// mouse-first.
    Standard,
}

/// The input model. Change and rebuild to switch.
pub const INPUT_MODEL: InputModel = InputModel::KeyTrie;

/// How long an `AwaitLonger` leaf waits for a longer sequence before the leaf
/// command executes (N-013).
pub const KEY_SEQUENCE_TIMEOUT_MS: u64 = 500;

// ---------------------------------------------------------------------------
// Key-trie bindings: the default keymap (uemacs-faithful).
// ---------------------------------------------------------------------------

const fn ctrl(c: char) -> KeyPress {
    KeyPress::ctrl(Key::Char(c))
}

const fn alt(c: char) -> KeyPress {
    KeyPress::alt(Key::Char(c))
}

const fn plain(key: Key) -> KeyPress {
    KeyPress::plain(key)
}

/// The default keybindings, in uemacs-faithful arrangement.
///
/// Each binding appears exactly once (N-010): no duplicate ESC spellings.
/// Leaf-and-prefix conflicts declare their policy explicitly (N-013).
pub const KEYBINDINGS: &[KeyBinding] = &[
    // -- movement --
    KeyBinding { keys: &[ctrl('f')], command: Command::ForwardCharacter, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('b')], command: Command::BackwardCharacter, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('n')], command: Command::NextLine, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('p')], command: Command::PreviousLine, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Right)], command: Command::ForwardCharacter, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Left)], command: Command::BackwardCharacter, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Down)], command: Command::NextLine, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Up)], command: Command::PreviousLine, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Home)], command: Command::BeginningOfLine, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::End)], command: Command::EndOfLine, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::PageUp)], command: Command::BackwardPage, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::PageDown)], command: Command::ForwardPage, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Insert)], command: Command::ToggleOverwriteMode, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('a')], command: Command::BeginningOfLine, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('e')], command: Command::EndOfLine, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('v')], command: Command::ForwardPage, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('V')], command: Command::BackwardPage, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('v')], command: Command::BackwardPage, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('<')], command: Command::BeginningOfFile, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('>')], command: Command::EndOfFile, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('f')], command: Command::ForwardWord, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('b')], command: Command::BackwardWord, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('t')], command: Command::TransposeWords, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('x')], command: Command::ExecuteNamedCommand, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('g')], command: Command::GotoLine, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('}')], command: Command::ForwardParagraph, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('{')], command: Command::BackwardParagraph, policy: PrefixPolicy::ExecuteLeaf },
    // Alt+Ctrl+F: goto-matching-fence.
    KeyBinding { keys: &[KeyPress::mods(Key::Char('f'), false, true, true)], command: Command::GotoMatchingFence, policy: PrefixPolicy::ExecuteLeaf },
    // -- file operations --
    KeyBinding { keys: &[ctrl('x'), ctrl('f')], command: Command::FindFile, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), ctrl('s')], command: Command::SaveBuffer, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), ctrl('w')], command: Command::WriteFile, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), ctrl('c')], command: Command::ExitErax, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), ctrl('r')], command: Command::ReadFile, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Esc), plain(Key::Char('P'))], command: Command::Print, policy: PrefixPolicy::ExecuteLeaf },
    // -- editing --
    KeyBinding { keys: &[ctrl('d')], command: Command::DeleteNextCharacter, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Delete)], command: Command::DeleteNextCharacter, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Backspace)], command: Command::DeletePreviousCharacter, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Enter)], command: Command::InsertNewline, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Tab)], command: Command::InsertTab, policy: PrefixPolicy::ExecuteLeaf },
    // ^C is both insert-space and the prefix of ^C Tab: wait for the longer
    // sequence (N-013).
    KeyBinding { keys: &[ctrl('c'), plain(Key::Tab)], command: Command::ExpandSnippet, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('k')], command: Command::KillToEndOfLine, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('y')], command: Command::Yank, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('y')], command: Command::YankPop, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('w')], command: Command::KillRegion, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('w')], command: Command::CopyRegion, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('o')], command: Command::OpenLine, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('t')], command: Command::TransposeCharacters, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('/')], command: Command::WordCompletion, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Tab)], command: Command::WordCompletion, policy: PrefixPolicy::ExecuteLeaf },
    // -- search --
    KeyBinding { keys: &[ctrl('s')], command: Command::SearchForward, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('r')], command: Command::SearchReverse, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('%')], command: Command::QueryReplace, policy: PrefixPolicy::ExecuteLeaf },
    // -- windows --
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('2'))], command: Command::SplitCurrentWindow, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('3'))], command: Command::SplitWindowHorizontally, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('0'))], command: Command::MinimizeWindow, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('9'))], command: Command::WindowPicker, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('1'))], command: Command::DeleteOtherWindows, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('o'))], command: Command::NextWindow, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('O'))], command: Command::NextWindow, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('Z'))], command: Command::GrowWindow, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), ctrl('z')], command: Command::ShrinkWindow, policy: PrefixPolicy::ExecuteLeaf },
    // -- control keys --
    KeyBinding { keys: &[ctrl('z')], command: Command::BackwardPage, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('c')], command: Command::InsertSpace, policy: PrefixPolicy::AwaitLonger },
    KeyBinding { keys: &[ctrl('j')], command: Command::NewlineAndIndent, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('l')], command: Command::RedrawDisplay, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('q')], command: Command::QuoteCharacter, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('u')], command: Command::UniversalArgument, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('g')], command: Command::KeyboardQuit, policy: PrefixPolicy::ExecuteLeaf },
    // -- buffers --
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('B'))], command: Command::SelectBuffer, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('K'))], command: Command::DeleteBuffer, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), ctrl('b')], command: Command::ListBuffers, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('d'))], command: Command::ToggleDiagnostics, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('='))], command: Command::ShowPosition, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('?'))], command: Command::DescribeKey, policy: PrefixPolicy::ExecuteLeaf },
    // -- case conversion --
    KeyBinding { keys: &[alt('j')], command: Command::JustifyParagraph, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('q')], command: Command::JustifyParagraph, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('z')], command: Command::ExitAndSave, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('u')], command: Command::CaseWordUpper, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('l')], command: Command::CaseWordLower, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('c')], command: Command::CaseWordCapitalize, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), ctrl('u')], command: Command::UppercaseRegion, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), ctrl('l')], command: Command::LowercaseRegion, policy: PrefixPolicy::ExecuteLeaf },
    // -- shell / calculator --
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('4'))], command: Command::ShellCommand, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('p'))], command: Command::Calculator, policy: PrefixPolicy::ExecuteLeaf },
    // -- terminal --
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('t')), plain(Key::Char('2'))], command: Command::SplitSpawnTerminalVertical, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('t')), plain(Key::Char('3'))], command: Command::SplitSpawnTerminalHorizontal, policy: PrefixPolicy::ExecuteLeaf },
    // C-c C-t: send the region (or current line) to the terminal.
    KeyBinding { keys: &[ctrl('c'), ctrl('t')], command: Command::TerminalSendInput, policy: PrefixPolicy::ExecuteLeaf },
    // C-c C-s: describe the active buffer's state as one spoken-style line.
    KeyBinding { keys: &[ctrl('c'), ctrl('s')], command: Command::DescribeState, policy: PrefixPolicy::ExecuteLeaf },
    // -- misc editing --
    KeyBinding { keys: &[ctrl('x'), ctrl('o')], command: Command::DeleteBlankLines, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('s')], command: Command::SedPreview, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('=')], command: Command::CountWords, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('$')], command: Command::SpellSuggest, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('|')], command: Command::FilterBuffer, policy: PrefixPolicy::ExecuteLeaf },
    // -- mark & region --
    KeyBinding { keys: &[ctrl('@')], command: Command::SetMark, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('.')], command: Command::SetMark, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), ctrl('x')], command: Command::ExchangePointAndMark, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('=')], command: Command::ExpandSelection, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('@')], command: Command::MarkWord, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('h')], command: Command::MarkParagraph, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[KeyPress::alt(Key::Delete)], command: Command::KillWord, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[KeyPress::alt(Key::Backspace)], command: Command::BackwardKillWord, policy: PrefixPolicy::ExecuteLeaf },
    // -- macros --
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('('))], command: Command::BeginMacro, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char(')'))], command: Command::EndMacro, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('e'))], command: Command::ExecuteMacro, policy: PrefixPolicy::ExecuteLeaf },
    // -- undo / redo --
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('u'))], command: Command::Undo, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('_')], command: Command::Undo, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x'), plain(Key::Char('U'))], command: Command::Redo, policy: PrefixPolicy::ExecuteLeaf },
    // -- diagnostics navigation --
    KeyBinding { keys: &[alt('n')], command: Command::DiagnosticsNext, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[alt('p')], command: Command::DiagnosticsPrevious, policy: PrefixPolicy::ExecuteLeaf },
    // -- menu (N-043: bound in config, not hardcoded in the handler) --
    KeyBinding { keys: &[plain(Key::F(10))], command: Command::ToggleMenu, policy: PrefixPolicy::ExecuteLeaf },
];

/// The keybinding table the key trie loads for an input model.
///
/// The trie underneath the modal engine is the key-trie table: ctrl/alt
/// chords pass through the engine to it (the engine only claims plain
/// keys). The modal engine reads [`MODAL_BINDINGS`] itself.
pub const fn bindings_for_model(model: InputModel) -> &'static [KeyBinding] {
    match model {
        InputModel::KeyTrie => KEYBINDINGS,
        InputModel::Standard => STANDARD_BINDINGS,
        InputModel::Modal => KEYBINDINGS,
    }
}

// ---------------------------------------------------------------------------
// Modal bindings (N-268): a real modal input path behind the INPUT_MODEL switch.
// ---------------------------------------------------------------------------
/// Modal editing mode (N-268).
///
/// Phase 1 implements all five modes. OperatorPending is entered after `d`/`y`/`c`;
/// the next key is a motion (or the same operator key for a linewise operation).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModalMode {
    /// Command mode: single keys are commands and motions.
    Normal,
    /// Typing inserts text; Esc returns to Normal.
    Insert,
    /// Motions extend the selection anchored at the mark.
    Visual,
    /// An operator (d/y/c) is awaiting its motion.
    OperatorPending,
    /// `:` command line: ex-style commands.
    CommandLine,
}

/// A composable vim editing operator (N-268 Phase 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalOperator {
    /// `d`: delete the region the motion covers.
    Delete,
    /// `y`: yank (copy) the region the motion covers into a register.
    Yank,
    /// `c`: delete the region, then enter Insert mode.
    Change,
}

/// Direction of a find-char motion (`f`/`F`/`t`/`T`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindDir {
    Forward,
    Backward,
}

/// Whether the find-char motion lands on the character (`f`/`F`) or just
/// before/after it (`t`/`T`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindEdge {
    On,
    Before,
}

/// What a modal keypress does (N-268).
///
/// `Command` variants compose over the existing typed [`Command`] set — the
/// modal engine never forks editing logic; operators are built from
/// SetMark + motion + KillRegion/CopyRegion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalAction {
    /// Run a typed editor command, staying in the current mode. In
    /// OperatorPending mode the command is the motion the pending operator
    /// applies to.
    Command(Command),
    /// Switch to a modal mode.
    EnterMode(ModalMode),
    /// Begin an operator (`d`/`y`/`c`); the next motion completes it.
    Operator(ModalOperator),
    /// `"` prefix: the next key names the register the yank/put uses.
    SelectRegister,
    /// `.`: repeat the last recorded change.
    RepeatLast,
    /// `r`: replace the character under the cursor with the next keypress.
    ReplaceChar,
    /// `f`/`F`/`t`/`T`: the next keypress is the character to find on the line.
    FindChar(FindDir, FindEdge),
    /// `;` / `,`: repeat the last find-char motion forward / backward.
    RepeatFind(bool),
    /// `g` prefix: the next key completes the `g` sequence (`gg`).
    PrefixG,
    /// `x`: delete the character under the cursor (count chars).
    DeleteCharForward,
    /// `X`: delete the character before the cursor (count chars).
    DeleteCharBackward,
    /// `s`: delete the character under the cursor and enter Insert.
    SubstituteChar,
    /// `S`: delete the current line(s) and enter Insert.
    SubstituteLine,
    /// `D`: delete to end of line (`d$`).
    DeleteToEndOfLine,
    /// `C`: change to end of line (`c$`, then Insert).
    ChangeToEndOfLine,
    /// `p` / `P`: put register text after / before the cursor.
    Put(bool),
    /// Vim `w`: to the start of the next word (word-start semantics, not
    /// the emacs word-end `ForwardWord`).
    WordForward,
    /// Vim `b`: back to the start of the previous word.
    WordBackward,
    /// Vim `e`: forward to the end of the current/next word (inclusive).
    WordEndForward,
    /// Vim `}`: forward to the next paragraph boundary (probed semantics,
    /// not the uEmacs [`Command::ForwardParagraph`]).
    VimParagraphForward,
    /// Vim `{`: backward to the previous paragraph boundary.
    VimParagraphBackward,
    /// Vim `$` in Normal mode: onto the last character of the line. The
    /// shared [`Command::EndOfLine`] stops one past the last character —
    /// that is the cursor-exclusive visual selection's end, not a
    /// Normal-mode cursor. As an operator motion `$` is
    /// [`Motion::ToEndOfLine`].
    VimLineEnd,
    /// Vim `G`: to the last content line of the buffer (a trailing newline
    /// does not count as a line, unlike the shared [`Command::EndOfFile`]).
    /// With a count, to that line number.
    VimFileEnd,
    /// Vim `ge` (via the `g` prefix): backward to the end of the previous
    /// word (inclusive).
    WordEndBackward,
    /// Vim `W`: forward to the start of the next WORD (blank-separated).
    WordForwardBig,
    /// Vim `B`: backward to the start of the previous WORD.
    WordBackwardBig,
    /// Vim `E`: forward to the end of the current/next WORD (inclusive).
    WordEndForwardBig,
    /// Vim `gE` (via the `g` prefix): backward to the end of the previous
    /// WORD (inclusive).
    WordEndBackwardBig,
    /// `i` / `a` in Visual mode or with an operator pending: begin a text
    /// object (`iw`, `a(`, `i"`, …). `inner` is true for `i`, false for `a`.
    TextObjectPrefix { inner: bool },
    /// `I`: enter Insert at the first non-blank of the line.
    InsertAtFirstNonBlank,
    /// `a`: enter Insert one character after the cursor.
    AppendAfterCursor,
    /// `A`: enter Insert at end of line.
    AppendAtEndOfLine,
    /// `o`: open a new line below and enter Insert.
    OpenLineBelow,
    /// `O`: open a new line above and enter Insert.
    OpenLineAbove,
    /// `V`: linewise visual mode (anchor whole lines).
    VisualLine,
}

/// One modal binding: a keypress in a modal mode to an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModalBinding {
    pub mode: ModalMode,
    pub key: KeyPress,
    pub action: ModalAction,
}

/// Normal/visual-mode keymap: motions, operators, registers, mode switches.
///
/// Phase 1 (N-268): counts, operators d/y/c with motions, dd/yy/cc linewise,
/// x/X/r/s/S/D/C, p/P with registers, visual v/V, command line `:`,
/// search `/`, find-char f/F/t/T with `;`/`,`, `.` repeat, `"` registers.
/// Phase 2 (N-268): `e`/`E`/`ge`/`gE` word-end motions, `W`/`B` WORD motions,
/// `(`/`)` sentence motions, and text objects (`iw`, `a(`, `i"`, …) —
/// all served by one shared word/text-object primitive.
pub const MODAL_BINDINGS: &[ModalBinding] = &[
    // -- motions --
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('h')), action: ModalAction::Command(Command::BackwardCharacter) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('j')), action: ModalAction::Command(Command::NextLine) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('k')), action: ModalAction::Command(Command::PreviousLine) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('l')), action: ModalAction::Command(Command::ForwardCharacter) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('w')), action: ModalAction::WordForward },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('b')), action: ModalAction::WordBackward },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('e')), action: ModalAction::WordEndForward },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('W')), action: ModalAction::WordForwardBig },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('B')), action: ModalAction::WordBackwardBig },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('E')), action: ModalAction::WordEndForwardBig },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('(')), action: ModalAction::Command(Command::BackwardSentence) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char(')')), action: ModalAction::Command(Command::ForwardSentence) },
    // '0' is BeginningOfLine only with no count pending; the engine special-cases it.
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('0')), action: ModalAction::Command(Command::BeginningOfLine) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('$')), action: ModalAction::VimLineEnd },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('^')), action: ModalAction::Command(Command::SmartHome) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('g')), action: ModalAction::PrefixG },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('G')), action: ModalAction::VimFileEnd },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('{')), action: ModalAction::VimParagraphBackward },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('}')), action: ModalAction::VimParagraphForward },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('%')), action: ModalAction::Command(Command::GotoMatchingFence) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('f')), action: ModalAction::FindChar(FindDir::Forward, FindEdge::On) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('F')), action: ModalAction::FindChar(FindDir::Backward, FindEdge::On) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('t')), action: ModalAction::FindChar(FindDir::Forward, FindEdge::Before) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('T')), action: ModalAction::FindChar(FindDir::Backward, FindEdge::Before) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char(';')), action: ModalAction::RepeatFind(true) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char(',')), action: ModalAction::RepeatFind(false) },
    // -- operators --
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('d')), action: ModalAction::Operator(ModalOperator::Delete) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('y')), action: ModalAction::Operator(ModalOperator::Yank) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('c')), action: ModalAction::Operator(ModalOperator::Change) },
    // -- char edits --
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('x')), action: ModalAction::DeleteCharForward },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('X')), action: ModalAction::DeleteCharBackward },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('r')), action: ModalAction::ReplaceChar },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('s')), action: ModalAction::SubstituteChar },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('S')), action: ModalAction::SubstituteLine },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('D')), action: ModalAction::DeleteToEndOfLine },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('C')), action: ModalAction::ChangeToEndOfLine },
    // -- put / undo / registers / repeat --
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('p')), action: ModalAction::Put(false) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('P')), action: ModalAction::Put(true) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('u')), action: ModalAction::Command(Command::Undo) },
    ModalBinding { mode: ModalMode::Normal, key: ctrl('r'), action: ModalAction::Command(Command::Redo) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('"')), action: ModalAction::SelectRegister },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('.')), action: ModalAction::RepeatLast },
    // -- search / command line --
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('/')), action: ModalAction::Command(Command::SearchForward) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('?')), action: ModalAction::Command(Command::SearchReverse) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char(':')), action: ModalAction::EnterMode(ModalMode::CommandLine) },
    // F5: run the line or selection in the terminal.
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::F(5)), action: ModalAction::Command(Command::TerminalSendInput) },
    // F6: describe the active buffer's state as one spoken-style line.
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::F(6)), action: ModalAction::Command(Command::DescribeState) },
    // -- visual --
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('v')), action: ModalAction::EnterMode(ModalMode::Visual) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('V')), action: ModalAction::VisualLine },
    // -- insert entry --
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('i')), action: ModalAction::EnterMode(ModalMode::Insert) },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('I')), action: ModalAction::InsertAtFirstNonBlank },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('a')), action: ModalAction::AppendAfterCursor },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('A')), action: ModalAction::AppendAtEndOfLine },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('o')), action: ModalAction::OpenLineBelow },
    ModalBinding { mode: ModalMode::Normal, key: plain(Key::Char('O')), action: ModalAction::OpenLineAbove },
    // -- insert mode --
    ModalBinding { mode: ModalMode::Insert, key: plain(Key::Esc), action: ModalAction::EnterMode(ModalMode::Normal) },
    // -- visual mode motions (extend the selection anchored at the mark) --
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('h')), action: ModalAction::Command(Command::BackwardCharacter) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('j')), action: ModalAction::Command(Command::NextLine) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('k')), action: ModalAction::Command(Command::PreviousLine) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('l')), action: ModalAction::Command(Command::ForwardCharacter) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('w')), action: ModalAction::WordForward },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('b')), action: ModalAction::WordBackward },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('e')), action: ModalAction::WordEndForward },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('W')), action: ModalAction::WordForwardBig },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('B')), action: ModalAction::WordBackwardBig },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('E')), action: ModalAction::WordEndForwardBig },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('(')), action: ModalAction::Command(Command::BackwardSentence) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char(')')), action: ModalAction::Command(Command::ForwardSentence) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('g')), action: ModalAction::PrefixG },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('i')), action: ModalAction::TextObjectPrefix { inner: true } },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('a')), action: ModalAction::TextObjectPrefix { inner: false } },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('0')), action: ModalAction::Command(Command::BeginningOfLine) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('$')), action: ModalAction::Command(Command::EndOfLine) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('{')), action: ModalAction::VimParagraphBackward },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('}')), action: ModalAction::VimParagraphForward },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('G')), action: ModalAction::Command(Command::EndOfFile) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('f')), action: ModalAction::FindChar(FindDir::Forward, FindEdge::On) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('F')), action: ModalAction::FindChar(FindDir::Backward, FindEdge::On) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('?')), action: ModalAction::Command(Command::SearchReverse) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('t')), action: ModalAction::FindChar(FindDir::Forward, FindEdge::Before) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('T')), action: ModalAction::FindChar(FindDir::Backward, FindEdge::Before) },
    // -- visual mode edits --
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('d')), action: ModalAction::Command(Command::KillRegion) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('x')), action: ModalAction::Command(Command::KillRegion) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('y')), action: ModalAction::Command(Command::CopyRegion) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('c')), action: ModalAction::Command(Command::KillRegion) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Char('v')), action: ModalAction::EnterMode(ModalMode::Normal) },
    ModalBinding { mode: ModalMode::Visual, key: plain(Key::Esc), action: ModalAction::EnterMode(ModalMode::Normal) },
];

// ---------------------------------------------------------------------------
// Modal continuation tables: multi-key sequences complete through
// config data, not engine match arms. The engine keeps the contextual logic
// (counts, operator-pending, operator application); these tables say which
// key completes the pending sequence and what it selects.
// ---------------------------------------------------------------------------

/// What the key after a `g` prefix selects. `GotoFirstLine` keeps the
/// special `gg`/`dgg`/`5gg` handling (operator linewise, count as line
/// number); word-end motions compose like any other motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalGAction {
    /// `gg`: first line (`dgg` is linewise, `5gg` goes to line 5).
    GotoFirstLine,
    /// `ge` / `gE`: backward to the previous word/WORD end (inclusive).
    WordEndBackward { big: bool },
    /// `g CTRL-G`: report the cursor position (byte, line, character).
    ShowPosition,
}

/// The `g`-prefix continuations: `gg`, `ge`, `gE`, `g CTRL-G`.
pub const MODAL_PREFIX_G: &[(KeyPress, ModalGAction)] = &[
    (
        KeyPress::plain(Key::Char('g')),
        ModalGAction::GotoFirstLine,
    ),
    (
        KeyPress::plain(Key::Char('e')),
        ModalGAction::WordEndBackward { big: false },
    ),
    (
        KeyPress::plain(Key::Char('E')),
        ModalGAction::WordEndBackward { big: true },
    ),
    (
        KeyPress::ctrl(Key::Char('g')),
        ModalGAction::ShowPosition,
    ),
];

/// What a text-object continuation key selects. Whether the object is inner
/// (`iw`) or around (`aw`) comes from the pending `i`/`a` key, not the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalTextObject {
    /// `w` / `W`.
    Word { big: bool },
    /// `(`, `)`, `b` → `(`, and the `[`/`{`/`<` families. `open` is the
    /// canonical opening bracket.
    Pair { open: char },
    /// `"` / `'`.
    Quote { q: char },
}

/// The text-object continuations after `i`/`a` (`iw`, `a(`, `i"`, …).
pub const MODAL_TEXT_OBJECTS: &[(KeyPress, ModalTextObject)] = &[
    (
        KeyPress::plain(Key::Char('w')),
        ModalTextObject::Word { big: false },
    ),
    (
        KeyPress::plain(Key::Char('W')),
        ModalTextObject::Word { big: true },
    ),
    (
        KeyPress::plain(Key::Char('(')),
        ModalTextObject::Pair { open: '(' },
    ),
    (
        KeyPress::plain(Key::Char(')')),
        ModalTextObject::Pair { open: '(' },
    ),
    (
        KeyPress::plain(Key::Char('b')),
        ModalTextObject::Pair { open: '(' },
    ),
    (
        KeyPress::plain(Key::Char('[')),
        ModalTextObject::Pair { open: '[' },
    ),
    (
        KeyPress::plain(Key::Char(']')),
        ModalTextObject::Pair { open: '[' },
    ),
    (
        KeyPress::plain(Key::Char('{')),
        ModalTextObject::Pair { open: '{' },
    ),
    (
        KeyPress::plain(Key::Char('}')),
        ModalTextObject::Pair { open: '{' },
    ),
    (
        KeyPress::plain(Key::Char('<')),
        ModalTextObject::Pair { open: '<' },
    ),
    (
        KeyPress::plain(Key::Char('>')),
        ModalTextObject::Pair { open: '<' },
    ),
    (
        KeyPress::plain(Key::Char('"')),
        ModalTextObject::Quote { q: '"' },
    ),
    (
        KeyPress::plain(Key::Char('\'')),
        ModalTextObject::Quote { q: '\'' },
    ),
];

// ---------------------------------------------------------------------------
// Standard bindings (N-268): modeless Notepad/VS Code style.
// ---------------------------------------------------------------------------

/// Standard (CUA-style) bindings: typing inserts; Ctrl+C/X/V clipboard,
/// Ctrl+S save, Ctrl+O open, Ctrl+N new, Ctrl+Z/Y and Ctrl+Shift+Z redo,
/// Ctrl+F find, Ctrl+H replace, Ctrl+A select-all, Ctrl+W close buffer,
/// Ctrl+Tab / Ctrl+Shift+Tab cycle buffers; Shift+arrows/Home/End/PgUp/PgDn
/// extend the selection; Home is smart (VS Code: first non-whitespace, then
/// column 0); Ctrl+Home/End jump to document start/end; Ctrl+Left/Right move
/// by word using the one word-boundary function.
pub const STANDARD_BINDINGS: &[KeyBinding] = &[
    // -- clipboard (CUA): act on the selection via the clipboard register --
    KeyBinding { keys: &[ctrl('c')], command: Command::CopyRegion, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('x')], command: Command::KillRegion, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('v')], command: Command::Yank, policy: PrefixPolicy::ExecuteLeaf },
    // -- file --
    KeyBinding { keys: &[ctrl('s')], command: Command::SaveBuffer, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('o')], command: Command::FindFile, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('n')], command: Command::NewBuffer, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('w')], command: Command::DeleteBuffer, policy: PrefixPolicy::ExecuteLeaf },
    // Ctrl+Q quits the editor (VS Code idiom); Ctrl+W above closes the buffer.
    KeyBinding { keys: &[ctrl('q')], command: Command::ExitErax, policy: PrefixPolicy::ExecuteLeaf },
    // -- undo/redo: both Ctrl+Y and Ctrl+Shift+Z redo --
    KeyBinding { keys: &[ctrl('z')], command: Command::Undo, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('y')], command: Command::Redo, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[KeyPress::mods(Key::Char('z'), true, false, true)], command: Command::Redo, policy: PrefixPolicy::ExecuteLeaf },
    // -- search --
    KeyBinding { keys: &[ctrl('f')], command: Command::SearchForward, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[ctrl('h')], command: Command::QueryReplace, policy: PrefixPolicy::ExecuteLeaf },
    // -- command palette: the Standard model's named-command facility,
    // the CUA-idiom counterpart to Emacs M-x and Vim `:` --
    KeyBinding { keys: &[KeyPress::mods(Key::Char('p'), true, false, true)], command: Command::ExecuteNamedCommand, policy: PrefixPolicy::ExecuteLeaf },
    // -- select all --
    KeyBinding { keys: &[ctrl('a')], command: Command::MarkWholeBuffer, policy: PrefixPolicy::ExecuteLeaf },
    // -- buffer cycling --
    KeyBinding { keys: &[KeyPress::ctrl(Key::Tab)], command: Command::NextBuffer, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[KeyPress::mods(Key::Tab, true, false, true)], command: Command::PreviousBuffer, policy: PrefixPolicy::ExecuteLeaf },
    // -- movement --
    KeyBinding { keys: &[plain(Key::Right)], command: Command::ForwardCharacter, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Left)], command: Command::BackwardCharacter, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Down)], command: Command::NextLine, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Up)], command: Command::PreviousLine, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Home)], command: Command::SmartHome, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::End)], command: Command::EndOfLine, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::PageUp)], command: Command::BackwardPage, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::PageDown)], command: Command::ForwardPage, policy: PrefixPolicy::ExecuteLeaf },
    // -- word-wise and document-wise --
    KeyBinding { keys: &[KeyPress::ctrl(Key::Left)], command: Command::BackwardWord, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[KeyPress::ctrl(Key::Right)], command: Command::ForwardWord, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[KeyPress::ctrl(Key::Home)], command: Command::BeginningOfFile, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[KeyPress::ctrl(Key::End)], command: Command::EndOfFile, policy: PrefixPolicy::ExecuteLeaf },
    // -- selection extension (Shift+...) --
    KeyBinding { keys: &[KeyPress::shift(Key::Right)], command: Command::ExtendSelectionRight, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[KeyPress::shift(Key::Left)], command: Command::ExtendSelectionLeft, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[KeyPress::shift(Key::Down)], command: Command::ExtendSelectionDown, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[KeyPress::shift(Key::Up)], command: Command::ExtendSelectionUp, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[KeyPress::shift(Key::Home)], command: Command::ExtendSelectionHome, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[KeyPress::shift(Key::End)], command: Command::ExtendSelectionEnd, policy: PrefixPolicy::ExecuteLeaf },
    // -- deletion --
    KeyBinding { keys: &[plain(Key::Backspace)], command: Command::DeletePreviousCharacter, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Delete)], command: Command::DeleteNextCharacter, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Enter)], command: Command::InsertNewline, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Tab)], command: Command::InsertTab, policy: PrefixPolicy::ExecuteLeaf },
    KeyBinding { keys: &[plain(Key::Insert)], command: Command::ToggleOverwriteMode, policy: PrefixPolicy::ExecuteLeaf },
    // -- menu --
    KeyBinding { keys: &[plain(Key::F(10))], command: Command::ToggleMenu, policy: PrefixPolicy::ExecuteLeaf },
    // -- terminal: the VS Code Ctrl+` idiom --
    KeyBinding { keys: &[ctrl('`')], command: Command::SpawnTerminal, policy: PrefixPolicy::ExecuteLeaf },
    // F5: the universal run key — send the line or selection to the terminal.
    KeyBinding { keys: &[plain(Key::F(5))], command: Command::TerminalSendInput, policy: PrefixPolicy::ExecuteLeaf },
    // F6: describe the active buffer's state as one spoken-style line.
    KeyBinding { keys: &[plain(Key::F(6))], command: Command::DescribeState, policy: PrefixPolicy::ExecuteLeaf },
    // -- spelling --
    KeyBinding { keys: &[plain(Key::F(7))], command: Command::SpellSuggest, policy: PrefixPolicy::ExecuteLeaf },
];

// ---------------------------------------------------------------------------
// Vim ex commands: the `:` vocabulary as data. The modal engine splits the
// command line into command and argument, looks the command up here, and
// interprets the action. Names missing from both tables fall through to the
// typed command table, so `:buffer-info` works without an ex entry.
// ---------------------------------------------------------------------------

/// What a vim `:` ex command does. Plain commands and command compositions
/// are data; the engine interprets the variants. The `:q` refusal logic
/// (modified buffers) stays in the engine: it is a state decision, not a
/// mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExAction {
    /// Run one command (`:w` saves the buffer).
    Run(Command),
    /// Run commands in order, returning the last outcome (`:wq`).
    RunSequence(&'static [Command]),
    /// `:q`: quit, refusing when any buffer is modified.
    ConditionalQuit,
}

/// The vim ex-command vocabulary: command name to action.
pub const EX_COMMANDS: &[(&str, ExAction)] = &[
    ("w", ExAction::Run(Command::SaveBuffer)),
    ("q", ExAction::ConditionalQuit),
    ("q!", ExAction::Run(Command::ExitWithoutSave)),
    ("wq", ExAction::RunSequence(&[Command::SaveBuffer, Command::ExitErax])),
    ("x", ExAction::RunSequence(&[Command::SaveBuffer, Command::ExitErax])),
    ("terminal", ExAction::Run(Command::SpawnTerminal)),
    ("ls", ExAction::Run(Command::ListBuffers)),
    ("buffers", ExAction::Run(Command::ListBuffers)),
    ("file", ExAction::Run(Command::BufferInfo)),
    ("describe", ExAction::Run(Command::DescribeState)),
    ("hardcopy", ExAction::Run(Command::PrintBuffer)),
];

/// What an ex command with a path argument does (`:w <file>`, `:e <file>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExArgAction {
    /// `:w <file>`: save the buffer under a new path.
    SaveAs,
    /// `:e <file>`: open a file.
    Open,
}

/// Ex commands that take a path argument. The engine only consults this
/// table when the command line carries a non-empty argument.
pub const EX_ARG_COMMANDS: &[(&str, ExArgAction)] = &[
    ("w", ExArgAction::SaveAs),
    ("e", ExArgAction::Open),
];

// ---------------------------------------------------------------------------
// Context key tables: single-key maps for special buffer kinds. The event
// loop consults these before the model tables when the active buffer is a
// diagnostics or diff view. Terminal buffers forward keys to the pty
// instead; that forwarding policy is engine behavior, not a mapping.
// ---------------------------------------------------------------------------

/// Keys active in a diagnostics buffer (`j`/`k` move, Enter jumps to the
/// diagnostic, `q` closes the window).
pub const DIAGNOSTICS_KEYS: &[(KeyPress, Command)] = &[
    (KeyPress::plain(Key::Char('j')), Command::NextLine),
    (KeyPress::plain(Key::Down), Command::NextLine),
    (KeyPress::plain(Key::Char('k')), Command::PreviousLine),
    (KeyPress::plain(Key::Up), Command::PreviousLine),
    (KeyPress::plain(Key::Enter), Command::DiagnosticsJump),
    (KeyPress::plain(Key::Char('q')), Command::DeleteWindow),
    (KeyPress::plain(Key::Esc), Command::DeleteWindow),
];

/// Keys active in a diff view (`j`/`k`/`n` move between hunks, `y` accepts
/// the hunk, `q` quits the diff).
pub const DIFF_KEYS: &[(KeyPress, Command)] = &[
    (KeyPress::plain(Key::Char('j')), Command::DiffNextHunk),
    (KeyPress::plain(Key::Char('n')), Command::DiffNextHunk),
    (KeyPress::plain(Key::Char('k')), Command::DiffPreviousHunk),
    (KeyPress::plain(Key::Char('y')), Command::DiffAcceptHunk),
    (KeyPress::plain(Key::Char('q')), Command::DiffQuit),
];

// ---------------------------------------------------------------------------
// Named-facility bindings: commands with no key idiom in a model, reachable
// only by name through that model's named facility. Recorded as data so a
// facility-only command is a deliberate inventory entry, never an implicit
// gap. Giving one of these commands a key binding means deleting it here.
// ---------------------------------------------------------------------------

/// Commands reachable only by name in a model, through the named facility.
/// The facility resolves names through the typed command table, so the
/// entry is the record that the command is intentionally keyless there.
pub const FACILITY_BINDINGS: &[(InputModel, &str, &[Command])] = &[
    (
        InputModel::KeyTrie,
        "M-x",
        &[
            Command::GotoByte,
            Command::MarkLine,
            Command::WrapWord,
            Command::WhatCursorPosition,
        ],
    ),
    (
        InputModel::Modal,
        ":",
        &[
            Command::GotoByte,
            Command::MarkLine,
            Command::WrapWord,
            Command::WhatCursorPosition,
        ],
    ),
    (
        InputModel::Standard,
        "palette",
        &[
            Command::GotoByte,
            Command::MarkLine,
            Command::WrapWord,
            Command::WhatCursorPosition,
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_for_model_selects_each_table() {
        // `const` statics materialize per use-site, so compare by content,
        // not pointer identity.
        let keytrie = bindings_for_model(InputModel::KeyTrie);
        assert_eq!(keytrie.len(), KEYBINDINGS.len());
        assert!(keytrie
            .iter()
            .zip(KEYBINDINGS.iter())
            .all(|(a, b)| a == b));
        let standard = bindings_for_model(InputModel::Standard);
        assert_eq!(standard.len(), STANDARD_BINDINGS.len());
        assert!(standard
            .iter()
            .zip(STANDARD_BINDINGS.iter())
            .all(|(a, b)| a == b));
        // The trie underneath the modal engine is the key-trie table:
        // ctrl/alt chords pass through the engine to it.
        let modal = bindings_for_model(InputModel::Modal);
        assert_eq!(modal.len(), KEYBINDINGS.len());
    }
}
