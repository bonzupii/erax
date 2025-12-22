//! Buffer: Pure data structure holding text content and metadata
//! No cursor, scrolling, or viewport state (those belong to Window)
//!
//! Simplified architecture: Uses ropey Rope for all files.
//! - Rope provides O(log n) line operations natively - no need for custom SumTree

use ropey::Rope;
use std::collections::VecDeque;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tempfile::NamedTempFile;

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

/// Represents an edit operation for undo/redo
/// Represents an edit operation for undo/redo
#[derive(Debug, Clone)]
pub enum Edit {
    /// Insert: (position, text inserted)
    Insert { pos: usize, text: Rope },
    /// Delete: (position, text deleted)
    Delete { pos: usize, text: Rope },
}

/// Buffer: Pure data structure holding text and metadata
/// No view state (cursor, scrolling) - that belongs to Window
#[derive(Debug)]
pub struct Buffer {
    /// Text content stored in a Rope (O(log n) operations)
    rope: Rope,
    /// Filename (if loaded from file)
    pub filename: Option<PathBuf>,
    /// The type of buffer
    pub buffer_kind: BufferKind,
    /// Dirty flag (true if buffer has unsaved changes)
    pub modified: bool,
    /// Last modification time of the file on disk
    pub last_modified_time: Option<SystemTime>,
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
}

impl Buffer {
    /// Create a new empty buffer
    pub fn new() -> Self {
        Self {
            rope: Rope::new(),
            filename: None,
            buffer_kind: BufferKind::Normal,
            modified: false,
            last_modified_time: None,
            version: 0,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            undo_grouper: UndoGrouper::new(),
            diagnostics: Vec::new(),
            syntax_state_cache: Vec::new(),
        }
    }

    /// Create a buffer from a string
    pub fn from_string(content: impl AsRef<str>) -> Self {
        Self {
            rope: Rope::from_str(content.as_ref()),
            filename: None,
            buffer_kind: BufferKind::Normal,
            modified: false,
            last_modified_time: None,
            version: 0,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            undo_grouper: UndoGrouper::new(),
            diagnostics: Vec::new(),
            syntax_state_cache: Vec::new(),
        }
    }

    /// Load a buffer from a file
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();
        let validated_path = Self::validate_file_path(path)?;

        let metadata = fs::metadata(&validated_path)
            .map_err(|e| format!("Failed to read file metadata: {}", e))?;
        let file_size = metadata.len();
        let modified_time = metadata.modified().ok();

        const HUGE_FILE_THRESHOLD: u64 = 500 * 1024 * 1024; // 500MB

        if file_size > HUGE_FILE_THRESHOLD {
            eprintln!(
                "Warning: File is very large ({:.1}MB). Loading may be slow.",
                file_size as f64 / (1024.0 * 1024.0)
            );
        }

        // Load file content using streaming reader for reduced memory usage.
        // Rope::from_reader streams the file directly into the rope structure,
        // avoiding the need to hold the entire file in RAM as an intermediate String.
        let file =
            fs::File::open(&validated_path).map_err(|e| format!("Failed to open file: {}", e))?;
        let reader = std::io::BufReader::new(file);

        // Try to load as valid UTF-8 first, fall back to lossy conversion if invalid
        let rope = match Rope::from_reader(reader) {
            Ok(r) => r,
            Err(_) => {
                // File contains invalid UTF-8 - use lossy conversion
                let bytes =
                    fs::read(&validated_path).map_err(|e| format!("Failed to read file: {}", e))?;
                let content = String::from_utf8_lossy(&bytes);
                Rope::from_str(&content)
            }
        };

