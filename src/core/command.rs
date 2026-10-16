//! Typed editor commands.
//!
//! Commands are typed values, not strings (N-155, N-205): there is no stringly
//! registry, no `Box<dyn Command>` trait objects, no clone machinery, and no
//! "unknown command" stderr path. Dispatch is an exhaustive match over this
//! enum in `crate::core::dispatcher`. Each command's kebab-case name
//! (`Command::name`) is what keybindings and `run-command` use; the
//! four historical aliases (`delete-backward-char`, `delete-char`,
//! `search-backward`, `switch-to-buffer`) are deleted — each command is bound
//! once under its canonical name (N-205).

/// Every command the editor can execute, as a typed value (N-155).
///
/// Variants are grouped by area. The canonical kebab-case name comes from
/// [`Command::name`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    // -- movement --
    ForwardCharacter,
    BackwardCharacter,
    NextLine,
    PreviousLine,
    BeginningOfLine,
    /// Smart Home (VS Code): first non-whitespace, then column 0.
    SmartHome,
    EndOfLine,
    BeginningOfFile,
    EndOfFile,
    ForwardWord,
    BackwardWord,
    ForwardPage,
    BackwardPage,
    ForwardParagraph,
    BackwardParagraph,
    ForwardSentence,
    BackwardSentence,
    GotoLine,
    GotoByte,
    GotoMatchingFence,
    // -- editing --
    OpenLine,
    InsertNewline,
    InsertTab,
    InsertSpace,
    DeletePreviousCharacter,
    DeleteNextCharacter,
    TransposeCharacters,
    TransposeWords,
    ToggleOverwriteMode,
    RedrawDisplay,
    QuoteCharacter,
    NewlineAndIndent,
    // -- kill ring --
    KillToEndOfLine,
    KillWord,
    BackwardKillWord,
    KillRegion,
    CopyRegion,
    Yank,
    YankPop,
    // -- marks --
    SetMark,
    ExchangePointAndMark,
    MarkWord,
    MarkLine,
    MarkParagraph,
    MarkWholeBuffer,
    ExtendSelectionRight,
    ExtendSelectionLeft,
    ExtendSelectionDown,
    ExtendSelectionUp,
    ExtendSelectionHome,
    ExtendSelectionEnd,
    ExpandSelection,
    // -- case --
    CaseWordUpper,
    CaseWordLower,
    CaseWordCapitalize,
    UppercaseRegion,
    LowercaseRegion,
    // -- text --
    JustifyParagraph,
    WrapWord,
    // -- search --
    SearchForward,
    SearchReverse,
    QueryReplace,
    // -- buffers --
    NewBuffer,
    ListBuffers,
    BufferInfo,
    NextBuffer,
    PreviousBuffer,
    SelectBuffer,
    DeleteBuffer,
    ShowPosition,
    WhatCursorPosition,
    CountWords,
    DeleteBlankLines,
    // -- files --
    FindFile,
    SaveBuffer,
    WriteFile,
    ReadFile,
    ExitErax,
    ExitWithoutSave,
    ExitAndSave,
    Print,
    PrintBuffer,
    // -- windows --
    SplitCurrentWindow,
    SplitWindowHorizontally,
    DeleteOtherWindows,
    DeleteWindow,
    NextWindow,
    MinimizeWindow,
    WindowPicker,
    GrowWindow,
    ShrinkWindow,
    // -- control --
    KeyboardQuit,
    UniversalArgument,
    ExecuteNamedCommand,
    DescribeKey,
    /// Toggle the menu bar (operates on display state; intercepted by the
    /// event loop before dispatch, N-043).
    ToggleMenu,
    // -- macros --
    BeginMacro,
    EndMacro,
    ExecuteMacro,
    // -- undo --
    Undo,
    Redo,
    // -- completion / snippets / spell --
    WordCompletion,
    ExpandSnippet,
    SpellSuggest,
    // -- calculator / shell --
    Calculator,
    ShellCommand,
    FilterBuffer,
    // -- terminal --
    SpawnTerminal,
    SplitSpawnTerminalVertical,
    SplitSpawnTerminalHorizontal,
    TerminalSendInput,
    // -- diagnostics --
    ToggleDiagnostics,
    DiagnosticsNext,
    DiagnosticsPrevious,
    DiagnosticsJump,
    // -- diff / sed --
    SedPreview,
    DiffAcceptHunk,
    DiffNextHunk,
    DiffPreviousHunk,
    DiffQuit,
    // -- accessibility --
    /// Describe the active buffer's state as one spoken-style line
    /// (buffer, position, mode, selection, diagnostics).
    DescribeState,
}

