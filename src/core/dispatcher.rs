//! Typed command dispatch.
//!
//! Commands are typed [`Command`] values dispatched through one exhaustive
//! match over pure functions (N-155): no string registry, no trait objects,
//! no clone machinery, no unknown-command stderr path. Unknown names are
//! rejected at the lookup (`Command::from_name`) and reported through the
//! typed message path by the caller.

use crate::core::bindings::InputModel;
use crate::core::app::EditorApp;
use crate::core::command::Command;
use crate::core::id::BufferId;

/// Action requiring user input
#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    /// Open a file (find-file)
    OpenFile,
    /// Save to a new path (write-file / save-as)
    SaveAs,
    /// Search forward for text
    SearchForward,
    /// Search backward for text
    SearchBackward,
    /// Query replace (old -> new)
    QueryReplace,
    /// Rename symbol
    RenameSymbol,
    /// Switch to a different buffer
    SwitchToBuffer,
    /// Read file content to cursor position
    ReadFile,
    ShellCommand,
    FilterBuffer,
    GotoLine,
    /// Go to a byte offset in the buffer
    GotoByte,
    Calculator,
    SedPreview,
    ExecuteNamedCommand,
    DescribeKey,
    /// Insert the next keypress literally, bypassing keybindings
    QuoteCharacter,
}

/// Result of command dispatch
#[derive(Debug, PartialEq)]
pub enum DispatchResult {
    /// Command executed successfully
    Success,
    /// Command not found/handled
    NotHandled,
    /// File modified on disk: the buffer named here needs the user's
    /// answer before the command can complete. Every consumer routes this
    /// through [`crate::core::focus::push_reload_question`], which owns the
    /// question state; the answer comes back through the same focus
    /// machinery in every shell.
    FileModified(BufferId),
    /// Exit requested
    Exit,
    /// Command needs user input before completing
    NeedsInput { prompt: String, action: InputAction },
    /// Informational message to display
    Info(String),
    /// Force full display redraw
    Redraw,
    /// Wait for the next keypress (e.g., for describe-key)
    AwaitKey(InputAction), // The action expecting the key (e.g., DescribeKey)
}

/// A recorded macro step: a typed command or a character insertion (N-157,
/// N-229). No string sentinels, no `"__insert:"` prefix encoding; insertion
/// replays the recorded character in full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacroStep {
    /// A typed command.
    Command(Command),
    /// A literally-inserted character.
    Insert(char),
}

/// Dispatch a typed command.
///
/// Records the step when a macro is recording, executes it through the
/// exhaustive match below, and maintains the kill-ring/yank chaining flags.
pub fn dispatch(app: &mut EditorApp, command: Command, count: usize) -> DispatchResult {
    if app.is_recording_macro
        && !matches!(command, Command::BeginMacro | Command::EndMacro)
    {
        app.current_macro.push((MacroStep::Command(command), count));
    }

    let result = execute(app, command, count);

    // Consecutive cut commands append to one kill-ring entry; any other
    // successful command resets the flag. Matched on the variants, not on
    // name text, so a rename can never silently change the set.
    let is_cut = matches!(
        command,
        Command::KillToEndOfLine | Command::KillWord | Command::KillRegion
    );
    if result == DispatchResult::Success && !is_cut {
        app.reset_kill_flag();
    }

    // Reset yank flag for non-yank commands (so yank-pop only chains after yank)
    if result == DispatchResult::Success
        && !matches!(command, Command::Yank | Command::YankPop)
    {
        app.last_command_was_yank = false;
    }

    // Every dispatched command leaves the active window valid for its
    // buffer by construction: cursor and mark are coordinates, and the
    // command just ran may have mutated the buffer under them.
    app.reconcile_active_window();

    result
}

/// Dispatch a character insertion (self-insert path).
///
/// Records the step when a macro is recording, then inserts the character
/// `count` times into the active window's buffer. Selection-replacement
/// behavior is model-parameterized: Standard mode replaces an active
/// selection (CUA); Emacs/Vim modes preserve their traditional mark
/// behavior. The runtime model is [`crate::config::keys::INPUT_MODEL`].
pub fn dispatch_insert(app: &mut EditorApp, c: char, count: usize) -> DispatchResult {
    dispatch_insert_in_model(
        app,
        c,
        count,
        crate::config::keys::INPUT_MODEL,
    )
}

