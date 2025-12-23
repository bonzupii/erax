// erax Configuration
// Edit this file to customize your editor, then run: make && make install

use crate::config::Config;

/// User configuration function
pub fn configure(config: &mut Config) {
    // Keybindings - erax defaults
    // Movement
    config.bind("^F", "forward-character");
    config.bind("^B", "backward-character");
    config.bind("^N", "next-line");
    config.bind("^P", "previous-line");

    // Arrow keys (modern convenience)
    config.bind("Right", "forward-character");
    config.bind("Left", "backward-character");
    config.bind("Down", "next-line");
    config.bind("Up", "previous-line");
    config.bind("Home", "beginning-of-line");
    config.bind("End", "end-of-line");
    config.bind("PageUp", "backward-page");
    config.bind("PageDown", "forward-page");
    config.bind("Insert", "toggle-overwrite-mode");
    config.bind("^A", "beginning-of-line");
    config.bind("^E", "end-of-line");
    config.bind("^V", "forward-page");
    config.bind("ESC-V", "backward-page");
    config.bind("Esc V", "backward-page"); // ESC then V fallback
    // Standard bindings
    config.bind("ESC-<", "beginning-of-file"); // M-< (Alt + <)
    config.bind("ESC->", "end-of-file"); // M-> (Alt + >)
    // Also support ESC then < or > sequence
    config.bind("ESC <", "beginning-of-file");
    config.bind("ESC >", "end-of-file");
    // Word movement
    config.bind("ESC-f", "forward-word"); // M-f
    config.bind("Esc f", "forward-word"); // ESC then f
    config.bind("ESC-b", "backward-word"); // M-b
    config.bind("Esc b", "backward-word"); // ESC then b
    config.bind("ESC-v", "backward-page"); // M-v
    config.bind("Esc v", "backward-page"); // ESC then v
    config.bind("ESC-DEL", "kill-word"); // M-DEL
    config.bind("ESC-Backspace", "backward-kill-word"); // M-backspace
    config.bind("ESC-t", "transpose-words"); // M-t
    config.bind("Esc t", "transpose-words"); // ESC then t
    config.bind("ESC-x", "execute-named-command"); // M-x
    config.bind("Esc x", "execute-named-command"); // ESC then x

    // File operations
    config.bind("^X^F", "find-file");
    config.bind("^X^S", "save-buffer");
    config.bind("^X^W", "write-file");
    config.bind("^X^C", "exit-without-save");
    config.bind("^X^R", "read-file");
    config.bind("Esc P", "print");

    // Editing
    config.bind("^D", "delete-char");
    config.bind("Delete", "delete-next-character");
    config.bind("Backspace", "delete-previous-character");
    config.bind("Enter", "insert-newline");
    config.bind("Tab", "insert-tab");
    config.bind("^C Tab", "expand-snippet");
    config.bind("^K", "kill-to-end-of-line");
    config.bind("^Y", "yank");
    config.bind("ESC-y", "yank-pop");
    config.bind("Esc y", "yank-pop");
    config.bind("^W", "kill-region");
    config.bind("ESC-w", "copy-region");
    config.bind("Esc w", "copy-region"); // ESC then w
    config.bind("^O", "open-line");
    config.bind("^T", "transpose-characters");

    config.bind("ESC-/", "word-completion");
    config.bind("Esc /", "word-completion");
    config.bind("^X Tab", "word-completion"); // Ctrl-X Tab for autocomplete

    // Search
    config.bind("^S", "search-forward");
    config.bind("^R", "search-reverse");
    config.bind("ESC-%", "query-replace");
    config.bind("Esc %", "query-replace"); // ESC then %

    // Windows
    config.bind("^X 2", "split-current-window"); // C-x 2
    config.bind("^X 3", "split-window-horizontally"); // C-x 3
    config.bind("^X 0", "minimize-window"); // C-x 0
    config.bind("^X 9", "window-picker"); // C-x 9
    config.bind("^X 1", "delete-other-windows");
    config.bind("^X o", "next-window"); // C-x o
    config.bind("^X O", "next-window"); // C-x O
    config.bind("^X Z", "grow-window"); // C-x Z
    config.bind("^X ^Z", "shrink-window"); // C-x C-z

    // Additional CTRL bindings
    config.bind("^Z", "backward-page"); // ^Z backward-page
    config.bind("^C", "insert-space"); // ^C insert space
    config.bind("^J", "newline-and-indent"); // ^J indent
    config.bind("^L", "redraw-display"); // ^L redraw screen
    config.bind("^Q", "quote-character"); // ^Q quote (insert literal)
    config.bind("^U", "universal-argument"); // ^U universal arg prefix
    config.bind("^G", "keyboard-quit"); // ^G abort command (universal cancel)

    // Buffers
    config.bind("^X B", "select-buffer");
    config.bind("^X K", "delete-buffer");
    config.bind("^X d", "toggle-diagnostics");
    config.bind("^X =", "show-position");
    config.bind("^X ?", "describe-key");

    // Custom examples - use lowercase for ESC sequences (input is normalized)
    config.bind("ESC-j", "justify-paragraph");
    config.bind("Esc j", "justify-paragraph"); // ESC then j for terminals without Alt
    config.bind("ESC-z", "exit-and-save");
    config.bind("Esc z", "exit-and-save");

    // Extended bindings
    config.bind("ESC-q", "justify-paragraph"); // M-q is standard fill-paragraph
    config.bind("Esc q", "justify-paragraph"); // ESC then q fallback
    config.bind("ESC-^F", "goto-matching-fence"); // M-C-f (Alt-Ctrl-f)

    // Case conversion
    config.bind("ESC-u", "case-word-upper");
    config.bind("Esc u", "case-word-upper");
    config.bind("ESC-l", "case-word-lower");
    config.bind("Esc l", "case-word-lower");
    config.bind("ESC-c", "case-word-capitalize");
    config.bind("Esc c", "case-word-capitalize");
    // Region case conversion bindings are here for future implementation
    config.bind("^X^U", "uppercase-region");
    config.bind("^X^L", "lowercase-region");

    // Shell integration
    config.bind("^X 4", "shell-command");
    config.bind("^X p", "calculator"); // Programmer's calculator (hex/bin/bitwise)

    // Terminal
    config.bind("^X t 2", "split-spawn-terminal-vertical");
    config.bind("^X t 3", "split-spawn-terminal-horizontal");

    // Delete blank lines
    config.bind("^X^O", "delete-blank-lines"); // C-x C-o

    // Count words
    config.bind("ESC-=", "count-words"); // M-=
    config.bind("Esc =", "count-words");

    // Mark & Region
    config.bind("^@", "set-mark"); // C-@ (Ctrl-Space on most terminals)
    config.bind("ESC-.", "set-mark");
    config.bind("Esc .", "set-mark");
    config.bind("^X^X", "exchange-point-and-mark");
    config.bind("^=", "expand-selection"); // C-= expand to word/line/paragraph
    config.bind("ESC-@", "mark-word"); // M-@ mark word
    config.bind("Esc @", "mark-word");
    config.bind("ESC-h", "mark-paragraph"); // M-h mark paragraph
    config.bind("Esc h", "mark-paragraph");

    // Line Operations
    // config.bind("^O", "open-line"); // Existing ^O binding is already present for 'open-line'

    // Goto Line
    config.bind("ESC-g", "goto-line"); // Modern/common binding
    config.bind("Esc g", "goto-line");

    // Paragraph Movement
    config.bind("ESC-}", "forward-paragraph");
    config.bind("Esc }", "forward-paragraph");
    config.bind("ESC-{", "backward-paragraph");
    config.bind("Esc {", "backward-paragraph");

    // Macros
    config.bind("^X (", "begin-macro");
    config.bind("^X )", "end-macro");
    config.bind("^X e", "execute-macro");

    // =========================================================================
    // EDITOR SETTINGS
    // =========================================================================
    // Tab behavior
    config.set("tab_width", 8); // Number of spaces per tab
    config.set("use_tabs", true); // Use tabs (true) or spaces (false) for indentation

    // Display settings
    config.set("line_numbers", true); // Show line numbers in gutter

    // ═══════════════════════════════════════════════════════════════════════════
    // LINE WRAPPING
    // ═══════════════════════════════════════════════════════════════════════════
    // Controls whether long lines wrap at the window edge or extend horizontally
    // with scrolling.
    //
    // When enabled (true), lines wrap at the window boundary and continue on
    // the next visual line. When disabled (false), long lines extend beyond
    // the window edge and can be viewed by scrolling horizontally.
    //
    // CLI override: --wrap, --no-wrap
    // Values: true | false
    // Default: false (no line wrapping)
    config.set("wrap_lines", false);

    config.set("syntax_highlighting", true); // Enable syntax highlighting
    config.set("auto_indent", true); // Auto-indent new lines

    // =========================================================================
    // FONT SETTINGS (GUI mode only)
    // =========================================================================
    // Path to a TrueType font file, or empty to use system default
    // Examples: "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
    //           "" (empty = auto-detect via fc-match)
    config.set("font_path", "JetBrains Mono");
    config.set("font_size", 16); // Font size in pixels

    // =========================================================================
    // COLOR SETTINGS
    // =========================================================================
    // Enable TrueColor (24-bit RGB) if your terminal supports it
    // Options: "auto" (detect), "true" (force enable), "false" (use 256-color)
    config.set("truecolor", "auto");

    // Color theme
    // Standard themes: "dracula", "monokai", "solarized_dark", "gruvbox", "nord",
    //   "tokyo_night", "catppuccin_mocha", "one_dark", "rose_pine"
    // Modern themes: "synthwave_84", "everforest", "github_dark", "ayu_mirage", "horizon"
    // Victorian: "victorian_gothic", "victorian_warm", "victorian_neutral",
    //   "victorian_royal", "victorian_gaslight"
    // Accessibility: "high_contrast_dark", "high_contrast_light", "high_contrast_solarized",
    //   "monochrome_high_contrast", "deuteranopia_azure", "protanopia_lumos",
    //   "tritanopia_blossom", "achroma_noir"
    config.set("theme", "dracula");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_user_configuration_defaults() {
        let mut config = Config::default();
        configure(&mut config);

        // Verify core keybindings
        // Movement
        assert_eq!(
            config.keybindings.get("^F"),
            Some(&"forward-character".to_string())
        );
        assert_eq!(
            config.keybindings.get("^B"),
            Some(&"backward-character".to_string())
        );
        assert_eq!(config.keybindings.get("^N"), Some(&"next-line".to_string()));
        assert_eq!(
            config.keybindings.get("^P"),
            Some(&"previous-line".to_string())
        );
        assert_eq!(
            config.keybindings.get("^A"),
            Some(&"beginning-of-line".to_string())
        );
        assert_eq!(
            config.keybindings.get("^E"),
            Some(&"end-of-line".to_string())
        );
        assert_eq!(
            config.keybindings.get("^V"),
            Some(&"forward-page".to_string())
        );
        assert_eq!(
            config.keybindings.get("^Z"),
            Some(&"backward-page".to_string())
        );
        assert_eq!(
            config.keybindings.get("PageUp"),
            Some(&"backward-page".to_string())
        );
        assert_eq!(
            config.keybindings.get("PageDown"),
            Some(&"forward-page".to_string())
        );
        assert_eq!(
            config.keybindings.get("Home"),
            Some(&"beginning-of-line".to_string())
        );
        assert_eq!(
            config.keybindings.get("End"),
            Some(&"end-of-line".to_string())
        );
        assert_eq!(
            config.keybindings.get("Insert"),
            Some(&"toggle-overwrite-mode".to_string())
        );
        assert_eq!(
            config.keybindings.get("ESC-V"),
            Some(&"backward-page".to_string())
        );
        assert_eq!(
            config.keybindings.get("ESC-<"),
            Some(&"beginning-of-file".to_string())
        );
        assert_eq!(
            config.keybindings.get("ESC->"),
            Some(&"end-of-file".to_string())
        );

        // File operations
        assert_eq!(
            config.keybindings.get("^X^F"),
            Some(&"find-file".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X^S"),
            Some(&"save-buffer".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X^W"),
            Some(&"write-file".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X^C"),
            Some(&"exit-without-save".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X^R"),
            Some(&"read-file".to_string())
        );

        // Editing
        assert_eq!(
            config.keybindings.get("^D"),
            Some(&"delete-char".to_string())
        );
        // Canonical name for kill-line might be kill-to-end-of-line depending on rename
        assert_eq!(
            config.keybindings.get("^K"),
            Some(&"kill-to-end-of-line".to_string())
        );
        assert_eq!(config.keybindings.get("^Y"), Some(&"yank".to_string()));
        assert_eq!(
            config.keybindings.get("ESC-y"),
            Some(&"yank-pop".to_string())
        );
        assert_eq!(
            config.keybindings.get("^W"),
            Some(&"kill-region".to_string())
        );
        assert_eq!(
            config.keybindings.get("ESC-w"),
            Some(&"copy-region".to_string())
        );
        assert_eq!(config.keybindings.get("^O"), Some(&"open-line".to_string()));
        assert_eq!(
            config.keybindings.get("^T"),
            Some(&"transpose-characters".to_string())
        );

        // Search
        assert_eq!(
            config.keybindings.get("^S"),
            Some(&"search-forward".to_string())
        );
        assert_eq!(
            config.keybindings.get("^R"),
            Some(&"search-reverse".to_string())
        );
        assert_eq!(
            config.keybindings.get("ESC-%"),
            Some(&"query-replace".to_string())
        );

        // Windows
        assert_eq!(
            config.keybindings.get("^X 2"),
            Some(&"split-current-window".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X 3"),
            Some(&"split-window-horizontally".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X 0"),
            Some(&"minimize-window".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X 9"),
            Some(&"window-picker".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X 1"),
            Some(&"delete-other-windows".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X o"),
            Some(&"next-window".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X O"),
            Some(&"next-window".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X Z"),
            Some(&"grow-window".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X ^Z"),
            Some(&"shrink-window".to_string())
        );

        // Buffers
        assert_eq!(
            config.keybindings.get("^X B"),
            Some(&"select-buffer".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X K"),
            Some(&"delete-buffer".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X ="),
            Some(&"show-position".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X ?"),
            Some(&"describe-key".to_string())
        );

        // Custom examples
        assert_eq!(
            config.keybindings.get("ESC-j"),
            Some(&"justify-paragraph".to_string())
        );
        assert_eq!(
            config.keybindings.get("ESC-z"),
            Some(&"exit-and-save".to_string())
        );

        assert_eq!(
            config.keybindings.get("^X p"),
            Some(&"calculator".to_string())
        );

        // Terminal
        assert_eq!(
            config.keybindings.get("^X t 2"),
            Some(&"split-spawn-terminal-vertical".to_string())
        );
        assert_eq!(
            config.keybindings.get("^X t 3"),
            Some(&"split-spawn-terminal-horizontal".to_string())
        );

        // Verify settings
        assert_eq!(config.get_int("tab_width"), Some(8));
        assert_eq!(config.get_bool("use_tabs"), Some(true));
        assert_eq!(config.get_bool("line_numbers"), Some(true));
    }
}