impl Command {
    /// Every command variant, for inventory tests.
    pub const ALL: &[Command] = &[
        Command::ForwardCharacter,
        Command::BackwardCharacter,
        Command::NextLine,
        Command::PreviousLine,
        Command::BeginningOfLine,
        Command::SmartHome,
        Command::EndOfLine,
        Command::BeginningOfFile,
        Command::EndOfFile,
        Command::ForwardWord,
        Command::BackwardWord,
        Command::ForwardPage,
        Command::BackwardPage,
        Command::ForwardParagraph,
        Command::BackwardParagraph,
        Command::ForwardSentence,
        Command::BackwardSentence,
        Command::GotoLine,
        Command::GotoByte,
        Command::GotoMatchingFence,
        Command::OpenLine,
        Command::InsertNewline,
        Command::InsertTab,
        Command::InsertSpace,
        Command::DeletePreviousCharacter,
        Command::DeleteNextCharacter,
        Command::TransposeCharacters,
        Command::TransposeWords,
        Command::ToggleOverwriteMode,
        Command::RedrawDisplay,
        Command::QuoteCharacter,
        Command::NewlineAndIndent,
        Command::KillToEndOfLine,
        Command::KillWord,
        Command::BackwardKillWord,
        Command::KillRegion,
        Command::CopyRegion,
        Command::Yank,
        Command::YankPop,
        Command::SetMark,
        Command::ExchangePointAndMark,
        Command::MarkWord,
        Command::MarkLine,
        Command::MarkParagraph,
        Command::MarkWholeBuffer,
        Command::ExtendSelectionRight,
        Command::ExtendSelectionLeft,
        Command::ExtendSelectionDown,
        Command::ExtendSelectionUp,
        Command::ExtendSelectionHome,
        Command::ExtendSelectionEnd,
        Command::ExpandSelection,
        Command::CaseWordUpper,
        Command::CaseWordLower,
        Command::CaseWordCapitalize,
        Command::UppercaseRegion,
        Command::LowercaseRegion,
        Command::JustifyParagraph,
        Command::WrapWord,
        Command::SearchForward,
        Command::SearchReverse,
        Command::QueryReplace,
        Command::NewBuffer,
        Command::ListBuffers,
        Command::BufferInfo,
        Command::NextBuffer,
        Command::PreviousBuffer,
        Command::SelectBuffer,
        Command::DeleteBuffer,
        Command::ShowPosition,
        Command::WhatCursorPosition,
        Command::CountWords,
        Command::DeleteBlankLines,
        Command::FindFile,
        Command::SaveBuffer,
        Command::WriteFile,
        Command::ReadFile,
        Command::ExitErax,
        Command::ExitWithoutSave,
        Command::ExitAndSave,
        Command::Print,
        Command::PrintBuffer,
        Command::SplitCurrentWindow,
        Command::SplitWindowHorizontally,
        Command::DeleteOtherWindows,
        Command::DeleteWindow,
        Command::NextWindow,
        Command::MinimizeWindow,
        Command::WindowPicker,
        Command::GrowWindow,
        Command::ShrinkWindow,
        Command::KeyboardQuit,
        Command::UniversalArgument,
        Command::ExecuteNamedCommand,
        Command::DescribeKey,
        Command::ToggleMenu,
        Command::BeginMacro,
        Command::EndMacro,
        Command::ExecuteMacro,
        Command::Undo,
        Command::Redo,
        Command::WordCompletion,
        Command::ExpandSnippet,
        Command::SpellSuggest,
        Command::Calculator,
        Command::ShellCommand,
        Command::FilterBuffer,
        Command::SpawnTerminal,
        Command::SplitSpawnTerminalVertical,
        Command::SplitSpawnTerminalHorizontal,
        Command::TerminalSendInput,
        Command::ToggleDiagnostics,
        Command::DiagnosticsNext,
        Command::DiagnosticsPrevious,
        Command::DiagnosticsJump,
        Command::SedPreview,
        Command::DiffAcceptHunk,
        Command::DiffNextHunk,
        Command::DiffPreviousHunk,
        Command::DiffQuit,
        Command::DescribeState,
    ];

