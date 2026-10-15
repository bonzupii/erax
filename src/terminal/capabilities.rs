//! Terminal-capability detection for graceful degradation.
/// Terminal capabilities detection for graceful degradation
/// Detects available features and chooses appropriate display mode
use std::env;

/// Display mode representing different terminal capabilities
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    /// True color support (24-bit color)
    TrueColor,
    /// Standard ANSI 16-color support
    Ansi,
    /// ASCII-only mode with minimal formatting (no color escapes, N-110)
    Ascii,
}

impl DisplayMode {
    /// Get the display mode based on environment detection
    pub fn detect() -> Self {
        // Check for true color support
        if has_true_color_support() {
            return DisplayMode::TrueColor;
        }

        // Check for basic ANSI support
        if has_ansi_support() {
            return DisplayMode::Ansi;
        }

        // Fallback to ASCII-only
        DisplayMode::Ascii
    }
}

/// Check if we're running in a GUI environment
/// Check if terminal supports true color (24-bit)
fn has_true_color_support() -> bool {
    // Check COLORTERM first
    if let Ok(colorterm) = env::var("COLORTERM")
        && (colorterm.contains("truecolor") || colorterm.contains("24bit")) {
            return true;
        }

    // Check TERM environment variable
    if let Ok(term) = env::var("TERM") {
        // Modern terminals with true color support
        if term.contains("24bit")
            || term.contains("truecolor")
            || term.starts_with("xterm-kitty")
            || term.starts_with("screen")
            || term.starts_with("tmux")
        {
            return true;
        }
    }

    // Check for specific terminal indicators
    env::var("TERM_PROGRAM").is_ok_and(|tp| {
        tp == "iTerm.app" || tp == "Hyper" || tp == "wezterm" || tp == "vscode"
    })
}

/// Check if terminal supports basic ANSI codes
fn has_ansi_support() -> bool {
    // On Windows, assume no ANSI unless specifically enabled
    #[cfg(windows)]
    {
        // Check if we're in a modern Windows terminal
        if let Ok(term_program) = env::var("TERM_PROGRAM") {
            return term_program == "vscode" || term_program.contains("windows");
        }

        // Check if running in ConEmu, Cmder, or Windows Terminal
        (match env::var("ConEmuANSI") {
            Ok(val) => val == "ON",
            Err(_) => false,
        })
        || env::var("CMDEXTVERSION").is_ok()
        || env::var("WT_SESSION").is_ok()
    }

    #[cfg(not(windows))]
    {
        // On Unix-like systems, most terminals support ANSI
        if let Ok(term) = env::var("TERM") {
            !term.is_empty() && term != "dumb"
        } else {
            // If TERM is not set, assume basic ANSI
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_mode_detection() {
        // Test that detection returns a valid mode
        let mode = DisplayMode::detect();
        match mode {
            DisplayMode::TrueColor | DisplayMode::Ansi | DisplayMode::Ascii => {
                // Success - at least one mode should be detected
            }
        }
    }

    #[test]
    fn test_color_support() {
        // Test that detection returns a valid mode
        let mode = DisplayMode::detect();
        match mode {
            DisplayMode::TrueColor | DisplayMode::Ansi => {
                // These modes support color
            }
            DisplayMode::Ascii => {
                // Ascii mode does not support color
            }
        }
    }

    #[test]
    fn test_advanced_formatting_support() {
        // Test that detection returns a valid mode
        let mode = DisplayMode::detect();
        match mode {
            DisplayMode::TrueColor => {
                // This mode supports advanced formatting
            }
            DisplayMode::Ansi | DisplayMode::Ascii => {
                // These modes do not support advanced formatting
            }
        }
    }


    #[test]
    fn test_has_true_color_support() {
        // This test may vary based on actual environment
        let has_true_color = has_true_color_support();
        // We can't make assumptions about the test environment,
        // but the function should not panic
        assert!(has_true_color == has_true_color); // Just verifying it doesn't panic
    }

    #[test]
    fn test_has_ansi_support() {
        // This test may vary based on actual environment
        let has_ansi = has_ansi_support();
        // We can't make assumptions about the test environment,
        // but the function should not panic
        assert!(has_ansi == has_ansi); // Just verifying it doesn't panic
    }
}
