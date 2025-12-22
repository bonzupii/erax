//! Unified Input Router
//!
//! Centralizes input handling logic that was previously duplicated between GUI and TUI.
//! Currently provides gutter width calculation used by both TUI and GUI.

/// Calculate gutter width based on line count and whether line numbers are shown.
///
/// This is a single source of truth for gutter calculation, used by:
/// - TUI event_handler.rs for mouse coordinate translation
/// - GUI gui.rs for mouse coordinate translation
pub fn gutter_width(line_count: usize, show_line_numbers: bool) -> usize {
    if show_line_numbers {
        let line_count = line_count.max(1);
        format!("{}", line_count).len() + 1 // digits + separator
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gutter_width_calculation() {
        assert_eq!(gutter_width(1, false), 0);
        assert_eq!(gutter_width(1, true), 2); // "1" + separator
        assert_eq!(gutter_width(99, true), 3); // "99" + separator
        assert_eq!(gutter_width(999, true), 4); // "999" + separator
    }
}
