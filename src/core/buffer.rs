//! Buffer: pure data structure holding text content and metadata.
//! No cursor, scrolling, or viewport state (those belong to Window)
//!
//! Storage engine: [`BText`](crate::core::btext::BText), the in-house
//! persistent B-tree over byte chunks (replaced ropey in session 5).
//! The Buffer API is byte-positioned end to end — no char/byte translation
//! layer — because the tree itself is encoding-dumb (bytes are bytes).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tempfile::NamedTempFile;

use crate::core::btext::{BText, LineRef, PagerError};
use crate::core::diagnostics::Diagnostic;
use crate::core::lexer::LexerState;
use crate::core::syntax::SyntaxLexerState;
use crate::core::undo_group::{UndoGroup, UndoGrouper};

/// Maximum undo stack depth to prevent OOM from unbounded undo history
/// Increased to 10000 to handle stress testing and rapid editing sessions
const MAX_UNDO_DEPTH: usize = 10_000;

/// Represents the type of buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferKind {
    Normal,
    Diagnostics,
    DiffOriginal,
    DiffModified,
    Terminal,
    ShellOutput,
}

/// Typed error for buffer file loading (N-143: distinguish IO from UTF-8 errors).
#[derive(Debug)]
pub enum LoadError {
    /// IO error (file not found, permission denied, etc.)
    Io(std::io::Error),
    /// File contains invalid UTF-8. The raw bytes are preserved in the error
    /// so the caller can prompt the user (read-only vs proceed) without
    /// re-reading the file.
    InvalidUtf8 {
        path: PathBuf,
        bytes: Vec<u8>,
        utf8_error: std::str::Utf8Error,
    },
    /// Path validation failed.
    InvalidPath(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "IO error: {}", e),
            LoadError::InvalidUtf8 { path, utf8_error, .. } => {
                write!(f, "File '{}' is not valid UTF-8: {}", path.display(), utf8_error)
            }
            LoadError::InvalidPath(msg) => write!(f, "Invalid path: {}", msg),
        }
    }
}

impl std::error::Error for LoadError {}

/// Represents an edit operation for undo/redo.
///
/// `pos` is a BYTE position; `text` is the exact bytes inserted or removed.
/// Byte positions are compared against byte lengths below: comparing them
/// against a character count would mis-handle multi-byte text.
#[derive(Debug, Clone)]
pub enum Edit {
    /// Insert: (position, text inserted)
    Insert { pos: usize, text: Vec<u8> },
    /// Delete: (position, text deleted)
    Delete { pos: usize, text: Vec<u8> },
}

/// Buffer: Pure data structure holding text and metadata
/// No view state (cursor, scrolling) - that belongs to Window
#[derive(Debug)]
pub struct Buffer {
    /// Text content stored in the in-house B-tree (O(log n) operations,
    /// byte-positioned; see `crate::core::btext`).
    text: BText,
    /// Filename (if loaded from file)
    pub filename: Option<PathBuf>,
    /// The type of buffer
    pub buffer_kind: BufferKind,
    /// Dirty flag (true if buffer has unsaved changes)
    pub modified: bool,
    /// Last modification time of the file on disk
    pub last_modified_time: Option<SystemTime>,
    /// On-disk mtime the user has already answered the reload question for.
    /// Declining the reload ("n") records the then-current mtime here, so a
    /// later save does not re-ask for the same on-disk state; any newer
    /// mtime re-arms the question. Cleared by reload and by save.
    pub acknowledged_mtime: Option<SystemTime>,
    /// Version counter for tracking buffer changes
    pub version: u64,
    /// Undo stack (VecDeque for O(1) pop_front when capping depth)
    pub undo_stack: VecDeque<UndoGroup>,
    /// Redo stack
    pub redo_stack: VecDeque<UndoGroup>,
    /// Undo grouper for smart grouping
    pub undo_grouper: UndoGrouper,
    /// Diagnostics (errors, warnings) associated with this buffer
    pub diagnostics: Vec<Diagnostic>,
    /// Syntax highlighting state cache for each line
    pub syntax_state_cache: Vec<SyntaxLexerState>,
    /// Original raw bytes for non-UTF-8 files (N-143, byte-exact preservation).
    /// When Some, the B-tree holds the lossy UTF-8 view for display/editing,
    /// but saving an unmodified buffer writes these bytes back byte-exact.
    /// (The tree can hold arbitrary bytes natively, but the bridge stays —
    /// display and edit positions are defined over the lossy view, and
    /// raw-byte editing is the hex viewer's job. Changing that
    /// would be an encoding-UX redesign, not a storage-engine swap.)
    pub original_bytes: Option<Vec<u8>>,
    /// Last pager failure (only possible for mapped files, see
    /// [`Buffer::open_mapped`]). Truncation and I/O errors are values, never
    /// panics: operations that hit the pager record the error here and return
    /// a safe default; the editor surfaces it via [`Buffer::has_pager_error`]
    /// / [`Buffer::take_pager_error`].
    /// Interior mutability so read-path accessors can stay `&self`.
    pager_error: RefCell<Option<PagerError>>,
}

impl Buffer {
    /// Create a new empty buffer
    pub fn new() -> Self {
        Self {
            text: BText::new(),
            filename: None,
            buffer_kind: BufferKind::Normal,
            modified: false,
            last_modified_time: None,
            acknowledged_mtime: None,
            version: 0,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            undo_grouper: UndoGrouper::new(),
            diagnostics: Vec::new(),
            syntax_state_cache: Vec::new(),
            original_bytes: None,
            pager_error: RefCell::new(None),
        }
    }

    /// Create a buffer from a string
    pub fn from_string(content: impl AsRef<str>) -> Self {
        Self {
            text: BText::from_bytes(content.as_ref().as_bytes()),
            filename: None,
            buffer_kind: BufferKind::Normal,
            modified: false,
            last_modified_time: None,
            acknowledged_mtime: None,
            version: 0,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            undo_grouper: UndoGrouper::new(),
            diagnostics: Vec::new(),
            syntax_state_cache: Vec::new(),
            original_bytes: None,
            pager_error: RefCell::new(None),
        }
    }

    /// Load a buffer from a file.
    ///
    /// Returns a typed [`LoadError`] distinguishing IO failures from invalid
    /// UTF-8 (N-143). For invalid UTF-8, the raw bytes are preserved in the
    /// error so the caller can prompt the user (read-only vs proceed) without
    /// re-reading the file. Use [`Buffer::from_invalid_utf8_bytes`] after the
    /// user chooses to proceed.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let path = path.as_ref();
        let validated_path =
            Self::validate_file_path(path).map_err(|e| LoadError::InvalidPath(e.to_string()))?;

        let metadata = fs::metadata(&validated_path).map_err(LoadError::Io)?;
        let file_size = metadata.len();
        let modified_time = metadata.modified().ok();

        const HUGE_FILE_THRESHOLD: u64 = 500 * 1024 * 1024; // 500MB

        if file_size > HUGE_FILE_THRESHOLD {
            eprintln!(
                "Warning: File is very large ({:.1}MB). Loading may be slow.",
                file_size as f64 / (1024.0 * 1024.0)
            );
        }

        // Read the full file once (needed to distinguish UTF-8 errors from IO
        // errors and to preserve bytes for the InvalidUtf8 case).
        let bytes = fs::read(&validated_path).map_err(LoadError::Io)?;

