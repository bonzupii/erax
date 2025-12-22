//! Application execution modes for erax.
//!
//! This module contains the implementation of different editor modes:
//! - Sed mode for stream editing
//! - Terminal mode for TUI
//! - GUI mode for graphical interface

mod sed;
mod tui;

#[cfg(feature = "gui")]
mod gui;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

// Re-export mode runners
pub use sed::run_sed_mode;
pub use tui::run_terminal_mode;

#[cfg(feature = "gui")]
pub use gui::run_gui_mode;

#[cfg(not(feature = "gui"))]
pub fn run_gui_mode(
    _files: &[PathBuf],
    _config: &crate::config::Config,
) -> Result<(), Box<dyn std::error::Error>> {
    Err("GUI mode requires the 'gui' feature to be enabled. \
         Rebuild with: cargo build --features gui"
        .into())
}

/// Editor execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    Sed,
    AsciiTerminal,
    AnsiTerminal,
    Utf8Terminal,
    Gui,
}

/// Terminal capability level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalCaps {
    Ascii,
    Ansi,
    Utf8,
}

/// Validate and canonicalize file paths to prevent directory traversal and block device files.
pub fn validate_file_path(path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let canonical = match path.canonicalize() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // File doesn't exist yet - validate the parent directory
            if let Some(parent) = path.parent() {
                let canonical_parent = parent
                    .canonicalize()
                    .map_err(|_| "Invalid parent directory")?;
                if let Some(filename) = path.file_name() {
                    canonical_parent.join(filename)
                } else {
                    return Err("Invalid file path: missing filename".into());
                }
            } else {
                let current_dir =
                    std::env::current_dir().map_err(|_| "Cannot determine current directory")?;
                current_dir.join(path)
            }
        }
        Err(e) => return Err(format!("Invalid path: {}", e).into()),
    };

    // Block special file types that could hang or crash the editor
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if let Ok(metadata) = std::fs::metadata(&canonical) {
            let ft = metadata.file_type();
            if ft.is_char_device() {
                return Err("Cannot open character device files (e.g., /dev/zero)".into());
            }
            if ft.is_block_device() {
                return Err("Cannot open block device files".into());
            }
            if ft.is_fifo() {
                return Err("Cannot open FIFO/named pipe files".into());
            }
            if ft.is_socket() {
                return Err("Cannot open socket files".into());
            }
        }
    }

    // Windows: block named pipes
    #[cfg(windows)]
    {
        let path_str = canonical.to_string_lossy();
        if path_str.starts_with(r"\\.\pipe\") || path_str.starts_with(r"\\?\pipe\") {
            return Err("Cannot open Windows named pipes".into());
        }
    }

    Ok(canonical)
}

/// Detect the appropriate editor mode based on environment.
pub fn detect_mode() -> Result<EditorMode, Box<dyn std::error::Error>> {
    let is_stdin_tty = std::io::stdin().is_terminal();
    detect_mode_internal(is_stdin_tty, |k| std::env::var(k))
}

/// Internal mode detection with injectable environment lookup.
pub fn detect_mode_internal<F>(
    is_stdin_tty: bool,
    get_env: F,
) -> Result<EditorMode, Box<dyn std::error::Error>>
where
    F: Fn(&str) -> Result<String, std::env::VarError>,
{
    if !is_stdin_tty {
        return Ok(EditorMode::Sed);
    }

    if get_env("DISPLAY").is_ok() || get_env("WAYLAND_DISPLAY").is_ok() {
        if cfg!(feature = "gui") {
            return Ok(EditorMode::Gui);
        }
    }

    let caps = detect_terminal_caps_with_env(get_env)?;
    Ok(match caps {
        TerminalCaps::Ascii => EditorMode::AsciiTerminal,
        TerminalCaps::Ansi => EditorMode::AnsiTerminal,
        TerminalCaps::Utf8 => EditorMode::Utf8Terminal,
    })
}

/// Detect terminal capabilities from environment variables.
pub fn detect_terminal_caps_with_env<F>(
    get_env: F,
) -> Result<TerminalCaps, Box<dyn std::error::Error>>
where
    F: Fn(&str) -> Result<String, std::env::VarError>,
{
    if let Ok(lang) = get_env("LANG").or_else(|_| get_env("LC_ALL")) {
        if lang.to_lowercase().contains("utf-8") || lang.to_lowercase().contains("utf8") {
            return Ok(TerminalCaps::Utf8);
        }
    }

    if let Ok(term) = get_env("TERM") {
        if term.contains("256color") || term.contains("ansi") || term.contains("xterm") {
            return Ok(TerminalCaps::Ansi);
        }
    }

    Ok(TerminalCaps::Ascii)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::VarError;

    fn mock_env<'a>(vars: &'a [(&str, &str)]) -> impl Fn(&str) -> Result<String, VarError> + 'a {
        move |key| {
            for (k, v) in vars {
                if *k == key {
                    return Ok((*v).to_string());
                }
            }
            Err(VarError::NotPresent)
        }
    }

    #[test]
    fn test_detect_terminal_caps_ascii_default() {
        let env = mock_env(&[]);
        let result = detect_terminal_caps_with_env(env).unwrap();
        assert_eq!(result, TerminalCaps::Ascii);
    }

    #[test]
    fn test_detect_terminal_caps_utf8() {
        let env = mock_env(&[("LANG", "en_US.UTF-8")]);
        let result = detect_terminal_caps_with_env(env).unwrap();
        assert_eq!(result, TerminalCaps::Utf8);
    }

    #[test]
    fn test_detect_mode_sed() {
        let env = mock_env(&[]);
        let result = detect_mode_internal(false, env).unwrap();
        assert_eq!(result, EditorMode::Sed);
    }

    #[test]
    fn test_detect_mode_terminal_fallback() {
        let env = mock_env(&[]);
        let result = detect_mode_internal(true, env).unwrap();
        assert_eq!(result, EditorMode::AsciiTerminal);
    }
}
