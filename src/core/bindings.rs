//! Schema of the binding tables: the types the key tables are written in.
//!
//! The tables themselves are data in [`crate::config::keys`]; this module
//! holds their vocabulary. Engines interpret the tables and never hardcode
//! key-to-command or word-to-command mappings.

use crate::core::command::Command;
use crate::core::input::{KeyPress, PrefixPolicy};

/// A single typed keybinding: a key sequence, its command, and the
/// leaf/prefix policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyBinding {
    /// The key sequence; usually one key, sometimes several (Ctrl+x Ctrl+f).
    pub keys: &'static [KeyPress],
    /// The typed command to run.
    pub command: Command,
    /// What to do when this binding is both a leaf and a prefix.
    pub policy: PrefixPolicy,
}

/// Compile-time input model switch.
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

/// Modal editing mode.
///
/// OperatorPending is entered after `d`/`y`/`c`; the next key is a motion
/// (or the same operator key for a linewise operation).
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

/// A composable vim editing operator.
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

/// What a modal keypress does.
///
/// `Command` variants compose over the existing typed [`Command`] set: the
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
    /// Vim `}`: forward to the next paragraph boundary (vim-style
    /// paragraph motion, not the uemacs [`Command::ForwardParagraph`]).
    VimParagraphForward,
    /// Vim `{`: backward to the previous paragraph boundary.
    VimParagraphBackward,
    /// Vim `$` in Normal mode: onto the last character of the line. The
    /// shared [`Command::EndOfLine`] stops one past the last character:
    /// that is the cursor-exclusive visual selection's end, not a
    /// Normal-mode cursor. As an operator motion `$` is the to-end-of-line
    /// motion.
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

/// What an ex command with a path argument does (`:w <file>`, `:e <file>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExArgAction {
    /// `:w <file>`: save the buffer under a new path.
    SaveAs,
    /// `:e <file>`: open a file.
    Open,
}