        // Validate UTF-8 explicitly (N-143: no silent lossy fallback).
        match std::str::from_utf8(&bytes) {
            Ok(text) => {
                let text = BText::from_bytes(text.as_bytes());
                Ok(Self {
                    text,
                    filename: Some(validated_path),
                    buffer_kind: BufferKind::Normal,
                    modified: false,
                    last_modified_time: modified_time,
                    acknowledged_mtime: None,
                    version: 0,
                    undo_stack: VecDeque::new(),
                    redo_stack: VecDeque::new(),
                    undo_grouper: UndoGrouper::new(),
                    diagnostics: Vec::new(),
                    syntax_state_cache: Vec::new(),
                    original_bytes: None,
                    pager_error: RefCell::new(None),
                })
            }
            Err(utf8_error) => Err(LoadError::InvalidUtf8 {
                path: validated_path,
                bytes,
                utf8_error,
            }),
        }
    }

    /// Open a file WITHOUT reading it: the tree is built from `fstat`
    /// metadata only (O(1)), and file bytes are materialized on demand.
    /// This is how erax opens files larger than available RAM.
    ///
    /// Backend (session-7 adoption decision): on Unix this uses the mmap
    /// backend (29–61x faster random reads than the pager, fully hardened —
    /// see `crate::core::mmap`); on non-Unix it uses the pager's bounded
    /// LRU cache. A same-length content replacement surfaces as a recorded
    /// `PagerError::FileChanged` under mmap (the pager serves its cache
    /// silently instead).
    ///
    /// Unlike [`Buffer::from_file`], no whole-file UTF-8 validation happens
    /// at open — content is validated lazily as it is read. I/O failures and
    /// truncation under the open buffer surface as [`PagerError`] values,
    /// recorded on the buffer and visible via [`Buffer::has_pager_error`] /
    /// [`Buffer::take_pager_error`]; they never panic and never raise signals.
    pub fn open_mapped(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let path = path.as_ref();
        let validated_path =
            Self::validate_file_path(path).map_err(|e| LoadError::InvalidPath(e.to_string()))?;
        let modified_time = fs::metadata(&validated_path)
            .ok()
            .and_then(|m| m.modified().ok());

        let text = BText::open_mapped(&validated_path)
            .map_err(|e| LoadError::Io(std::io::Error::other(e.to_string())))?;
        Ok(Self {
            text,
            filename: Some(validated_path),
            buffer_kind: BufferKind::Normal,
            modified: false,
            last_modified_time: modified_time,
            acknowledged_mtime: None,
            version: 0,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            undo_grouper: UndoGrouper::new(),
            diagnostics: Vec::new(),
            syntax_state_cache: Vec::new(),
            original_bytes: None,
            pager_error: RefCell::new(None),
        })
    }

    /// Open a file lazily with the syscall-pager backend (explicit fallback;
    /// the default on non-Unix). Content replacement at the same length is
    /// served silently from the pager's cache (pinned snapshot) rather than
    /// refused — the documented semantic difference from mmap.
    pub fn open_mapped_pager(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let path = path.as_ref();
        let validated_path =
            Self::validate_file_path(path).map_err(|e| LoadError::InvalidPath(e.to_string()))?;
        let modified_time = fs::metadata(&validated_path)
            .ok()
            .and_then(|m| m.modified().ok());

        let text = BText::open_mapped_pager(&validated_path)
            .map_err(|e| LoadError::Io(std::io::Error::other(e.to_string())))?;
        Ok(Self {
            text,
            filename: Some(validated_path),
            buffer_kind: BufferKind::Normal,
            modified: false,
            last_modified_time: modified_time,
            acknowledged_mtime: None,
            version: 0,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            undo_grouper: UndoGrouper::new(),
            diagnostics: Vec::new(),
            syntax_state_cache: Vec::new(),
            original_bytes: None,
            pager_error: RefCell::new(None),
        })
    }

    /// Open a file lazily with the mmap backend (Unix only). This is what
    /// [`Buffer::open_mapped`] uses on Unix (session-7 adoption decision).
    ///
    /// Same contract as [`Buffer::open_mapped`]. See `crate::core::mmap`
    /// for the mitigation stack and the documented semantic difference
    /// from the pager on content replacement.
    #[cfg(unix)]
    pub fn open_mapped_mmap(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let path = path.as_ref();
        let validated_path =
            Self::validate_file_path(path).map_err(|e| LoadError::InvalidPath(e.to_string()))?;
        let modified_time = fs::metadata(&validated_path)
            .ok()
            .and_then(|m| m.modified().ok());

        let text = BText::open_mapped_mmap(&validated_path)
            .map_err(|e| LoadError::Io(std::io::Error::other(e.to_string())))?;
        Ok(Self {
            text,
            filename: Some(validated_path),
            buffer_kind: BufferKind::Normal,
            modified: false,
            last_modified_time: modified_time,
            acknowledged_mtime: None,
            version: 0,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            undo_grouper: UndoGrouper::new(),
            diagnostics: Vec::new(),
            syntax_state_cache: Vec::new(),
            original_bytes: None,
            pager_error: RefCell::new(None),
        })
    }

    /// Create a buffer from bytes known to be invalid UTF-8, after the user
    /// has chosen to proceed (N-143).
    ///
    /// The tree holds the lossy UTF-8 view for display/editing; the original
    /// bytes are preserved in `original_bytes` so an unmodified buffer saves
    /// byte-exact.
    pub fn from_invalid_utf8_bytes(path: PathBuf, bytes: Vec<u8>) -> Self {
        let content = String::from_utf8_lossy(&bytes);
        let text = BText::from_bytes(content.as_bytes());
        Self {
            text,
            filename: Some(path),
            buffer_kind: BufferKind::Normal,
            modified: false,
            last_modified_time: None,
            acknowledged_mtime: None,
            version: 0,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            undo_grouper: UndoGrouper::new(),
            diagnostics: Vec::new(),
            syntax_state_cache: Vec::new(),
            original_bytes: Some(bytes),
            pager_error: RefCell::new(None),
        }
    }

    // ==================== Pager error channel ====================

    /// The last pager failure seen by this buffer, if any (mapped files only:
    /// truncation under the open buffer, or an I/O error reading a page).
    /// Cleared by [`Buffer::take_pager_error`].
    pub fn has_pager_error(&self) -> bool {
        self.pager_error.borrow().is_some()
    }

    /// Take (and clear) the recorded pager error, if any.
    pub fn take_pager_error(&self) -> Option<PagerError> {
        self.pager_error.borrow_mut().take()
    }

    /// Record a pager failure. Operations that hit the pager call this and
    /// return a safe default instead of panicking or propagating; the editor
    /// surfaces the recorded error (e.g. in the status bar).
    fn record_pager_error(&self, err: PagerError) {
        *self.pager_error.borrow_mut() = Some(err);
    }

    /// Validate file path and return canonical path
    fn validate_file_path(path: &Path) -> Result<PathBuf, String> {
        // Convert to absolute path if needed
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| format!("Failed to get current directory: {}", e))?
                .join(path)
        };

        // Check if file exists
        if !path.exists() {
            return Err(format!("File does not exist: {}", path.display()));
        }

        // Check if it's a file (not directory)
        if path.is_dir() {
            return Err(format!(
                "Path is a directory, not a file: {}",
                path.display()
            ));
        }

        // Block device files (Unix only) - opening these can hang the editor
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
            let file_type = metadata.file_type();
            if file_type.is_block_device()
                || file_type.is_char_device()
                || file_type.is_fifo()
                || file_type.is_socket()
            {
                return Err(format!(
                    "Cannot open device/special file: {}",
                    path.display()
                ));
            }
        }

        Ok(path)
    }

    // ==================== Content Access ====================

    /// Get total length in bytes
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.text.len() == 0
    }

    /// Get an iterator over lines starting from a specific line index.
    /// Lines are borrowed for owned regions and materialized for mapped
    /// regions (see `BText::lines_at`). Pager failures are recorded on the
    /// buffer and the unreadable lines are skipped (never a panic).
    pub fn lines_at(&self, start_line: usize) -> impl Iterator<Item = LineRef<'_>> {
        self.text.lines_at(start_line).filter_map(|r| match r {
            Ok(line) => Some(line),
            Err(e) => {
                self.record_pager_error(e);
                None
            }
        })
    }

    /// Get disjoint references to internal data for rendering without borrowing conflicts
    pub fn split_for_render(
        &mut self,
    ) -> (
        &BText,
        &Vec<Diagnostic>,
        &mut Vec<SyntaxLexerState>,
        &Option<PathBuf>,
    ) {
        (
            &self.text,
            &self.diagnostics,
            &mut self.syntax_state_cache,
            &self.filename,
        )
    }

    /// Get entire content as string (lossy: invalid UTF-8 becomes U+FFFD;
    /// the tree's bytes are untouched).
    /// Returns the full buffer content as a String. Prefer byte-native
    /// operations; this is for APIs that require an owned String.
    pub fn content_string(&self) -> String {
        match self.text.collect_bytes() {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => {
                self.record_pager_error(e);
                String::new()
            }
        }
    }

    /// Collect the buffer's raw bytes (byte-exact; mapped regions are read
    /// through the pager). On a pager failure the error is recorded and the
    /// bytes collected so far are returned.
    pub fn bytes(&self) -> Vec<u8> {
        // Cap the pre-allocation: for a sparse mapped file larger than RAM,
        // `len()` can be gigabytes — reserving it up front would OOM. The
        // vector still grows as chunks arrive.
        let mut out = Vec::with_capacity(self.text.len().min(1 << 20));
        for chunk in self.text.chunks() {
            match chunk {
                Ok(c) => out.extend_from_slice(c.as_bytes()),
                Err(e) => {
                    self.record_pager_error(e);
                    break;
                }
            }
        }
        out
    }

    /// Get a range of text as string
    pub fn get_range_as_string(&self, start: usize, length: usize) -> String {
        if length == 0 {
            return String::new();
        }
        let len = self.text.len();
        let actual_start = start.min(len);
        let actual_end = (actual_start + length).min(len);
        if actual_start >= actual_end {
            return String::new();
        }
        match self.text.slice_bytes(actual_start..actual_end) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => {
                self.record_pager_error(e);
                String::new()
            }
        }
    }

    /// Get character at byte position (byte_pos must be a char boundary;
    /// returns None past the end or on invalid UTF-8 at that position).
    pub fn char_at(&self, byte_pos: usize) -> Option<char> {
        self.text.char_at(byte_pos)
    }

    // ==================== Line Operations (O(log n) via the B-tree) ====================

    /// Get number of lines in buffer.
    /// On a pager failure the error is recorded and 0 is returned (the
    /// editor surfaces the error; rendering degrades rather than panicking).
    pub fn line_count(&self) -> usize {
        match self.text.line_count() {
            Ok(n) => n,
            Err(e) => {
                self.record_pager_error(e);
                0
            }
        }
    }

    /// Get byte offset for start of a line (O(log n)).
    pub fn line_to_byte(&self, line_idx: usize) -> Option<usize> {
        match self.text.line_to_byte(line_idx) {
            Ok(v) => v,
            Err(e) => {
                self.record_pager_error(e);
                None
            }
        }
    }

    /// Get which line a byte offset is on (O(log n)).
    pub fn byte_to_line(&self, byte_offset: usize) -> usize {
        let len = self.text.len();
        if byte_offset >= len {
            return self.line_count().saturating_sub(1);
        }
        match self.text.byte_to_line(byte_offset) {
            Ok(line) => line,
            Err(e) => {
                self.record_pager_error(e);
                0
            }
        }
    }

    /// Get content of a specific line (without newline)
    pub fn line(&self, line_idx: usize) -> Option<String> {
        self.line_with_newline(line_idx).map(|mut s| {
            // Pop the trailing newline in place (no realloc) instead of
            // `trim_end_matches('\n').to_string()` (which copies): one
            // fewer allocation per `line()` call.
            if s.ends_with('\n') {
                s.pop();
            }
            s
        })
    }

    /// Get content of a line with newline preserved
    pub fn line_with_newline(&self, line_idx: usize) -> Option<String> {
        let (start, end) = match self.text.line_byte_range(line_idx) {
            Ok(Some(r)) => r,
            Ok(None) => return None,
            Err(e) => {
                self.record_pager_error(e);
                return None;
            }
        };
        match self.text.slice_bytes(start..end) {
            // `String::from_utf8` reuses the Vec's allocation when valid
            // (no copy); `from_utf8_lossy(&bytes).into_owned()` always copied:
            // one fewer allocation per `line()` call.
            Ok(bytes) => Some(
                String::from_utf8(bytes)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()),
            ),
            Err(e) => {
                self.record_pager_error(e);
                None
            }
        }
    }

    /// Get length of a specific line in bytes (including newline)
    pub fn line_len(&self, line_idx: usize) -> Option<usize> {
        match self.text.line_byte_range(line_idx) {
            Ok(Some((start, end))) => Some(end - start),
            Ok(None) => None,
            Err(e) => {
                self.record_pager_error(e);
                None
            }
        }
    }

    // ==================== Editing Operations ====================

    /// Insert text at byte position
    pub fn insert(&mut self, pos: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let pos = pos.min(self.text.len());
        self.text.insert_bytes(pos, text.as_bytes());
        self.mark_modified();

        // Record for undo
        self.push_edit(Edit::Insert {
            pos,
            text: text.as_bytes().to_vec(),
        });
        self.redo_stack.clear();
    }

    /// Delete text at byte position. Atomic: on a pager failure the tree is
    /// untouched, the error is recorded, and no undo entry is pushed.
    pub fn delete(&mut self, pos: usize, len: usize) {
        if len == 0 || pos >= self.text.len() {
            return;
        }
        let actual_len = len.min(self.text.len() - pos);

        // Save text for undo
        let deleted_text = match self.text.delete_bytes(pos, actual_len) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.record_pager_error(e);
                return;
            }
        };
        self.mark_modified();

        // Record for undo
        self.push_edit(Edit::Delete {
            pos,
            text: deleted_text,
        });
        self.redo_stack.clear();
    }

    /// Insert a single character (optimized path)
    pub fn insert_char(&mut self, pos: usize, ch: char) {
        let pos = pos.min(self.text.len());
        let mut buf = [0u8; 4];
        let bytes = ch.encode_utf8(&mut buf).as_bytes();
        self.text.insert_bytes(pos, bytes);
        self.mark_modified();

        // Record for undo
        self.push_edit(Edit::Insert {
            pos,
            text: bytes.to_vec(),
        });
        self.redo_stack.clear();
    }

    /// Delete a single character at a byte position (the position must be a
    /// char boundary; a no-op otherwise).
    pub fn delete_char(&mut self, pos: usize) {
        if pos >= self.text.len() {
            return;
        }
        if let Some(ch) = self.text.char_at(pos) {
            let char_len = ch.len_utf8();
            // Atomic: on a pager failure the tree is untouched.
            match self.text.delete_bytes(pos, char_len) {
                Ok(_) => {
                    self.mark_modified();

                    // Record for undo
                    let mut encoded = [0u8; 4];
                    let bytes = ch.encode_utf8(&mut encoded).as_bytes().to_vec();
                    self.push_edit(Edit::Delete { pos, text: bytes });
                    self.redo_stack.clear();
                }
                Err(e) => {
                    self.record_pager_error(e);
                }
            }
        }
    }

    // ==================== Undo/Redo ====================

    /// Undo the last edit. Byte-exact inverses over the B-tree: undoing an
    /// insert deletes exactly the inserted bytes; undoing a delete reinserts
    /// the recorded bytes.
    ///
    /// Atomic at the group level: the tree is snapshotted (O(1) — the B-tree
    /// is persistent, `Clone` shares the root) before applying, so a pager
    /// failure rolls the whole group back, the error is recorded, the group
    /// stays on the undo stack for retry, and `false` is returned. No partial
    /// undo is ever observable.
    pub fn undo(&mut self) -> bool {
        if let Some(group) = self.undo_stack.pop_back() {
            // O(1) snapshot: BText::clone shares the Rc root.
            let snapshot = self.text.clone();
            let mut ok = true;
            // Apply edits in reverse order for undo
            for edit in group.edits.iter().rev() {
                let applied = match edit {
                    Edit::Insert { pos, text } => {
                        // Undo insert = delete
                        match self.text.delete_bytes(*pos, text.len()) {
                            Ok(_) => true,
                            Err(e) => {
                                self.record_pager_error(e);
                                false
                            }
                        }
                    }
                    Edit::Delete { pos, text } => {
                        // Undo delete = insert (infallible)
                        self.text.insert_bytes(*pos, text);
                        true
                    }
                };
                if !applied {
                    ok = false;
                    break;
                }
            }
            if !ok {
                self.text = snapshot;
                self.undo_stack.push_back(group);
                return false;
            }
            self.redo_stack.push_back(group);
            self.mark_modified();
            true
        } else {
            false
        }
    }

    /// Redo the last undone edit. Atomic like [`Buffer::undo`]: a pager
    /// failure rolls back, keeps the group on the redo stack, and returns
    /// `false`.
    pub fn redo(&mut self) -> bool {
        if let Some(group) = self.redo_stack.pop_back() {
            let snapshot = self.text.clone();
            let mut ok = true;
            // Apply edits in original order for redo
            for edit in &group.edits {
                let applied = match edit {
                    Edit::Insert { pos, text } => {
                        self.text.insert_bytes(*pos, text);
                        true
                    }
                    Edit::Delete { pos, text } => {
                        match self.text.delete_bytes(*pos, text.len()) {
                            Ok(_) => true,
                            Err(e) => {
                                self.record_pager_error(e);
                                false
                            }
                        }
                    }
                };
                if !applied {
                    ok = false;
                    break;
                }
            }
            if !ok {
                self.text = snapshot;
                self.redo_stack.push_back(group);
                return false;
            }
            self.undo_stack.push_back(group);
            self.mark_modified();
            true
        } else {
            false
        }
    }

    // ==================== File Operations ====================

    /// Save buffer to file
    pub fn save(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let path = self.filename.as_ref().ok_or("No filename set for buffer")?;

        // Write to temp file first for atomic save
        let parent = match path.parent() {
            Some(p) => p,
            None => Path::new("."),
        };
        let mut temp_file = NamedTempFile::new_in(parent)?;

        // N-143: If the buffer was loaded from invalid UTF-8 and has NOT been
        // modified, write the original bytes back byte-exact (never destructive).
        // If modified, the tree content is written (user chose to proceed).
        if !self.modified {
            if let Some(original) = &self.original_bytes {
                temp_file.write_all(original)?;
            } else {
                // Single streaming pass over the tree; mapped regions are
                // read through the pager (a pager failure aborts the save
                // with a typed error — never a partial file: the temp file
                // is only renamed on success).
                self.text
                    .write_to(&mut temp_file)
                    .map_err(|e| format!("failed to write buffer content: {e}"))?;
            }
        } else {
            self.text
                .write_to(&mut temp_file)
                .map_err(|e| format!("failed to write buffer content: {e}"))?;
            // Once modified, the original bytes are no longer authoritative.
            self.original_bytes = None;
        }
        temp_file.flush()?;

        // CRITICAL: sync_all() ensures data is flushed to disk before atomic rename.
        // Without this, a system crash after persist() could result in data loss.
        temp_file.as_file().sync_all()?;

        // Atomic rename
        temp_file.persist(path)?;

        // Update state
        self.modified = false;
        self.last_modified_time = fs::metadata(path).ok().and_then(|m| m.modified().ok());
        // A successful save resyncs with disk: any recorded divergence is stale.
        self.acknowledged_mtime = None;

        Ok(())
    }

    /// Save buffer to a specific file (save as)
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
        self.filename = Some(path.as_ref().to_path_buf());
        self.save()
    }

    // ==================== State Management ====================

    /// Push an edit to the undo stack, with smart grouping
    fn push_edit(&mut self, edit: Edit) {
        let should_group = if let Some(last_group) = self.undo_stack.back() {
            if let Some(prev_edit) = last_group.edits.last() {
                // For now, we use LexerState::Normal as we don't have full lexer integration here yet
                self.undo_grouper
                    .should_group(prev_edit, &edit, LexerState::Normal)
            } else {
                false
            }
        } else {
            false
        };

        if should_group {
            if let Some(group) = self.undo_stack.back_mut() {
                group.add_edit(edit);
            }
        } else {
            let mut group = UndoGroup::new();
            group.add_edit(edit);
            if self.undo_stack.len() >= MAX_UNDO_DEPTH {
                self.undo_stack.pop_front(); // O(1) discard oldest
            }
            self.undo_stack.push_back(group);
        }
    }

    /// Mark buffer as modified
    fn mark_modified(&mut self) {
        self.modified = true;
        self.version += 1;
    }

    /// Get the type of buffer
    pub fn buffer_kind(&self) -> BufferKind {
        self.buffer_kind
    }

    pub fn display_name(&self) -> String {
        match self.filename.as_ref() {
            Some(p) => p.display().to_string(),
            None => "*scratch*".to_string(),
        }
    }

    /// Check if the file changed on disk since this buffer last synced with
    /// it. A change the user already answered the reload question for
    /// (`acknowledged_mtime`) is not new: declining records that exact
    /// on-disk state, and only a *newer* state re-arms the question.
    pub fn check_external_modification(&self) -> bool {
        if let (Some(path), Some(stored_time)) = (&self.filename, &self.last_modified_time)
            && let Ok(metadata) = fs::metadata(path)
                && let Ok(current_time) = metadata.modified()
                    && current_time != *stored_time {
                        return Some(current_time) != self.acknowledged_mtime;
                    }
        false
    }

    /// The on-disk mtime at the moment of the call, if the file exists.
    /// The decline path of the reload question records this so the same
    /// on-disk state is never asked about twice.
    pub fn current_disk_mtime(&self) -> Option<SystemTime> {
        self.filename
            .as_ref()
            .and_then(|path| fs::metadata(path).ok())
            .and_then(|metadata| metadata.modified().ok())
    }

    /// Replace this buffer's file-derived state with a freshly loaded one,
    /// keeping the buffer's identity: filename, windows, and undo grouper
    /// survive; text, encoding view, mtime, and caches come from `fresh`.
    /// Post: the buffer is unmodified, undo/redo are empty, and any
    /// recorded divergence is cleared.
    pub fn adopt_loaded(&mut self, fresh: Buffer) {
        self.text = fresh.text;
        self.buffer_kind = fresh.buffer_kind;
        self.modified = false;
        self.last_modified_time = fresh.last_modified_time;
        self.acknowledged_mtime = None;
        self.version += 1;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.diagnostics = fresh.diagnostics;
        self.syntax_state_cache = fresh.syntax_state_cache;
        self.original_bytes = fresh.original_bytes;
        self.pager_error = fresh.pager_error;
    }

    // ==================== Diagnostics ====================

    /// Add a diagnostic to this buffer
    pub fn add_diagnostic(&mut self, diag: Diagnostic) {
        self.diagnostics.push(diag);
    }

    /// Clear all diagnostics from this buffer
    pub fn clear_diagnostics(&mut self) {
        self.diagnostics.clear();
    }

    /// Get diagnostics for a specific line (0-indexed)
    /// Note: Diagnostic line numbers are 1-indexed
    pub fn diagnostics_for_line(&self, line: usize) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.line == line + 1)
            .collect()
    }

    // ==================== Search Operations ====================

    /// Find all occurrences of a pattern (streaming, no full allocation)
    /// Returns byte offsets of all matches.
    /// Yield every match start of `pattern` in ascending byte order.
    /// `limit_pos`: only matches with `start < limit_pos` are yielded
    /// (`usize::MAX` = no limit). Returning `false` from `emit` stops the
    /// enumeration early. Streaming: O(pattern.len()) extra space.
    ///
    /// Matches spanning chunk boundaries — including across several tiny
    /// chunks (e.g. 1-byte leaves left by typing into a mapped region) — are
    /// found via `tail`, the last `pattern.len()-1` bytes of the *stream*
    /// before the current chunk: a match starting in the tail is
    /// `pattern.len()` bytes, so it ends within the first `overlap_size`
    /// bytes of the chunk, and searching `tail ++ chunk[..overlap_size]`
    /// catches exactly the boundary matches. (Session-6 fix: the old
    /// per-chunk overlap required the current chunk to complete the pattern
    /// and missed matches spanning three or more chunks.)
    fn for_each_match(
        &self,
        pattern: &str,
        limit_pos: usize,
        mut emit: impl FnMut(usize) -> bool,
    ) {
        debug_assert!(!pattern.is_empty());
        let pbytes = pattern.as_bytes();
        // Border check (KMP table): if the pattern can't overlap with itself,
        // the SIMD `match_indices` path is complete; else use the overlap loop.
        let table = kmp_table(pbytes);
        let has_border = table[pbytes.len() - 1] > 0;
        let plen = pbytes.len();
        let overlap_size = plen.saturating_sub(1);
        let mut tail: Vec<u8> = Vec::with_capacity(overlap_size);
        // Reused probe buffer: tail ++ chunk[..overlap_size].
        let mut probe: Vec<u8> = Vec::with_capacity(2 * overlap_size);
        let mut byte_pos = 0usize;
        let mut keep_going = true;
        for chunk in self.text.chunks() {
            if !keep_going {
                break;
            }
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    self.record_pager_error(e);
                    break;
                }
            };
            let bytes = chunk.as_bytes();

            // Boundary matches (start in tail, end in this chunk) come
            // before within-chunk matches: ascending order. The probe runs
            // even when `byte_pos >= limit_pos`: a match starting before the
            // limit can still end in this chunk.
            if !tail.is_empty() && !bytes.is_empty() {
                let probe_len = overlap_size.min(bytes.len());
                probe.clear();
                probe.extend_from_slice(&tail);
                probe.extend_from_slice(&bytes[..probe_len]);
                let tlen = tail.len();
                emit_matches(&probe, pattern, has_border, |p| {
                    if p < tlen {
                        let sp = byte_pos - tlen + p;
                        if sp < limit_pos {
                            keep_going = emit(sp);
                        }
                    }
                });
            }
            if byte_pos >= limit_pos {
                break;
            }
            // Within-chunk matches. The full chunk is searched (a match may
            // start before `limit_pos` and end after it); only starts below
            // the limit are emitted.
            emit_matches(bytes, pattern, has_border, |pos| {
                let sp = byte_pos + pos;
                if sp < limit_pos {
                    keep_going = emit(sp);
                }
            });

            // Advance the tail: last `overlap_size` bytes of the stream.
            // (Take only the chunk's suffix: extending with the whole chunk
            // then draining would memmove O(chunk) per chunk.)
            let keep = overlap_size.min(bytes.len());
            tail.extend_from_slice(&bytes[bytes.len() - keep..]);
            if tail.len() > overlap_size {
                tail.drain(..tail.len() - overlap_size);
            }

            byte_pos += bytes.len();
        }
    }

    pub fn find_all(&self, pattern: &str) -> Vec<usize> {
        if pattern.is_empty() {
            return Vec::new();
        }
        let mut results = Vec::new();
        self.for_each_match(pattern, usize::MAX, |pos| {
            results.push(pos);
            true
        });
        results
    }

    /// Find next occurrence starting from position (streaming, O(pattern.len()) space)
    pub fn find_forward(&self, pattern: &str, start_pos: usize) -> Option<usize> {
        if pattern.is_empty() {
            return None;
        }
        // Matches arrive in ascending order: the first at-or-after
        // `start_pos` is the answer; stop the enumeration there. (The
        // closure must ignore later emits: `kmp_find` keeps emitting
        // after we ask to stop.)
        let mut found = None;
        self.for_each_match(pattern, usize::MAX, |pos| {
            if found.is_none() && pos >= start_pos {
                found = Some(pos);
            }
            found.is_none()
        });
        found
    }

    /// Find previous occurrence before position (streaming backward)
    pub fn find_backward(&self, pattern: &str, start_pos: usize) -> Option<usize> {
        if pattern.is_empty() {
            return None;
        }

        // For backward search, we collect all matches before start_pos and take the last one.
        // The `limit_pos` bound stops the stream once chunks start at-or-after
        // `start_pos` (a match starting before the limit can still end in the
        // next chunk; the boundary probe runs before the stop).
        let mut last_match: Option<usize> = None;
        self.for_each_match(pattern, start_pos, |pos| {
            last_match = Some(pos);
            true
        });
        last_match
    }
}

