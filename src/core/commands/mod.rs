//! Command implementations for the erax editor
//!
//! This module contains all editor commands organized into logical sub-modules:
//!
//! - **movement**: Cursor navigation (forward-char, next-line, forward-word, etc.)
//! - **marks**: Mark management and region operations (set-mark, kill-region, etc.)
//! - **editing**: Basic text editing (insert-newline, delete-char, etc.)
//! - **window**: Window management (split-window, delete-window, etc.)
//! - **file**: File operations (save-buffer, find-file, kill-buffer, etc.)
//! - **search**: Search and navigation (search-forward, goto-line, etc.)
//! - **kill_ring**: Kill ring operations (kill-line, yank, transpose-words, etc.)
//! - **macro_cmd**: Macro recording and playback (begin-macro, execute-macro, etc.)
//! - **buffer**: Buffer introspection (buffer-info, count-words, etc.)
//! - **text**: Text transformation (justify-paragraph, upper-word, etc.)
//! - **control**: Application control (exit, etc.)
//!
//! All commands implement the [`Command`](crate::core::command::Command) trait,
//! which defines a uniform interface for command execution.

/// Buffer introspection
pub mod buffer;
/// Programming calculator
pub mod calculator;
/// Word completion
pub mod completion;
/// Application control
pub mod control;
/// Diagnostics pane
pub mod diagnostics;
/// Diff and Sed preview
pub mod diff;
/// Basic editing (insert, delete)
pub mod editing;
/// Expand selection to syntactic units
pub mod expand_selection;
/// File operations
pub mod file;
/// Kill ring and yank
pub mod kill_ring;
/// Macro recording
pub mod macro_cmd;
/// Mark and region commands
pub mod marks;
/// Cursor movement commands
pub mod movement;
/// Printing commands
pub mod print;
/// Search and navigation
pub mod search;
/// Code snippets
pub mod snippets;
/// Spell suggestions
pub mod spell_suggest;
/// Terminal emulation
pub mod terminal;
/// Text transformation
pub mod text;
/// Undo/redo operations
pub mod undo;
/// Window management
pub mod window;