/// [`dispatch_insert`] with an explicit input model.
///
/// Passing the compile-time [`crate::config::keys::INPUT_MODEL`] const
/// monomorphizes away the branch, so this is zero-cost; tests and other
/// callers may pass any [`InputModel`] to exercise model-specific paths.
pub fn dispatch_insert_in_model(
    app: &mut EditorApp,
    c: char,
    count: usize,
    model: InputModel,
) -> DispatchResult {
    if app.is_recording_macro {
        app.current_macro.push((MacroStep::Insert(c), count));
    }

    // Standard mode: typing with an active selection replaces it (CUA).
    // Emacs/Vim modes preserve their traditional mark behavior.
    if model == InputModel::Standard {
        crate::core::commands::marks::delete_active_selection(app);
    }

    let active_window = app.active_window;
    let buffer_id = match app.windows.get(&active_window) {
        Some(w) => w.buffer_id,
        None => return DispatchResult::NotHandled,
    };
    let result = match app.buffers.get_mut(&buffer_id) {
        Some(buffer) => {
            if let Some(window) = app.windows.get_mut(&active_window) {
                for _ in 0..count {
                    window.insert_char(buffer, c);
                }
                DispatchResult::Success
            } else {
                DispatchResult::NotHandled
            }
        }
        None => DispatchResult::NotHandled,
    };
    // Same seam as dispatch(): insertion mutates the buffer under the
    // cursor, so the window is reconciled before any reader runs.
    app.reconcile_active_window();
    result
}