    /// The canonical kebab-case name, used by keybindings and
    /// `run-command`. Each command has exactly one name (N-205).
    pub fn name(self) -> &'static str {
        match self {
            Command::ForwardCharacter => "cursor-right",
            Command::BackwardCharacter => "cursor-left",
            Command::NextLine => "cursor-down",
            Command::PreviousLine => "cursor-up",
            Command::BeginningOfLine => "line-start",
            Command::SmartHome => "smart-home",
            Command::EndOfLine => "line-end",
            Command::BeginningOfFile => "file-start",
            Command::EndOfFile => "file-end",
            Command::ForwardWord => "word-right",
            Command::BackwardWord => "word-left",
            Command::ForwardPage => "page-down",
            Command::BackwardPage => "page-up",
            Command::ForwardParagraph => "next-paragraph",
            Command::BackwardParagraph => "previous-paragraph",
            Command::ForwardSentence => "next-sentence",
            Command::BackwardSentence => "previous-sentence",
            Command::GotoLine => "go-to-line",
            Command::GotoByte => "go-to-byte",
            Command::GotoMatchingFence => "go-to-matching-bracket",
            Command::OpenLine => "open-line-below",
            Command::InsertNewline => "insert-newline",
            Command::InsertTab => "insert-tab",
            Command::InsertSpace => "insert-space",
            Command::DeletePreviousCharacter => "delete-backward",
            Command::DeleteNextCharacter => "delete-forward",
            Command::TransposeCharacters => "swap-characters",
            Command::TransposeWords => "swap-words",
            Command::ToggleOverwriteMode => "toggle-overwrite",
            Command::RedrawDisplay => "refresh-display",
            Command::QuoteCharacter => "insert-literal",
            Command::NewlineAndIndent => "newline-indent",
            Command::KillToEndOfLine => "cut-to-line-end",
            Command::KillWord => "cut-word-forward",
            Command::BackwardKillWord => "cut-word-backward",
            Command::KillRegion => "cut-selection",
            Command::CopyRegion => "copy-selection",
            Command::Yank => "paste",
            Command::YankPop => "cycle-paste",
            Command::SetMark => "set-selection-anchor",
            Command::ExchangePointAndMark => "swap-selection-ends",
            Command::MarkWord => "select-word",
            Command::MarkLine => "select-line",
            Command::MarkParagraph => "select-paragraph",
            Command::MarkWholeBuffer => "select-all",
            Command::ExtendSelectionRight => "extend-selection-right",
            Command::ExtendSelectionLeft => "extend-selection-left",
            Command::ExtendSelectionDown => "extend-selection-down",
            Command::ExtendSelectionUp => "extend-selection-up",
            Command::ExtendSelectionHome => "extend-selection-home",
            Command::ExtendSelectionEnd => "extend-selection-end",
            Command::ExpandSelection => "expand-selection",
            Command::CaseWordUpper => "uppercase-word",
            Command::CaseWordLower => "lowercase-word",
            Command::CaseWordCapitalize => "capitalize-word",
            Command::UppercaseRegion => "uppercase-selection",
            Command::LowercaseRegion => "lowercase-selection",
            Command::JustifyParagraph => "justify-paragraph",
            Command::WrapWord => "wrap-word",
            Command::SearchForward => "find-forward",
            Command::SearchReverse => "find-backward",
            Command::QueryReplace => "find-and-replace",
            Command::NewBuffer => "new-buffer",
            Command::ListBuffers => "list-buffers",
            Command::BufferInfo => "buffer-info",
            Command::NextBuffer => "next-buffer",
            Command::PreviousBuffer => "previous-buffer",
            Command::SelectBuffer => "select-buffer",
            Command::DeleteBuffer => "close-buffer",
            Command::ShowPosition => "show-position",
            Command::WhatCursorPosition => "cursor-position",
            Command::CountWords => "word-count",
            Command::DeleteBlankLines => "delete-blank-lines",
            Command::FindFile => "open-file",
            Command::SaveBuffer => "save",
            Command::WriteFile => "save-as",
            Command::ReadFile => "insert-file",
            Command::ExitErax => "quit",
            Command::ExitWithoutSave => "quit-without-saving",
            Command::ExitAndSave => "quit-and-save",
            Command::Print => "print",
            Command::PrintBuffer => "print-buffer",
            Command::SplitCurrentWindow => "split-window-down",
            Command::SplitWindowHorizontally => "split-window-right",
            Command::DeleteOtherWindows => "close-other-windows",
            Command::DeleteWindow => "close-window",
            Command::NextWindow => "next-window",
            Command::MinimizeWindow => "minimize-window",
            Command::WindowPicker => "window-picker",
            Command::GrowWindow => "grow-window",
            Command::ShrinkWindow => "shrink-window",
            Command::KeyboardQuit => "cancel",
            Command::UniversalArgument => "prefix-argument",
            Command::ExecuteNamedCommand => "run-command",
            Command::DescribeKey => "describe-key",
            Command::ToggleMenu => "toggle-menu",
            Command::BeginMacro => "record-macro",
            Command::EndMacro => "stop-macro",
            Command::ExecuteMacro => "play-macro",
            Command::Undo => "undo",
            Command::Redo => "redo",
            Command::WordCompletion => "complete-word",
            Command::ExpandSnippet => "expand-snippet",
            Command::SpellSuggest => "spelling-suggestions",
            Command::Calculator => "calculator",
            Command::ShellCommand => "run-shell-command",
            Command::FilterBuffer => "filter-buffer",
            Command::SpawnTerminal => "open-terminal",
            Command::SplitSpawnTerminalVertical => "open-terminal-below",
            Command::SplitSpawnTerminalHorizontal => "open-terminal-right",
            Command::TerminalSendInput => "send-to-terminal",
            Command::ToggleDiagnostics => "toggle-diagnostics",
            Command::DiagnosticsNext => "next-diagnostic",
            Command::DiagnosticsPrevious => "previous-diagnostic",
            Command::DiagnosticsJump => "go-to-diagnostic",
            Command::SedPreview => "sed-preview",
            Command::DiffAcceptHunk => "diff-accept-hunk",
            Command::DiffNextHunk => "diff-next-hunk",
            Command::DiffPreviousHunk => "diff-previous-hunk",
            Command::DiffQuit => "close-diff",
            Command::DescribeState => "describe-state",
        }
    }

    /// Look up a command by its canonical name (for `run-command`).
    /// Returns `None` for unknown names — the caller reports it through the
    /// typed message path, never stderr (N-155).
    pub fn from_name(name: &str) -> Option<Command> {
        for cmd in Command::all() {
            if cmd.name() == name {
                return Some(*cmd);
            }
        }
        None
    }

    /// All commands, in a stable order.
    pub fn all() -> &'static [Command] {
        Self::ALL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_round_trip() {
        let all = Command::all();
        let mut seen = std::collections::HashSet::new();
        for cmd in all {
            assert!(seen.insert(cmd.name()), "duplicate name: {}", cmd.name());
            assert_eq!(Command::from_name(cmd.name()), Some(*cmd));
        }
    }

    #[test]
    fn unknown_name_is_none() {
        assert_eq!(Command::from_name("not-a-command"), None);
        // Deleted aliases are unknown
        assert_eq!(Command::from_name("delete-backward-char"), None);
        assert_eq!(Command::from_name("delete-char"), None);
        assert_eq!(Command::from_name("search-backward"), None);
        assert_eq!(Command::from_name("switch-to-buffer"), None);
    }

    #[test]
    fn every_bound_config_command_exists() {
        // Spot-check commands the default keybindings rely on.
        for name in [
            "cursor-right",
            "save",
            "run-command",
            "cancel",
            "prefix-argument",
            "open-terminal-below",
            "toggle-diagnostics",
            "describe-key",
        ] {
            assert!(Command::from_name(name).is_some(), "{name} must exist");
        }
    }
}