/// Register all commands in the application
pub fn register_all(app: &mut crate::core::app::EditorApp) {
    use crate::core::command::Command;
    use std::collections::HashMap;

    // Import all command modules
    use self::buffer::*;
    use self::calculator::*;
    use self::completion::*;
    use self::control::*;
    use self::diagnostics::*;
    use self::diff::*;
    use self::editing::*;
    use self::expand_selection::*;
    use self::file::*;
    use self::kill_ring::*;
    use self::macro_cmd::*;
    use self::marks::*;
    use self::movement::*;
    use self::print::*;
    use self::search::*;
    use self::snippets::*;
    use self::spell_suggest::*;
    use self::terminal::*;
    use self::text::*;
    use self::undo::*;
    use self::window::*;

    let mut registry: HashMap<String, Box<dyn Command>> = HashMap::new();

    // Movement commands
    registry.insert("forward-character".to_string(), Box::new(ForwardChar));
    registry.insert("backward-character".to_string(), Box::new(BackwardChar));
    registry.insert("next-line".to_string(), Box::new(NextLine));
    registry.insert("previous-line".to_string(), Box::new(PreviousLine));
    registry.insert("beginning-of-line".to_string(), Box::new(BeginningOfLine));
    registry.insert("end-of-line".to_string(), Box::new(EndOfLine));
    registry.insert("beginning-of-file".to_string(), Box::new(BeginningOfBuffer));
    registry.insert("end-of-file".to_string(), Box::new(EndOfBuffer));
    registry.insert("forward-word".to_string(), Box::new(ForwardWord));
    registry.insert("backward-word".to_string(), Box::new(BackwardWord));
    registry.insert("forward-page".to_string(), Box::new(ForwardPage));
    registry.insert("backward-page".to_string(), Box::new(BackwardPage));
    registry.insert("forward-paragraph".to_string(), Box::new(ForwardParagraph));
    registry.insert(
        "backward-paragraph".to_string(),
        Box::new(BackwardParagraph),
    );

    // Mark and region commands
    registry.insert("set-mark".to_string(), Box::new(SetMark));
    registry.insert(
        "exchange-point-and-mark".to_string(),
        Box::new(ExchangePointAndMark),
    );
    registry.insert("kill-region".to_string(), Box::new(KillRegion));
    registry.insert("copy-region".to_string(), Box::new(CopyRegion));
    registry.insert("mark-word".to_string(), Box::new(MarkWord));
    registry.insert("mark-line".to_string(), Box::new(MarkLine));
    registry.insert("mark-paragraph".to_string(), Box::new(MarkParagraph));
    registry.insert(
        "expand-selection".to_string(),
        Box::new(ExpandSelection::new()),
    );

    // Editing commands
    registry.insert("open-line".to_string(), Box::new(OpenLine));
    registry.insert("insert-newline".to_string(), Box::new(InsertNewline));
    registry.insert("insert-tab".to_string(), Box::new(InsertTab));
    registry.insert(
        "delete-previous-character".to_string(),
        Box::new(DeleteBackwardChar),
    );
    registry.insert(
        "delete-backward-char".to_string(),
        Box::new(DeleteBackwardChar),
    ); // uEmacs alias
    registry.insert(
        "delete-next-character".to_string(),
        Box::new(DeleteForwardChar),
    );
    registry.insert("delete-char".to_string(), Box::new(DeleteForwardChar)); // Alias often used
    registry.insert("transpose-characters".to_string(), Box::new(TransposeChars));
    registry.insert(
        "toggle-overwrite-mode".to_string(),
        Box::new(ToggleOverwriteMode),
    );
    registry.insert("insert-space".to_string(), Box::new(InsertSpace));
    registry.insert("redraw-display".to_string(), Box::new(RedrawDisplay));
    registry.insert("quote-character".to_string(), Box::new(QuoteCharacter));
    registry.insert("newline-and-indent".to_string(), Box::new(NewlineAndIndent));

    // Window commands
    registry.insert(
        "split-current-window".to_string(),
        Box::new(SplitWindowVertically),
    );
    registry.insert(
        "split-window-horizontally".to_string(),
        Box::new(SplitWindowHorizontally), // Not in uEmacs standard but good to keep
    );
    registry.insert(
        "delete-other-windows".to_string(),
        Box::new(DeleteOtherWindows),
    );
    registry.insert("delete-window".to_string(), Box::new(DeleteWindow));
    registry.insert("next-window".to_string(), Box::new(OtherWindow)); // "next-window" in uEmacs
    registry.insert("minimize-window".to_string(), Box::new(MinimizeWindow));
    registry.insert("window-picker".to_string(), Box::new(WindowPicker));
    registry.insert("grow-window".to_string(), Box::new(GrowWindow));
    registry.insert("shrink-window".to_string(), Box::new(ShrinkWindow));

    // File commands
    registry.insert("save-buffer".to_string(), Box::new(SaveBuffer));
    registry.insert("find-file".to_string(), Box::new(FindFile));
    registry.insert("write-file".to_string(), Box::new(WriteFile));
    registry.insert("read-file".to_string(), Box::new(ReadFile));
    registry.insert("delete-buffer".to_string(), Box::new(KillBuffer));
    registry.insert("select-buffer".to_string(), Box::new(SwitchToBuffer));
    registry.insert("print-buffer".to_string(), Box::new(PrintBuffer));
    registry.insert("print".to_string(), Box::new(PrintCommand));
    registry.insert("next-buffer".to_string(), Box::new(NextBuffer));
    registry.insert("previous-buffer".to_string(), Box::new(PreviousBuffer));
    registry.insert("new-buffer".to_string(), Box::new(NewBuffer)); // Keep as modern convenience
    registry.insert("switch-to-buffer".to_string(), Box::new(SwitchToBuffer)); // uEmacs alias

    // Search commands
    registry.insert("search-forward".to_string(), Box::new(SearchForward));
    registry.insert("search-reverse".to_string(), Box::new(SearchBackward));
    registry.insert("search-backward".to_string(), Box::new(SearchBackward)); // uEmacs alias
    registry.insert("query-replace".to_string(), Box::new(QueryReplace));
    registry.insert("goto-line".to_string(), Box::new(GotoLine));
    registry.insert("expand-snippet".to_string(), Box::new(ExpandSnippet));
    registry.insert("spell-suggest".to_string(), Box::new(SpellSuggest));

    // Kill ring commands
    registry.insert("kill-to-end-of-line".to_string(), Box::new(KillLine));
    registry.insert("kill-word".to_string(), Box::new(KillWord));
    registry.insert("backward-kill-word".to_string(), Box::new(BackwardKillWord));
    registry.insert("yank".to_string(), Box::new(Yank));
    registry.insert("yank-pop".to_string(), Box::new(YankPop));
    registry.insert("transpose-words".to_string(), Box::new(TransposeWords));

    // Macro commands
    registry.insert("begin-macro".to_string(), Box::new(BeginMacro));
    registry.insert("end-macro".to_string(), Box::new(EndMacro));
    registry.insert("execute-macro".to_string(), Box::new(ExecuteMacro));

    // Undo/Redo commands
    registry.insert("undo".to_string(), Box::new(Undo));
    registry.insert("redo".to_string(), Box::new(Redo));

    // Buffer commands
    registry.insert("buffer-info".to_string(), Box::new(BufferInfo));
    registry.insert(
        "what-cursor-position".to_string(),
        Box::new(WhatCursorPosition),
    );
    registry.insert("show-position".to_string(), Box::new(ShowPosition));
    registry.insert("count-words".to_string(), Box::new(CountWords));
    registry.insert("goto-byte".to_string(), Box::new(GotoByte));
    registry.insert("list-buffers".to_string(), Box::new(ListBuffers));
    registry.insert("calculator".to_string(), Box::new(CalculatorCommand));
    registry.insert("word-completion".to_string(), Box::new(WordCompletion));
    registry.insert(
        "toggle-diagnostics".to_string(),
        Box::new(ToggleDiagnosticsPane),
    );
    registry.insert("diagnostics-jump".to_string(), Box::new(DiagnosticsJump));
    registry.insert("diagnostics-next".to_string(), Box::new(DiagnosticsNext));
    registry.insert(
        "diagnostics-previous".to_string(),
        Box::new(DiagnosticsPrevious),
    );

    // Text manipulation commands
    registry.insert("justify-paragraph".to_string(), Box::new(JustifyParagraph));
    registry.insert("delete-blank-lines".to_string(), Box::new(DeleteBlankLines));
    registry.insert(
        "goto-matching-fence".to_string(),
        Box::new(GotoMatchingFence),
    );
    registry.insert("case-word-upper".to_string(), Box::new(UpperWord));
    registry.insert("case-word-lower".to_string(), Box::new(LowerWord));
    registry.insert("case-word-capitalize".to_string(), Box::new(CapitalizeWord));
    registry.insert("uppercase-region".to_string(), Box::new(UppercaseRegion));
    registry.insert("lowercase-region".to_string(), Box::new(LowercaseRegion));
    registry.insert("shell-command".to_string(), Box::new(ShellCommand));
    registry.insert("filter-buffer".to_string(), Box::new(FilterBuffer));
    registry.insert("wrap-word".to_string(), Box::new(WrapWord));

    // Terminal commands
    registry.insert("spawn-terminal".to_string(), Box::new(SpawnTerminal));
    registry.insert(
        "terminal-send-input".to_string(),
        Box::new(TerminalSendInput),
    );
    registry.insert(
        "split-spawn-terminal-vertical".to_string(),
        Box::new(SplitSpawnTerminalVertical),
    );
    registry.insert(
        "split-spawn-terminal-horizontal".to_string(),
        Box::new(SplitSpawnTerminalHorizontal),
    );

    // Control commands
    registry.insert("exit-erax".to_string(), Box::new(Exit));
    registry.insert("exit-without-save".to_string(), Box::new(ExitWithoutSave));
    registry.insert("exit-and-save".to_string(), Box::new(ExitAndSave));
    registry.insert(
        "universal-argument".to_string(),
        Box::new(UniversalArgument),
    );
    registry.insert("keyboard-quit".to_string(), Box::new(KeyboardQuit));
    registry.insert("describe-key".to_string(), Box::new(DescribeKey));
    registry.insert("execute-named-command".to_string(), Box::new(NamedCommand));

    // Diff commands
    registry.insert("sed-preview".to_string(), Box::new(SedPreviewCommand));
    registry.insert("diff-next-hunk".to_string(), Box::new(DiffNextHunk));
    registry.insert("diff-previous-hunk".to_string(), Box::new(DiffPrevHunk));
    registry.insert("diff-accept-hunk".to_string(), Box::new(DiffAcceptHunk));
    registry.insert("diff-quit".to_string(), Box::new(DiffQuit));

    app.command_registry = registry;
}