/// The single exhaustive dispatch match (N-155): every [`Command`] maps to the
/// pure function implementing it. Adding a command variant without an arm here
/// is a compile error.
fn execute(app: &mut EditorApp, command: Command, count: usize) -> DispatchResult {
    use crate::core::commands::*;
    match command {
        // -- movement --
        Command::ForwardCharacter => movement::forward_character(app, count),
        Command::BackwardCharacter => movement::backward_character(app, count),
        Command::NextLine => movement::next_line(app, count),
        Command::PreviousLine => movement::previous_line(app, count),
        Command::BeginningOfLine => movement::beginning_of_line(app, count),
        Command::SmartHome => movement::smart_home(app, count),
        Command::EndOfLine => movement::end_of_line(app, count),
        Command::BeginningOfFile => movement::beginning_of_file(app, count),
        Command::EndOfFile => movement::end_of_file(app, count),
        Command::ForwardWord => movement::forward_word(app, count),
        Command::BackwardWord => movement::backward_word(app, count),
        Command::ForwardPage => movement::forward_page(app, count),
        Command::BackwardPage => movement::backward_page(app, count),
        Command::ForwardParagraph => movement::forward_paragraph(app, count),
        Command::BackwardParagraph => movement::backward_paragraph(app, count),
        Command::ForwardSentence => movement::forward_sentence(app, count),
        Command::BackwardSentence => movement::backward_sentence(app, count),
        Command::GotoLine => search::goto_line(app, count),
        Command::GotoByte => buffer::goto_byte(app, count),
        Command::GotoMatchingFence => text::goto_matching_fence(app, count),
        // -- editing --
        Command::OpenLine => editing::open_line(app, count),
        Command::InsertNewline => editing::insert_newline(app, count),
        Command::InsertTab => editing::insert_tab(app, count),
        Command::InsertSpace => editing::insert_space(app, count),
        Command::DeletePreviousCharacter => {
            editing::delete_previous_character(app, count)
        }
        Command::DeleteNextCharacter => editing::delete_next_character(app, count),
        Command::TransposeCharacters => editing::transpose_characters(app, count),
        Command::TransposeWords => kill_ring::transpose_words(app, count),
        Command::ToggleOverwriteMode => editing::toggle_overwrite_mode(app, count),
        Command::RedrawDisplay => editing::redraw_display(app, count),
        Command::QuoteCharacter => editing::quote_character(app, count),
        Command::NewlineAndIndent => editing::newline_and_indent(app, count),
        // -- kill ring --
        Command::KillToEndOfLine => kill_ring::kill_to_end_of_line(app, count),
        Command::KillWord => kill_ring::kill_word(app, count),
        Command::BackwardKillWord => kill_ring::backward_kill_word(app, count),
        Command::KillRegion => marks::kill_region(app, count),
        Command::CopyRegion => marks::copy_region(app, count),
        Command::Yank => kill_ring::yank(app, count),
        Command::YankPop => kill_ring::yank_pop(app, count),
        // -- marks --
        Command::SetMark => marks::set_mark(app, count),
        Command::ExchangePointAndMark => marks::exchange_point_and_mark(app, count),
        Command::MarkWord => expand_selection::mark_word(app, count),
        Command::MarkLine => expand_selection::mark_line(app, count),
        Command::MarkParagraph => expand_selection::mark_paragraph(app, count),
        Command::MarkWholeBuffer => expand_selection::mark_whole_buffer(app, count),
        Command::ExtendSelectionRight => marks::extend_selection_right(app, count),
        Command::ExtendSelectionLeft => marks::extend_selection_left(app, count),
        Command::ExtendSelectionDown => marks::extend_selection_down(app, count),
        Command::ExtendSelectionUp => marks::extend_selection_up(app, count),
        Command::ExtendSelectionHome => marks::extend_selection_home(app, count),
        Command::ExtendSelectionEnd => marks::extend_selection_end(app, count),
        Command::ExpandSelection => expand_selection::expand_selection(app, count),
        // -- case --
        Command::CaseWordUpper => text::case_word_upper(app, count),
        Command::CaseWordLower => text::case_word_lower(app, count),
        Command::CaseWordCapitalize => text::case_word_capitalize(app, count),
        Command::UppercaseRegion => text::uppercase_region(app, count),
        Command::LowercaseRegion => text::lowercase_region(app, count),
        // -- text --
        Command::JustifyParagraph => text::justify_paragraph(app, count),
        Command::WrapWord => text::wrap_word(app, count),
        // -- search --
        Command::SearchForward => search::search_forward(app, count),
        Command::SearchReverse => search::search_reverse(app, count),
        Command::QueryReplace => search::query_replace(app, count),
        // -- buffers --
        Command::NewBuffer => buffer::new_buffer(app, count),
        Command::ListBuffers => buffer::list_buffers(app, count),
        Command::BufferInfo => buffer::buffer_info(app, count),
        Command::NextBuffer => buffer::next_buffer(app, count),
        Command::PreviousBuffer => buffer::previous_buffer(app, count),
        Command::SelectBuffer => file::select_buffer(app, count),
        Command::DeleteBuffer => file::delete_buffer(app, count),
        Command::ShowPosition => buffer::show_position(app, count),
        Command::WhatCursorPosition => buffer::what_cursor_position(app, count),
        Command::CountWords => buffer::count_words(app, count),
        Command::DeleteBlankLines => text::delete_blank_lines(app, count),
        // -- files --
        Command::FindFile => file::find_file(app, count),
        Command::SaveBuffer => file::save_buffer(app, count),
        Command::WriteFile => file::write_file(app, count),
        Command::ReadFile => file::read_file(app, count),
        Command::ExitErax => control::exit_erax(app, count),
        Command::ExitWithoutSave => control::exit_without_save(app, count),
        Command::ExitAndSave => control::exit_and_save(app, count),
        Command::Print => print::print(app, count),
        Command::PrintBuffer => file::print_buffer(app, count),
        // -- windows --
        Command::SplitCurrentWindow => window::split_current_window(app, count),
        Command::SplitWindowHorizontally => window::split_window_horizontally(app, count),
        Command::DeleteOtherWindows => window::delete_other_windows(app, count),
        Command::DeleteWindow => window::delete_window(app, count),
        Command::NextWindow => window::next_window(app, count),
        Command::MinimizeWindow => window::minimize_window(app, count),
        Command::WindowPicker => window::window_picker(app, count),
        Command::GrowWindow => window::grow_window(app, count),
        Command::ShrinkWindow => window::shrink_window(app, count),
        // -- control --
        Command::KeyboardQuit => control::keyboard_quit(app, count),
        Command::UniversalArgument => control::universal_argument(app, count),
        Command::ExecuteNamedCommand => control::execute_named_command(app, count),
        Command::DescribeKey => control::describe_key(app, count),
        // ToggleMenu operates on Display state, so the event loop intercepts
        // it before dispatch (N-043). If it ever reaches dispatch (e.g. via
        // execute-named-command), it is not handled here.
        Command::ToggleMenu => DispatchResult::NotHandled,
        // -- macros --
        Command::BeginMacro => macro_cmd::begin_macro(app, count),
        Command::EndMacro => macro_cmd::end_macro(app, count),
        Command::ExecuteMacro => macro_cmd::execute_macro(app, count),
        // -- undo --
        Command::Undo => undo::undo(app, count),
        Command::Redo => undo::redo(app, count),
        // -- completion / snippets / spell --
        Command::WordCompletion => completion::word_completion(app, count),
        Command::ExpandSnippet => snippets::expand_snippet(app, count),
        Command::SpellSuggest => spell_suggest::spell_suggest(app, count),
        // -- calculator / shell --
        Command::Calculator => calculator::calculator(app, count),
        Command::ShellCommand => text::shell_command(app, count),
        Command::FilterBuffer => text::filter_buffer(app, count),
        // -- terminal --
        Command::SpawnTerminal => terminal::spawn_terminal(app, count),
        Command::SplitSpawnTerminalVertical => {
            terminal::split_spawn_terminal_vertical(app, count)
        }
        Command::SplitSpawnTerminalHorizontal => {
            terminal::split_spawn_terminal_horizontal(app, count)
        }
        Command::TerminalSendInput => terminal::terminal_send_input(app, count),
        // -- diagnostics --
        Command::ToggleDiagnostics => diagnostics::toggle_diagnostics(app, count),
        Command::DiagnosticsNext => diagnostics::diagnostics_next(app, count),
        Command::DiagnosticsPrevious => diagnostics::diagnostics_previous(app, count),
        Command::DiagnosticsJump => diagnostics::diagnostics_jump(app, count),
        // -- diff / sed --
        Command::SedPreview => diff::sed_preview(app, count),
        Command::DiffAcceptHunk => diff::diff_accept_hunk(app, count),
        Command::DiffNextHunk => diff::diff_next_hunk(app, count),
        Command::DiffPreviousHunk => diff::diff_previous_hunk(app, count),
        Command::DiffQuit => diff::diff_quit(app, count),
        // -- accessibility --
        Command::DescribeState => a11y::describe_state(app, count),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_app() -> EditorApp {
        EditorApp::new()
    }

    #[test]
    fn test_dispatch_forward_character() {
        let mut app = setup_test_app();
        let result = dispatch(&mut app, Command::ForwardCharacter, 1);
        assert_eq!(result, DispatchResult::Success);
    }

    #[test]
    fn test_dispatch_split_window() {
        let mut app = setup_test_app();
        assert_eq!(app.windows.len(), 1);
        let result = dispatch(&mut app, Command::SplitCurrentWindow, 1);
        assert_eq!(result, DispatchResult::Success);
        assert_eq!(app.windows.len(), 2);
    }

    #[test]
    fn test_dispatch_exit() {
        let mut app = setup_test_app();
        let result = dispatch(&mut app, Command::ExitErax, 1);
        assert_eq!(result, DispatchResult::Exit);
    }

    #[test]
    fn test_macro_records_typed_steps() {
        let mut app = setup_test_app();
        dispatch(&mut app, Command::BeginMacro, 1);
        dispatch(&mut app, Command::ForwardCharacter, 1);
        dispatch_insert(&mut app, 'x', 1);
        dispatch(&mut app, Command::EndMacro, 1);
        assert_eq!(
            app.last_macro,
            vec![
                (MacroStep::Command(Command::ForwardCharacter), 1),
                (MacroStep::Insert('x'), 1),
            ]
        );
    }

    #[test]
    fn test_macro_replay_inserts_full_character() {
        let mut app = setup_test_app();
        dispatch(&mut app, Command::BeginMacro, 1);
        dispatch_insert(&mut app, 'é', 1);
        dispatch(&mut app, Command::EndMacro, 1);
        dispatch(&mut app, Command::ExecuteMacro, 1);
        let buffer = app.active_buffer().expect("active buffer");
        assert!(buffer.content_string().contains('é'));
    }

    #[test]
    fn test_nested_macro_execution_is_rejected() {
        let mut app = setup_test_app();
        // Recording execute-macro inside a macro, then replaying, must not
        // recurse: the nested execution is rejected (NotHandled), which
        // halts the replay without treating it as a command failure.
        dispatch(&mut app, Command::BeginMacro, 1);
        dispatch(&mut app, Command::ExecuteMacro, 1);
        dispatch(&mut app, Command::EndMacro, 1);
        let result = dispatch(&mut app, Command::ExecuteMacro, 1);
        assert!(matches!(result, DispatchResult::NotHandled));
    }
}
