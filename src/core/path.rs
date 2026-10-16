//! Canonical file-path validation for open and create flows.
//!
//! One implementation, one concrete error type: every caller (sed mode,
//! buffer loading, file commands) shares this policy instead of each
//! implementing its own stringly validator.

use std::path::{Path, PathBuf};

/// Typed path-validation failures.
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
    /// The path exists but its metadata could not be read, so the
    /// device-file filter cannot vouch for it. Refused outright: the
    /// alternative is opening an unfiltered path.
    UnreadableMetadata(String),
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathError::InvalidPath(p) => write!(f, "invalid path '{p}'"),
            PathError::InvalidParent(p) => write!(f, "invalid parent directory '{p}'"),
            PathError::MissingFileName => write!(f, "invalid file path: missing filename"),
            PathError::NoCurrentDir => write!(f, "cannot determine current directory"),
            PathError::BlockedFileType(kind) => write!(f, "cannot open {kind} files"),
            PathError::UnreadableMetadata(p) => {
                write!(f, "cannot read metadata for '{p}'")
            }
        }
    }
}

/// Validate and canonicalize a file path.
///
/// Canonicalization resolves traversal (`.`, `..`, symlinks); the
/// device-file filter then refuses directories, character/block
/// devices, FIFOs, and sockets, which could hang or crash the editor.
///
/// Paths that do not exist yet are validated through their parent
/// directory (the create/save-as flow). When the file must already
/// exist, use [`validate_existing_file`].
///
/// A metadata read failure on an existing path is a typed error, never
/// a bypassed filter: the filter's contract is that no unchecked path
/// reaches `open`.
pub fn validate_file_path(path: &Path) -> Result<PathBuf, PathError> {
    let canonical = match path.canonicalize() {
        Ok(c) => {
            let metadata = std::fs::metadata(&c)
                .map_err(|_| PathError::UnreadableMetadata(c.display().to_string()))?;
            block_special_types(&metadata)?;
            c
        }
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

    // Windows: block named pipes
    #[cfg(windows)]
    {
        let path_str = canonical.to_string_lossy();
        if path_str.starts_with(r"\\.\\pipe\\") || path_str.starts_with(r"\\?\pipe\\") {
            return Err(PathError::BlockedFileType("Windows named pipe"));
        }
    }

    Ok(canonical)
}

/// [`validate_file_path`] plus the existence requirement for open flows.
///
/// A validated-but-missing path is [`PathError::InvalidPath`]: the open
/// callers' contract is that a missing file is an invalid path, not an
/// IO error.
pub fn validate_existing_file(path: &Path) -> Result<PathBuf, PathError> {
    let canonical = validate_file_path(path)?;
    if !canonical.exists() {
        return Err(PathError::InvalidPath(format!(
            "file does not exist: {}",
            canonical.display()
        )));
    }
    Ok(canonical)
}

/// Refuse file types the editor must never open.
///
/// `metadata` is the single stat call of [`validate_file_path`]; the
/// directory check rides the same call.
#[cfg(unix)]
fn block_special_types(metadata: &std::fs::Metadata) -> Result<(), PathError> {
    use std::os::unix::fs::FileTypeExt;
    if metadata.is_dir() {
        return Err(PathError::BlockedFileType("directory"));
    }
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
    Ok(())
}

#[cfg(not(unix))]
fn block_special_types(_metadata: &std::fs::Metadata) -> Result<(), PathError> {
    Ok(())
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

    #[test]
    #[cfg(unix)]
    fn validate_blocks_directories() {
        let dir = std::env::temp_dir();
        let result = validate_file_path(&dir);
        assert_eq!(result, Err(PathError::BlockedFileType("directory")));
    }

    #[test]
    #[cfg(unix)]
    fn validate_existing_file_rejects_missing() {
        let path = std::env::temp_dir().join("erax-validate-test-missing.txt");
        let _ = std::fs::remove_file(&path);
        let result = validate_existing_file(&path);
        assert!(matches!(result, Err(PathError::InvalidPath(_))));
    }

    #[test]
    #[cfg(unix)]
    fn unreadable_metadata_is_refused_not_bypassed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir();
        let path = dir.join("erax-validate-test-nometa.txt");
        std::fs::write(&path, b"x").expect("write fixture");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
            .expect("chmod fixture");
        let result = validate_file_path(&path);
        // Root (or a permissive umask environment) can still stat the
        // file; only assert the refusal when the metadata read genuinely
        // fails, and never assert a bypass.
        if std::fs::metadata(&path).is_err() {
            assert!(matches!(result, Err(PathError::UnreadableMetadata(_))));
        } else {
            assert!(result.is_ok());
        }
        let _ = std::fs::remove_file(&path);
    }
}