        Ok(Self {
            rope,
            filename: Some(validated_path),
            buffer_kind: BufferKind::Normal,
            modified: false,
            last_modified_time: modified_time,
            version: 0,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            undo_grouper: UndoGrouper::new(),
            diagnostics: Vec::new(),
            syntax_state_cache: Vec::new(),
        })
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
        self.rope.len_bytes()
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    /// Get an iterator over lines starting from a specific line index.
    /// Returns RopeSlice, which is zero-copy.
    pub fn lines_at(&self, start_line: usize) -> impl Iterator<Item = ropey::RopeSlice<'_>> {
        self.rope.lines_at(start_line)
    }

    /// Get disjoint references to internal data for rendering without borrowing conflicts
    pub fn split_for_render(
        &mut self,
    ) -> (
        &Rope,
        &Vec<Diagnostic>,
        &mut Vec<SyntaxLexerState>,
        &Option<PathBuf>,
    ) {
        (
            &self.rope,
            &self.diagnostics,
            &mut self.syntax_state_cache,
            &self.filename,
        )
    }

    /// Get entire content as string
    pub fn to_string(&self) -> String {
        self.rope.to_string()
    }

    /// Get a range of text as string
    pub fn get_range_as_string(&self, start: usize, length: usize) -> String {
        if length == 0 {
            return String::new();
        }
        let len = self.rope.len_bytes();
        let actual_start = start.min(len);
        let actual_end = (actual_start + length).min(len);
        if actual_start >= actual_end {
            return String::new();
        }
        self.rope.byte_slice(actual_start..actual_end).to_string()
    }

    /// Get character at position
    pub fn char_at(&self, byte_pos: usize) -> Option<char> {
        if byte_pos >= self.rope.len_bytes() {
            return None;
        }
        let char_idx = self.rope.byte_to_char(byte_pos);
        self.rope.get_char(char_idx)
    }

    // ==================== Line Operations (O(log n) via Rope) ====================

    /// Get number of lines in buffer
    /// This is O(1) - ropey caches this
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// Get byte offset for start of a line
    /// This is O(log n) - uses ropey's B-tree
    pub fn line_to_byte(&self, line_idx: usize) -> Option<usize> {
        if line_idx >= self.rope.len_lines() {
            return None;
        }
        Some(self.rope.line_to_byte(line_idx))
    }

    /// Get which line a byte offset is on
    /// This is O(log n)
    pub fn byte_to_line(&self, byte_offset: usize) -> usize {
        if byte_offset >= self.rope.len_bytes() {
            return self.rope.len_lines().saturating_sub(1);
        }
        self.rope.byte_to_line(byte_offset)
    }

    /// Get content of a specific line (without newline)
    pub fn line(&self, line_idx: usize) -> Option<String> {
        if line_idx >= self.rope.len_lines() {
            return None;
        }
        let line = self.rope.line(line_idx);
        // Remove trailing newline if present
        let s = line.to_string();
        Some(s.trim_end_matches('\n').to_string())
    }

    /// Get content of a line with newline preserved
    pub fn line_with_newline(&self, line_idx: usize) -> Option<String> {
        if line_idx >= self.rope.len_lines() {
            return None;
        }
        Some(self.rope.line(line_idx).to_string())
    }

    /// Get length of a specific line in bytes (including newline)
    pub fn line_len(&self, line_idx: usize) -> Option<usize> {
        if line_idx >= self.rope.len_lines() {
            return None;
        }
        Some(self.rope.line(line_idx).len_bytes())
    }

    // ==================== Editing Operations ====================

    /// Insert text at byte position
    pub fn insert(&mut self, pos: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let pos = pos.min(self.rope.len_bytes());
        let char_idx = self.rope.byte_to_char(pos);
        self.rope.insert(char_idx, text);
        self.mark_modified();

        // Record for undo
        self.push_edit(Edit::Insert {
            pos,
            text: Rope::from_str(text),
        });
        self.redo_stack.clear();
    }

    /// Delete text at byte position
    pub fn delete(&mut self, pos: usize, len: usize) {
        if len == 0 || pos >= self.rope.len_bytes() {
            return;
        }
        let actual_len = len.min(self.rope.len_bytes() - pos);
        let start_char = self.rope.byte_to_char(pos);
        let end_char = self.rope.byte_to_char(pos + actual_len);

        // Save text for undo
        let deleted_text = Rope::from(self.rope.slice(start_char..end_char));

        self.rope.remove(start_char..end_char);
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
        let pos = pos.min(self.rope.len_bytes());
        let char_idx = self.rope.byte_to_char(pos);
        self.rope.insert_char(char_idx, ch);
        self.mark_modified();

        // Record for undo
        let mut text = Rope::new();
        text.insert_char(0, ch);
        self.push_edit(Edit::Insert { pos, text });
        self.redo_stack.clear();
    }

    /// Delete a single character (optimized path)
    pub fn delete_char(&mut self, pos: usize) {
        if pos >= self.rope.len_bytes() {
            return;
        }
        let char_idx = self.rope.byte_to_char(pos);
        if let Some(ch) = self.rope.get_char(char_idx) {
            self.rope.remove(char_idx..char_idx + 1);
            self.mark_modified();

            // Record for undo
            let mut text = Rope::new();
            text.insert_char(0, ch);
            self.push_edit(Edit::Delete { pos, text });
            self.redo_stack.clear();
        }
    }

    // ==================== Undo/Redo ====================

    /// Undo the last edit
    pub fn undo(&mut self) -> bool {
        if let Some(group) = self.undo_stack.pop_back() {
            // Apply edits in reverse order for undo
            for edit in group.edits.iter().rev() {
                match edit {
                    Edit::Insert { pos, text } => {
                        // Undo insert = delete
                        let start_char = self.rope.byte_to_char(*pos);
                        let end_char = self.rope.byte_to_char(*pos + text.len_bytes());
                        self.rope.remove(start_char..end_char);
                    }
                    Edit::Delete { pos, text } => {
                        // Undo delete = insert
                        let char_idx = self.rope.byte_to_char(*pos);
                        // Rope::insert needs check if we can insert Rope directly
                        // ropey 1.2+ has insert_rope? checking docs hypothesis...
                        // Assuming converting to string for now if not available, OR trying to use slice
                        // To be safe and since text is Rope, we iterate chunks

                        // Correct approach without allocating full string:
                        // Iterate chunks and insert them properly.
                        // Inserting chunk 1 at idx, then chunk 2 at idx + chunk1_len...
                        let mut current_idx = char_idx;
                        for chunk in text.chunks() {
                            self.rope.insert(current_idx, chunk);
                            current_idx += chunk.chars().count();
                        }
                    }
                }
            }
            self.redo_stack.push_back(group);
            self.mark_modified();
            true
        } else {
            false
        }
    }

    /// Redo the last undone edit
    pub fn redo(&mut self) -> bool {
        if let Some(group) = self.redo_stack.pop_back() {
            // Apply edits in original order for redo
            for edit in &group.edits {
                match edit {
                    Edit::Insert { pos, text } => {
                        let char_idx = self.rope.byte_to_char(*pos);
                        let mut current_idx = char_idx;
                        for chunk in text.chunks() {
                            self.rope.insert(current_idx, chunk);
                            current_idx += chunk.chars().count();
                        }
                    }
                    Edit::Delete { pos, text } => {
                        let start_char = self.rope.byte_to_char(*pos);
                        let end_char = self.rope.byte_to_char(*pos + text.len_bytes());
                        self.rope.remove(start_char..end_char);
                    }
                }
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
        let parent = path.parent().unwrap_or(Path::new("."));
        let mut temp_file = NamedTempFile::new_in(parent)?;

        // Write content chunk by chunk to avoid large allocations
        for chunk in self.rope.chunks() {
            temp_file.write_all(chunk.as_bytes())?;
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

    /// Get the filename as a string for display
    pub fn display_name(&self) -> String {
        self.filename
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "*scratch*".to_string())
    }

    /// Check if file has been modified externally
    pub fn check_external_modification(&self) -> bool {
        if let (Some(path), Some(stored_time)) = (&self.filename, &self.last_modified_time) {
            if let Ok(metadata) = fs::metadata(path) {
                if let Ok(current_time) = metadata.modified() {
                    return current_time != *stored_time;
                }
            }
        }
        false
    }

    /// Reload buffer from disk
    pub fn reload(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let path = self
            .filename
            .as_ref()
            .ok_or("No filename set for buffer")?
            .clone();

        use std::io::BufReader;
        let file = std::fs::File::open(&path)?;
        let reader = BufReader::new(file);
        self.rope = Rope::from_reader(reader)?;

        self.modified = false;
        self.last_modified_time = fs::metadata(&path).ok().and_then(|m| m.modified().ok());
        self.version += 1;
        self.undo_stack.clear();
        self.redo_stack.clear();

        Ok(())
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
    pub fn find_all(&self, pattern: &str) -> Vec<usize> {
        if pattern.is_empty() {
            return Vec::new();
        }

        let mut results = Vec::new();
        let mut byte_pos = 0;

        // We need to handle patterns that span chunk boundaries
        // Keep a sliding window of (pattern.len() - 1) bytes from previous chunk
        let overlap_size = pattern.len().saturating_sub(1);
        let mut overlap = Vec::with_capacity(overlap_size);

        for chunk in self.rope.chunks() {
            // Check overlap region from previous chunk + start of current
            if !overlap.is_empty() && chunk.len() >= pattern.len() - overlap.len() {
                let needed = pattern.len() - overlap.len();
                overlap.extend_from_slice(&chunk.as_bytes()[..needed.min(chunk.len())]);
                if let Ok(overlap_str) = std::str::from_utf8(&overlap) {
                    if overlap_str == pattern {
                        results.push(byte_pos - (pattern.len() - needed));
                    }
                }
            }

            // Search within current chunk
            let mut search_start = 0;
            while let Some(idx) = chunk[search_start..].find(pattern) {
                results.push(byte_pos + search_start + idx);
                search_start += idx + 1; // Move past to find next
            }

            // Save overlap for next chunk
            overlap.clear();
            if overlap_size > 0 && chunk.len() > 0 {
                let start = chunk.len().saturating_sub(overlap_size);
                overlap.extend_from_slice(&chunk.as_bytes()[start..]);
            }

            byte_pos += chunk.len();
        }

        results
    }

    /// Find next occurrence starting from position (streaming, O(pattern.len()) space)
    pub fn find_forward(&self, pattern: &str, start_pos: usize) -> Option<usize> {
        if pattern.is_empty() {
            return None;
        }

        let mut byte_pos = 0;
        let overlap_size = pattern.len().saturating_sub(1);
        let mut overlap = Vec::with_capacity(overlap_size);

        for chunk in self.rope.chunks() {
            let chunk_end = byte_pos + chunk.len();

            // Check overlap region (if we're past start_pos)
            if !overlap.is_empty() && byte_pos > start_pos.saturating_sub(pattern.len()) {
                let needed = pattern.len() - overlap.len();
                if chunk.len() >= needed {
                    overlap.extend_from_slice(&chunk.as_bytes()[..needed]);
                    if let Ok(overlap_str) = std::str::from_utf8(&overlap) {
                        if overlap_str == pattern {
                            let found_pos = byte_pos - (pattern.len() - needed);
                            if found_pos >= start_pos {
                                return Some(found_pos);
                            }
                        }
                    }
                }
            }

            // Search within this chunk if it contains positions >= start_pos
            if chunk_end > start_pos {
                let search_start = if byte_pos >= start_pos {
                    0
                } else {
                    start_pos - byte_pos
                };

                if search_start < chunk.len() {
                    if let Some(idx) = chunk[search_start..].find(pattern) {
                        return Some(byte_pos + search_start + idx);
                    }
                }
            }

            // Save overlap
            overlap.clear();
            if overlap_size > 0 && chunk.len() > 0 {
                let start = chunk.len().saturating_sub(overlap_size);
                overlap.extend_from_slice(&chunk.as_bytes()[start..]);
            }

            byte_pos = chunk_end;
        }

        None
    }

    /// Find previous occurrence before position (streaming backward)
    pub fn find_backward(&self, pattern: &str, start_pos: usize) -> Option<usize> {
        if pattern.is_empty() {
            return None;
        }

        // For backward search, we collect all matches up to start_pos and take the last one
        // This is still O(n) time but O(1) space for each match found
        let mut last_match: Option<usize> = None;
        let mut byte_pos = 0;
        let overlap_size = pattern.len().saturating_sub(1);
        let mut overlap = Vec::with_capacity(overlap_size);

        for chunk in self.rope.chunks() {
            // Stop if we're past the search range
            if byte_pos >= start_pos {
                break;
            }

            // Check overlap region
            if !overlap.is_empty() {
                let needed = pattern.len() - overlap.len();
                if chunk.len() >= needed {
                    overlap.extend_from_slice(&chunk.as_bytes()[..needed]);
                    if let Ok(overlap_str) = std::str::from_utf8(&overlap) {
                        if overlap_str == pattern {
                            let found_pos = byte_pos - (pattern.len() - needed);
                            if found_pos < start_pos {
                                last_match = Some(found_pos);
                            }
                        }
                    }
                }
            }

            // Search within chunk
            let search_end = (start_pos.saturating_sub(byte_pos)).min(chunk.len());
            let mut search_start = 0;
            while search_start < search_end {
                if let Some(idx) = chunk[search_start..search_end].find(pattern) {
                    let found_pos = byte_pos + search_start + idx;
                    if found_pos < start_pos {
                        last_match = Some(found_pos);
                    }
                    search_start += idx + 1;
                } else {
                    break;
                }
            }

            // Save overlap
            overlap.clear();
            if overlap_size > 0 && chunk.len() > 0 {
                let start = chunk.len().saturating_sub(overlap_size);
                overlap.extend_from_slice(&chunk.as_bytes()[start..]);
            }

            byte_pos += chunk.len();
        }

        last_match
    }

    /// Replace all occurrences (streaming - no full buffer allocation)
    pub fn replace_all(&mut self, pattern: &str, replacement: &str) -> usize {
        if pattern.is_empty() {
            return 0;
        }

        // Find all match positions using streaming search
        let matches = self.find_all(pattern);
        if matches.is_empty() {
            return 0;
        }

        // Apply replacements in reverse order to keep byte positions valid
        for &pos in matches.iter().rev() {
            let char_start = self.rope.byte_to_char(pos);
            let char_end = self.rope.byte_to_char(pos + pattern.len());
            self.rope.remove(char_start..char_end);
            self.rope.insert(char_start, replacement);
        }

        self.mark_modified();

        // Record single undo group for entire replace-all operation
        self.push_edit(Edit::Insert {
            pos: 0,
            text: Rope::from_str(&format!("[replace-all: {} matches]", matches.len())),
        });
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
    /// Legacy: Access rope directly (for code migrating from BufferBackend)
    pub fn backend_len(&self) -> usize {
        self.len()
    }

    /// Legacy: Check if backend is empty
    pub fn backend_is_empty(&self) -> bool {
        self.is_empty()
    }

    /// Legacy: Get content as string (same as to_string)
    pub fn backend_to_string(&self) -> String {
        self.to_string()
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
        assert_eq!(buf.to_string(), "Hello, World!");
        assert!(buf.modified);

        buf.delete(0, 7);
        assert_eq!(buf.to_string(), "World!");
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
        assert_eq!(buf.to_string(), "Hello");

        buf.undo();
        assert_eq!(buf.to_string(), "");

        buf.redo();
        assert_eq!(buf.to_string(), "Hello");
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
        assert_eq!(buf.to_string(), "hello world");

        // Single undo removes all (batched together)
        buf.undo();
        assert_eq!(buf.to_string(), "");
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

        // Load the file - should succeed with lossy conversion
        let result = Buffer::from_file(&test_file);

        // Clean up
        let _ = std::fs::remove_file(&test_file);

        // Verify it loaded successfully
        assert!(
            result.is_ok(),
            "Buffer::from_file should handle invalid UTF-8"
        );
        let buffer = result.unwrap();

        // Invalid bytes should be replaced with replacement character(s)
        let content = buffer.to_string();
        assert!(
            content.contains("Hello"),
            "Should contain valid text before invalid bytes"
        );
        assert!(
            content.contains("World"),
            "Should contain valid text after invalid bytes"
        );
    }
}