/// KMP failure function for `pattern`: `table[i]` is the length of the
/// longest proper prefix of `pattern[..=i]` that is also a suffix.
/// Used only for the border check (whether the pattern can overlap with
/// itself); the search itself uses SIMD `str` APIs, not KMP (a byte-at-a-time
/// KMP loop is 4x slower than memchr-backed `str::find` — measured session 6).
fn kmp_table(pattern: &[u8]) -> Vec<usize> {
    let m = pattern.len();
    let mut table = vec![0; m];
    let mut len = 0usize;
    for i in 1..m {
        while len > 0 && pattern[i] != pattern[len] {
            len = table[len - 1];
        }
        if pattern[i] == pattern[len] {
            len += 1;
        }
        table[i] = len;
    }
    table
}

/// Emit matches of `pattern` in `bytes` (byte offsets relative to `bytes`).
/// `has_border`: whether the pattern can overlap with itself (KMP table's
/// last entry > 0). Without a border, matches cannot overlap, so the SIMD
/// `match_indices` fast path is complete; with a border, fall back to the
/// manual overlapping loop. Both paths require valid UTF-8 for `str` APIs;
/// invalid bytes use a direct bytewise scan.
fn emit_matches(bytes: &[u8], pattern: &str, has_border: bool, mut emit: impl FnMut(usize)) {
    if pattern.is_empty() || bytes.len() < pattern.len() {
        return;
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        if !has_border {
            // Fast path: no self-overlap possible, `match_indices` (which
            // reuses its searcher) finds every match. 42% faster per chunk
            // than a `str::find` loop in micro-benchmarks.
            for (pos, _) in s.match_indices(pattern) {
                emit(pos);
            }
        } else {
            // Slow path: overlapping matches need the manual advance-by-1.
            // Snap to char boundaries: `pos + 1` can land mid-char for
            // multi-byte patterns (matches themselves always start at lead
            // bytes, so snapping forward can't skip one).
            let mut start = 0;
            while start < s.len() {
                while start < s.len() && !s.is_char_boundary(start) {
                    start += 1;
                }
                let Some(idx) = s[start..].find(pattern) else {
                    break;
                };
                let pos = start + idx;
                emit(pos);
                start = pos + 1;
            }
        }
    } else {
        // Non-UTF-8 bytes: direct scan (advance by 1 = overlapping).
        let needle = pattern.as_bytes();
        let mut start = 0;
        while start + needle.len() <= bytes.len() {
            if &bytes[start..start + needle.len()] == needle {
                emit(start);
            }
            start += 1;
        }
    }
}

