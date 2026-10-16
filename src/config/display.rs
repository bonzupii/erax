//! Display and rendering settings: typed `pub const`s.
//!
//! Edit the values and rebuild. A wrong type is a compile error (principle 1).

use crate::core::bindings::ModalMode;
use crate::core::cursor_style::CursorStyle;

/// Show line numbers in the gutter.
pub const LINE_NUMBERS: bool = true;

/// Wrap long lines at the window edge instead of scrolling horizontally.
///
/// May be overridden per-invocation by `--wrap` / `--no-wrap`; the override
/// is a typed invocation parameter and never mutates this constant.
pub const WRAP_LINES: bool = false;

/// Enable syntax highlighting.
pub const SYNTAX_HIGHLIGHTING: bool = true;

/// Auto-indent new lines.
pub const AUTO_INDENT: bool = true;

/// Height of the menu bar in terminal cells.
///
/// The single source of menu-bar height: layout, viewport, and input
/// hit-testing all read this constant.
pub const MENU_BAR_HEIGHT_CELLS: usize = 1;

/// Lines scrolled per scrollbar-arrow click.
pub const SCROLLBAR_ARROW_STEP_LINES: usize = 3;

/// Columns scrolled per horizontal scrollbar-arrow click.
pub const SCROLLBAR_ARROW_STEP_COLS: usize = 5;

// ---------------------------------------------------------------------------
// Cursor styles (mode-aware UI).
// ---------------------------------------------------------------------------
/// Cursor style for the emacs-style key-trie model (the default): steady
/// block, the historical erax cursor.
pub const KEYTRIE_CURSOR_STYLE: CursorStyle = CursorStyle::SteadyBlock;

/// Cursor style for the modeless standard model: steady bar (VS Code-like).
pub const STANDARD_CURSOR_STYLE: CursorStyle = CursorStyle::SteadyBar;

/// Cursor style for the vim modal model, per modal state:
/// Normal = steady block, Insert = bar, Visual = block (hollow ok),
/// Replace = underline. OperatorPending is a sub-state of Normal; CommandLine
/// is text entry on the bar.
///
/// `overwrite` is the active window's overwrite flag: modal Insert with
/// overwrite on is Replace behavior (`-- REPLACE --`).
pub const fn modal_cursor_style(mode: ModalMode, overwrite: bool) -> CursorStyle {
    match mode {
        ModalMode::Normal | ModalMode::OperatorPending => CursorStyle::SteadyBlock,
        ModalMode::Insert if overwrite => CursorStyle::SteadyUnderline,
        ModalMode::Insert => CursorStyle::SteadyBar,
        ModalMode::Visual => CursorStyle::HollowBlock,
        ModalMode::CommandLine => CursorStyle::SteadyBlock,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_style_decscusr_mapping() {
        // Discriminants are the DECSCUSR Ps values (hollow degrades to block).
        assert_eq!(CursorStyle::Default.decscusr_param(), 0);
        assert_eq!(CursorStyle::SteadyBlock.decscusr_param(), 2);
        assert_eq!(CursorStyle::SteadyUnderline.decscusr_param(), 4);
        assert_eq!(CursorStyle::SteadyBar.decscusr_param(), 6);
        assert_eq!(CursorStyle::HollowBlock.decscusr_param(), 2);
        assert_eq!(CursorStyle::SteadyBlock.decscusr_bytes(), *b"\x1b[2 q");
        assert_eq!(CursorStyle::SteadyBar.decscusr_bytes(), *b"\x1b[6 q");
        assert_eq!(CursorStyle::Default.decscusr_bytes(), *b"\x1b[0 q");
    }

    #[test]
    fn modal_cursor_style_mapping() {
        // The D6 const mapping: Normal = steady block, Insert = bar,
        // Visual = block (hollow ok), Replace = underline.
        assert_eq!(
            modal_cursor_style(ModalMode::Normal, false),
            CursorStyle::SteadyBlock
        );
        assert_eq!(
            modal_cursor_style(ModalMode::OperatorPending, false),
            CursorStyle::SteadyBlock
        );
        assert_eq!(
            modal_cursor_style(ModalMode::Insert, false),
            CursorStyle::SteadyBar
        );
        assert_eq!(
            modal_cursor_style(ModalMode::Insert, true),
            CursorStyle::SteadyUnderline
        );
        assert_eq!(
            modal_cursor_style(ModalMode::Visual, false),
            CursorStyle::HollowBlock
        );
    }
}
