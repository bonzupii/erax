//! Application execution modes for erax.
//!
//! The three modes — sed (stream editor), terminal (TUI), and GUI — are all
//! compiled unconditionally (N-005: no feature gates, no stub fallbacks).
//! Each runner takes the typed [`crate::cli::Invocation`] and returns a
//! concrete typed error.

mod gui;
mod sed;
mod tui;

use std::path::{Path, PathBuf};

pub use gui::run_gui_mode;
pub use sed::run_sed_mode;
pub use tui::run_terminal_mode;

/// One path-validation function with one concrete error type, used by all
/// callers (N-017): sed mode, buffer loading, and file commands share this
/// policy instead of each implementing their own stringly validator.
#[derive(Debug, PartialEq, Eq)]
pub enum PathError {
    /// The path could not be canonicalized.
    InvalidPath(String),
    /// The parent directory does not exist.
    InvalidParent(String),
    /// The path has no file name component.
    MissingFileName,
    /// The current directory could not be determined.
    NoCurrentDir,
    /// Special file types that could hang or crash the editor.
    BlockedFileType(&'static str),
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathError::InvalidPath(p) => write!(f, "invalid path '{p}'"),
            PathError::InvalidParent(p) => write!(f, "invalid parent directory '{p}'"),
            PathError::MissingFileName => write!(f, "invalid file path: missing filename"),
            PathError::NoCurrentDir => write!(f, "cannot determine current directory"),
            PathError::BlockedFileType(kind) => write!(f, "cannot open {kind} files"),
        }
    }
}

/// Validate and canonicalize file paths to prevent directory traversal and
/// block device files.
///
/// Returns the canonical path or a typed [`PathError`]; no boxed strings.
pub fn validate_file_path(path: &Path) -> Result<PathBuf, PathError> {
    let canonical = match path.canonicalize() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // File doesn't exist yet - validate the parent directory
            if let Some(parent) = path.parent() {
                let canonical_parent = parent
                    .canonicalize()
                    .map_err(|_| PathError::InvalidParent(parent.display().to_string()))?;
                if let Some(filename) = path.file_name() {
                    canonical_parent.join(filename)
                } else {
                    return Err(PathError::MissingFileName);
                }
            } else {
                let current_dir =
                    std::env::current_dir().map_err(|_| PathError::NoCurrentDir)?;
                current_dir.join(path)
            }
        }
        Err(_) => {
            return Err(PathError::InvalidPath(path.display().to_string()));
        }
    };

    // Block special file types that could hang or crash the editor
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if let Ok(metadata) = std::fs::metadata(&canonical) {
            let ft = metadata.file_type();
            if ft.is_char_device() {
                return Err(PathError::BlockedFileType("character device"));
            }
            if ft.is_block_device() {
                return Err(PathError::BlockedFileType("block device"));
            }
            if ft.is_fifo() {
                return Err(PathError::BlockedFileType("FIFO/named pipe"));
            }
            if ft.is_socket() {
                return Err(PathError::BlockedFileType("socket"));
            }
        }
    }

    // Windows: block named pipes
    #[cfg(windows)]
    {
        let path_str = canonical.to_string_lossy();
        if path_str.starts_with(r"\\.\pipe\") || path_str.starts_with(r"\\?\pipe\") {
            return Err(PathError::BlockedFileType("Windows named pipe"));
        }
    }

    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_nonexistent_parent() {
        let result = validate_file_path(Path::new("/no/such/dir/erax-test.txt"));
        assert!(matches!(result, Err(PathError::InvalidParent(_))));
    }

    #[test]
    fn validate_accepts_new_file_in_existing_dir() {
        let dir = std::env::temp_dir();
        let path = dir.join("erax-validate-test-new-file.txt");
        let result = validate_file_path(&path);
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn validate_blocks_character_devices() {
        let result = validate_file_path(Path::new("/dev/null"));
        assert_eq!(
            result,
            Err(PathError::BlockedFileType("character device"))
        );
    }
}