impl Buffer {
    /// Replace all occurrences (streaming - no full buffer allocation)
    /// Replace every non-overlapping occurrence of `pattern` with
    /// `replacement`, left to right — the same semantics as
    /// `str::replace`. (`find_all` reports overlapping matches, which is
    /// what search highlighting wants; replacement must not: applying
    /// overlapping matches is order-dependent — right-to-left gives "b"
    /// for "aaa".replace("aa"→"b") while left-to-right gives "bb" — so any
    /// match starting inside the previous replacement's span is skipped.)
    /// Returns the number of replacements applied.
    pub fn replace_all(&mut self, pattern: &str, replacement: &str) -> usize {
        if pattern.is_empty() {
            return 0;
        }

        // Non-overlapping matches in the ORIGINAL text (replaced text is
        // never rescanned, like `str::replace`).
        let mut matches = Vec::new();
        let mut end = 0usize; // end of the last kept match
        self.for_each_match(pattern, usize::MAX, |pos| {
            if matches.is_empty() || pos >= end {
                end = pos + pattern.len();
                matches.push(pos);
            }
            true
        });
        if matches.is_empty() {
            return 0;
        }

        // Apply replacements in reverse order to keep byte positions valid,
        // recording the REAL edits in a single undo group (N-146: no fake marker).
        let mut group = UndoGroup::new();
        for &pos in matches.iter().rev() {
            // Record the deleted text for undo (delete_bytes is atomic: on a
            // pager failure the tree is untouched and we stop the operation).
            let deleted = match self.text.delete_bytes(pos, pattern.len()) {
                Ok(bytes) => bytes,
                Err(e) => {
                    self.record_pager_error(e);
                    break;
                }
            };
            self.text.insert_bytes(pos, replacement.as_bytes());
            // The edit is Delete(old) then Insert(new) at the same position.
            // Edit.pos is a BYTE index (see undo/redo).
            group.add_edit(Edit::Delete {
                pos,
                text: deleted,
            });
            group.add_edit(Edit::Insert {
                pos,
                text: replacement.as_bytes().to_vec(),
            });
        }

        self.mark_modified();

        // Push the single group for the entire replace-all operation.
        if self.undo_stack.len() >= MAX_UNDO_DEPTH {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(group);
        self.redo_stack.clear();

        matches.len()
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== Legacy API Compatibility ====================
// These provide compatibility with code that used the old BufferBackend interface

impl Buffer {
    /// Legacy: Access the underlying text store directly (for code migrating
    /// from BufferBackend)
    pub fn backend_len(&self) -> usize {
        self.len()
    }

    /// Legacy: Check if backend is empty
    pub fn backend_is_empty(&self) -> bool {
        self.is_empty()
    }

    /// Legacy: Get content as string (same as to_string)
    pub fn backend_to_string(&self) -> String {
        self.content_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer() {
        let buf = Buffer::new();
        assert!(buf.is_empty());
        assert_eq!(buf.line_count(), 1); // Empty buffer has 1 line
        assert!(!buf.modified);
    }

    #[test]
    fn test_insert_and_delete() {
        let mut buf = Buffer::new();
        buf.insert(0, "Hello, World!");
        assert_eq!(buf.content_string(), "Hello, World!");
        assert!(buf.modified);

        buf.delete(0, 7);
        assert_eq!(buf.content_string(), "World!");
    }

    #[test]
    fn test_line_operations() {
        let mut buf = Buffer::new();
        buf.insert(0, "Line 1\nLine 2\nLine 3");

        assert_eq!(buf.line_count(), 3);
        assert_eq!(buf.line(0), Some("Line 1".to_string()));
        assert_eq!(buf.line(1), Some("Line 2".to_string()));
        assert_eq!(buf.line(2), Some("Line 3".to_string()));

        assert_eq!(buf.line_to_byte(0), Some(0));
        assert_eq!(buf.line_to_byte(1), Some(7));
        assert_eq!(buf.line_to_byte(2), Some(14));
    }

    #[test]
    fn test_undo_redo() {
        let mut buf = Buffer::new();
        buf.insert(0, "Hello");
        assert_eq!(buf.content_string(), "Hello");

        buf.undo();
        assert_eq!(buf.content_string(), "");

        buf.redo();
        assert_eq!(buf.content_string(), "Hello");
    }

    #[test]
    fn test_smart_undo_grouping() {
        let mut buf = Buffer::new();
        // Type "hello world" - with batch grouping, all single-char edits group together
        buf.insert_char(0, 'h');
        buf.insert_char(1, 'e');
        buf.insert_char(2, 'l');
        buf.insert_char(3, 'l');
        buf.insert_char(4, 'o');
        buf.insert_char(5, ' ');
        buf.insert_char(6, 'w');
        buf.insert_char(7, 'o');
        buf.insert_char(8, 'r');
        buf.insert_char(9, 'l');
        buf.insert_char(10, 'd');

        // With batch grouping (100 per batch), all chars go in one group
        assert_eq!(buf.undo_stack.len(), 1);
        assert_eq!(buf.content_string(), "hello world");

        // Single undo removes all (batched together)
        buf.undo();
        assert_eq!(buf.content_string(), "");
    }

    #[test]
    fn test_find() {
        let mut buf = Buffer::new();
        buf.insert(0, "foo bar foo baz foo");

        let matches = buf.find_all("foo");
        assert_eq!(matches, vec![0, 8, 16]);

        assert_eq!(buf.find_forward("foo", 0), Some(0));
        assert_eq!(buf.find_forward("foo", 1), Some(8));
        assert_eq!(buf.find_backward("foo", 19), Some(16));
    }

    #[test]
    fn test_find_multibyte_no_panic() {
        // Regression: the ropey-era implementation sliced a `str` at
        // `match_idx + 1`, which panics when the next byte is a UTF-8
        // continuation byte. The byte-native search must not panic and must
        // report exact byte positions.
        let mut buf = Buffer::new();
        buf.insert(0, "café x café"); // bytes: café=0..5, ' '=5, x=6, ' '=7, café=8..13

        assert_eq!(buf.find_all("café"), vec![0, 8]);
        assert_eq!(buf.find_forward("café", 1), Some(8));
        assert_eq!(buf.find_forward("café", 9), None);
        assert_eq!(buf.find_backward("café", 13), Some(8));
        assert_eq!(buf.find_backward("café", 8), Some(0));
    }

    #[test]
    fn test_open_mapped_healthy_file() {
        use std::io::Write;
        let path = std::env::temp_dir().join("erax_test_mapped_healthy.txt");
        // 2.5 MiB → three mapped leaves (1 MiB regions).
        let mut f = std::fs::File::create(&path).expect("create");
        let chunk = vec![b'x'; 1024];
        for _ in 0..2560 {
            f.write_all(&chunk).expect("write");
        }
        drop(f);

        let mut buf = Buffer::open_mapped(&path).expect("open_mapped");
        assert_eq!(buf.len(), 2560 * 1024);
        assert_eq!(buf.line_count(), 1, "no newlines → one line");
        assert!(!buf.has_pager_error());

        // Editing a mapped buffer works: inserts create owned leaves.
        buf.insert(0, "hello");
        assert_eq!(buf.len(), 2560 * 1024 + 5);
        assert!(!buf.has_pager_error());
        assert_eq!(&buf.content_string()[..5], "hello");
        assert!(buf.undo(), "undo of mapped-buffer insert");
        assert_eq!(buf.len(), 2560 * 1024);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_mapped_truncation_delete_is_noop_with_recorded_error() {
        use std::io::Write;
        let path = std::env::temp_dir().join("erax_test_truncate_delete.txt");
        // 2 MiB → two 1 MiB mapped leaves.
        let mut f = std::fs::File::create(&path).expect("create");
        let chunk = vec![b'x'; 1024];
        for _ in 0..2048 {
            f.write_all(&chunk).expect("write");
        }
        drop(f);

        let mut buf = Buffer::open_mapped(&path).expect("open_mapped");
        assert_eq!(buf.len(), 2 * 1024 * 1024);
        assert!(buf.undo_stack.is_empty());

        // Adversarial: truncate the file under the open buffer.
        let t = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open");
        t.set_len(100).expect("truncate");
        drop(t);

        // Deleting now-unreadable mapped bytes: a recorded-error no-op —
        // tree metadata untouched, no undo entry, not marked modified.
        // Never a panic, never SIGBUS.
        buf.delete(500_000, 100);
        assert!(buf.has_pager_error(), "truncation must be recorded");
        assert_eq!(buf.len(), 2 * 1024 * 1024, "tree metadata untouched");
        assert!(buf.undo_stack.is_empty(), "failed delete pushes no undo entry");
        assert!(!buf.modified, "failed delete does not mark modified");
        let err = buf.take_pager_error().expect("error recorded");
        assert!(!buf.has_pager_error(), "take clears the error");
        let msg = format!("{err}");
        assert!(msg.contains("truncat"), "unexpected error: {msg}");

        // Read path degrades the same way: partial bytes + recorded error.
        let bytes = buf.bytes();
        assert!(buf.has_pager_error());
        assert!(bytes.is_empty(), "first mapped leaf unreadable → nothing collected");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_load_file_with_invalid_utf8() {
        use std::io::Write;

        // Create a temporary file with invalid UTF-8 bytes
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("erax_test_invalid_utf8.txt");

        // Write valid text followed by invalid UTF-8 sequence (0xFF is invalid)
        let mut file = std::fs::File::create(&test_file).expect("Failed to create test file");
        file.write_all(b"Hello \xFF\xFE World")
            .expect("Failed to write test data");
        drop(file);

        // Load the file - should return InvalidUtf8 error (N-143: no silent lossy)
        let result = Buffer::from_file(&test_file);

        // Clean up
        let _ = std::fs::remove_file(&test_file);

        // Verify it returns the typed InvalidUtf8 error with bytes preserved
        match result {
            Err(LoadError::InvalidUtf8 { bytes, .. }) => {
                assert_eq!(bytes, b"Hello \xFF\xFE World", "Bytes must be preserved byte-exact");
                // After user chooses "proceed", from_invalid_utf8_bytes creates
                // a buffer with lossy view + preserved original bytes.
                let buffer = Buffer::from_invalid_utf8_bytes(
                    test_file.clone(),
                    bytes,
                );
                let content = buffer.content_string();
                assert!(
                    content.contains("Hello"),
                    "Should contain valid text before invalid bytes"
                );
                assert!(
                    content.contains("World"),
                    "Should contain valid text after invalid bytes"
                );
                assert!(
                    buffer.original_bytes.is_some(),
                    "Original bytes must be preserved for byte-exact save"
                );
            }
            other => panic!("Expected LoadError::InvalidUtf8, got {:?}", other.is_ok()),
        }
    }

    #[test]
    fn test_replace_all_undo_restores_real_content() {
        // N-146: replace_all must record real edits, not a fake marker.
        let mut buffer = Buffer::new();
        buffer.insert(0, "foo bar foo baz foo");
        let original = buffer.content_string();

        let count = buffer.replace_all("foo", "qux");
        assert_eq!(count, 3);
        assert_eq!(buffer.content_string(), "qux bar qux baz qux");

        // Undo must restore the ACTUAL prior state, not corrupt content.
        assert!(buffer.undo(), "undo should succeed");
        assert_eq!(
            buffer.content_string(),
            original,
            "undo of replace_all must restore original content byte-exact"
        );

        // Redo must re-apply the replacement.
        assert!(buffer.redo(), "redo should succeed");
        assert_eq!(buffer.content_string(), "qux bar qux baz qux");
    }

    #[test]
    fn test_find_all_spanning_tiny_mapped_leaves() {
        // Regression (session 6): `find_all` missed matches spanning three
        // or more chunks when a middle chunk was shorter than the pattern —
        // the old overlap logic required the *current* chunk to complete the
        // pattern, so a match split across two boundaries was never found.
        // Build three adjacent mapped leaves by fragmenting a mapped file
        // with insert/delete pairs (content ends up "abcdef" again).
        let path =
            std::env::temp_dir().join(format!("erax-findall-{}.txt", std::process::id()));
        std::fs::write(&path, b"abcdef").expect("write temp file");
        let mut buf = Buffer::open_mapped(&path).expect("open_mapped");
        buf.insert(2, "X");
        buf.insert(3, "Y");
        buf.delete(2, 2);
        buf.insert(4, "Z");
        buf.delete(4, 1);
        assert_eq!(buf.content_string(), "abcdef");
        // The pattern now spans three leaves ("ab" | "cd" | "ef").
        assert_eq!(buf.find_all("abcdef"), vec![0]);
        assert_eq!(buf.find_all("bcdef"), vec![1]);
        assert_eq!(buf.find_all("cde"), vec![2]);
        assert_eq!(buf.find_all("def"), vec![3]);
        assert_eq!(buf.find_all("xyz"), Vec::<usize>::new());
        // Overlapping matches still reported (kmp_find semantics).
        assert_eq!(buf.find_all("bcd"), vec![1]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_replace_all_is_non_overlapping() {
        // Regression (session 6): `replace_all` used to apply `find_all`'s
        // overlapping matches right-to-left, giving order-dependent results
        // ("aaa".replace("aa"→"b") yielded "b"). It now mirrors
        // `str::replace`: non-overlapping, left to right.
        let mut buf = Buffer::from_string("aaa");
        assert_eq!(buf.replace_all("aa", "b"), 1);
        assert_eq!(buf.content_string(), "ba");

        let mut buf = Buffer::from_string("aaaa");
        assert_eq!(buf.replace_all("aa", "b"), 2);
        assert_eq!(buf.content_string(), "bb");

        // Replaced text is not rescanned.
        let mut buf = Buffer::from_string("aa");
        assert_eq!(buf.replace_all("a", "aa"), 2);
        assert_eq!(buf.content_string(), "aaaa");

        // Undo restores the original through the single group.
        assert!(buf.undo());
        assert_eq!(buf.content_string(), "aa");
    }

    // -- Differential soak over the rewired Buffer API (BText, no ropey) --
    //
    // The BText-level fuzz proves the tree; this proves the Buffer layer on
    // top: undo/redo integrity through the grouper, find/replace_all
    // against naive models, and line-API consistency. Scale via
    // ERAX_BUFFER_FUZZ_ITERS batches (default 3_000 for a debug `cargo
    // test`; heavy release soaks set it higher and are recorded in
    // PROGRESS.md).

    struct BufRng(u64);
    impl BufRng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
    }

    /// Random valid-UTF-8 text: ASCII, multi-byte, newlines, tabs.
    fn rnd_text(rng: &mut BufRng) -> String {
        const PIECES: &[&str] = &[
            "a", "hello", " ", "\n", "x\n", "\n\n", "héllo", "→", "😀", "日本語",
            "  ", "\t", "z", "ab\ncd", "0", "e\u{301}",
        ];
        let n = rng.below(6);
        let mut s = String::new();
        for _ in 0..n {
            s.push_str(PIECES[rng.below(PIECES.len())]);
        }
        s
    }

    /// Snap a byte position forward to a char boundary.
    fn snap_fwd(s: &str, mut pos: usize) -> usize {
        pos = pos.min(s.len());
        while pos < s.len() && !s.is_char_boundary(pos) {
            pos += 1;
        }
        pos
    }

    /// Naive overlapping find_all (mirrors `kmp_find` semantics:
    /// overlapping matches, char-boundary starts).
    fn naive_find_all(hay: &str, pat: &str) -> Vec<usize> {
        if pat.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut start = 0;
        while start <= hay.len() {
            while start < hay.len() && !hay.is_char_boundary(start) {
                start += 1;
            }
            match hay[start..].find(pat) {
                Some(i) => {
                    let p = start + i;
                    out.push(p);
                    start = p + 1;
                }
                None => break,
            }
        }
        out
    }

    fn check_buffer_equiv(buf: &Buffer, model: &str, rng: &mut BufRng, ctx: &str) {
        assert_eq!(buf.bytes(), model.as_bytes(), "bytes diverged ({ctx})");
        assert_eq!(buf.content_string(), model, "content_string diverged ({ctx})");
        assert!(!buf.has_pager_error(), "unexpected pager error ({ctx})");
        // Line API vs naive model. ("a\n" is 2 lines: the trailing empty
        // line counts — `split('\n')`, not `split_inclusive`, which drops it.)
        let model_lines: Vec<&str> = model.split_inclusive('\n').collect();
        let expect_count = model.split('\n').count();
        assert_eq!(buf.line_count(), expect_count, "line_count ({ctx})");
        for _ in 0..4 {
            let li = rng.below(expect_count);
            let start: usize = model_lines[..li].iter().map(|l| l.len()).sum();
            assert_eq!(buf.line_to_byte(li), Some(start), "line_to_byte({li}) ({ctx})");
            assert_eq!(buf.byte_to_line(start), li, "byte_to_line({start}) ({ctx})");
            if !model.is_empty() {
                let pos = rng.below(model.len() + 1);
                let pos = snap_fwd(model, pos);
                // Naive byte_to_line: count newlines before pos.
                let want = model[..pos].bytes().filter(|&b| b == b'\n').count();
                assert_eq!(buf.byte_to_line(pos), want, "byte_to_line({pos}) ({ctx})");
            }
        }
        // Find APIs vs naive models.
        for _ in 0..3 {
            let pat = rnd_text(rng);
            if pat.is_empty() {
                continue;
            }
            let want = naive_find_all(model, &pat);
            assert_eq!(buf.find_all(&pat), want, "find_all({pat:?}) ({ctx})");
            // find_forward from a random start: naive = first want >= start.
            let start = rng.below(model.len() + 1);
            let fwd_naive = want.iter().copied().find(|&p| p >= start);
            assert_eq!(buf.find_forward(&pat, start), fwd_naive, "find_forward({pat:?}, {start}) ({ctx})");
            let bwd_naive = want.iter().copied().rev().find(|&p| p < start);
            assert_eq!(
                buf.find_backward(&pat, start),
                bwd_naive,
                "find_backward({pat:?}, {start}) ({ctx})"
            );
        }
    }

    #[test]
    fn buffer_differential_soak_rewired_path() {
        let iters: usize = std::env::var("ERAX_BUFFER_FUZZ_ITERS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3_000);
        let mut rng = BufRng(0x9E3779B97F4A7C15);
        // Epochs use a fresh Buffer so undo-all has a known floor.
        let mut iter = 0;
        while iter < iters {
            let epoch_len = (iters - iter).min(400);
            let init = rnd_text(&mut rng);
            let mut buf = Buffer::from_string(&init);
            let mut model = init.clone();
            let epoch_ctx = format!("epoch@{iter}");
            for _ in 0..epoch_len {
                let batch = 1 + rng.below(5);
                for _ in 0..batch {
                    match rng.below(6) {
                        0 | 1 => {
                            // Insert at a char boundary (Buffer::insert takes &str).
                            let pos = snap_fwd(&model, rng.below(model.len() + 1));
                            let text = rnd_text(&mut rng);
                            buf.insert(pos, &text);
                            model.insert_str(pos, &text);
                        }
                        2 | 3 => {
                            // Delete a char-aligned range.
                            if !model.is_empty() {
                                let start = snap_fwd(&model, rng.below(model.len()));
                                let max_chars = model[start..].chars().count().min(8);
                                let k = rng.below(max_chars + 1);
                                let end: usize = model[start..]
                                    .char_indices()
                                    .nth(k)
                                    .map(|(i, _)| start + i)
                                    .unwrap_or(model.len());
                                buf.delete(start, end - start);
                                model.drain(start..end);
                            }
                        }
                        4 => {
                            // replace_all vs str::replace semantics
                            // (non-overlapping, left to right).
                            let pat = rnd_text(&mut rng);
                            if !pat.is_empty() {
                                let repl = rnd_text(&mut rng);
                                let want_n = model.matches(&pat).count();
                                let n = buf.replace_all(&pat, &repl);
                                assert_eq!(n, want_n, "replace_all count ({epoch_ctx})");
                                model = model.replace(&pat, &repl);
                            }
                        }
                        _ => {
                            // insert_char / delete_char single-char paths.
                            if rng.below(2) == 0 {
                                let pos = snap_fwd(&model, rng.below(model.len() + 1));
                                let ch = ['x', 'é', '😀', '\n'][rng.below(4)];
                                buf.insert_char(pos, ch);
                                model.insert(pos, ch);
                            } else if !model.is_empty() {
                                let pos = snap_fwd(&model, rng.below(model.len()));
                                if let Some(ch) = model[pos..].chars().next() {
                                    buf.delete_char(pos);
                                    model.drain(pos..pos + ch.len_utf8());
                                }
                            }
                        }
                    }
                }
                check_buffer_equiv(&buf, &model, &mut rng, &epoch_ctx);
                iter += 1;
            }
            // Undo-all must restore the epoch's initial content exactly;
            // redo-all must restore the post-ops content exactly.
            let post_ops = buf.bytes();
            let mut undos = 0;
            while buf.undo() {
                undos += 1;
                assert!(undos < 1_000_000, "undo loop did not terminate");
            }
            assert_eq!(buf.bytes(), init.as_bytes(), "undo-all must restore epoch init");
            assert!(!buf.undo(), "undo on empty stack must return false");
            let mut redos = 0;
            while buf.redo() {
                redos += 1;
                assert!(redos < 1_000_000, "redo loop did not terminate");
            }
            assert_eq!(buf.bytes(), post_ops, "redo-all must restore post-ops content");
            assert!(!buf.redo(), "redo on empty stack must return false");
            assert_eq!(undos, redos, "undo/redo group counts must agree");
            check_buffer_equiv(&buf, &model, &mut rng, &format!("{epoch_ctx} after redo"));
        }
    }
}
