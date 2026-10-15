//! Hex viewer: xxd/hexdump-style rendering over raw bytes.
//!
//! [`HexView`] renders any byte slice as classic `offset: hex-bytes  ascii`
//! rows. Rendering is pure ([`render_row`] needs no terminal), so the layout
//! is fully unit-testable.
//!
//! Per DATA-STRUCTURE.md decision C, the view is **read-only by default**:
//! [`HexView::request_edit`] returns a [`HexEditWarning`] until the caller
//! explicitly proceeds via [`HexView::unlock_for_edit`] — the same
//! read-only-vs-proceed pattern family as N-143.
//!
//! The view never mutates anything itself. [`HexView::byte_offset`] maps a
//! (row, column) cell to a byte offset into the viewed bytes; the caller
//! applies edits through the normal [`Buffer`](crate::core::buffer::Buffer)
//! edit machinery. There is deliberately no parallel edit path.

use std::fmt;

use crate::core::buffer::Buffer;

/// Default bytes per rendered row (classic 16-byte xxd layout).
pub const BYTES_PER_ROW: usize = 16;

/// Upper bound for configurable bytes-per-row. Generous on purpose: the
/// layout math is O(bytes-per-row) either way, this just rejects nonsense.
pub const MAX_BYTES_PER_ROW: usize = 256;

/// Error from [`HexView::with_bytes_per_row`] for out-of-range widths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HexConfigError {
    value: usize,
    message: String,
}

impl HexConfigError {
    /// The rejected value.
    pub fn value(&self) -> usize {
        self.value
    }

    /// Human-readable reason.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for HexConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for HexConfigError {}

/// Warning returned by [`HexView::request_edit`] while the view is locked.
///
/// The UI must show [`HexEditWarning::message`] and offer an explicit
/// proceed action (which calls [`HexView::unlock_for_edit`]) before any
/// hex editing is allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HexEditWarning {
    message: String,
}

impl HexEditWarning {
    /// The warning text the UI should present.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for HexEditWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// One row of the hex view: a file offset plus the bytes on that row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HexRow<'a> {
    /// Byte offset of the first byte in this row.
    pub offset: usize,
    /// The bytes on this row (at most `bytes_per_row`).
    pub bytes: &'a [u8],
}

/// Render one row as `00000000: 48 65 6c 6c 6f  |Hello|`.
///
/// Pure function — no terminal needed. `bytes` longer than `bytes_per_row`
/// are truncated; a short final row is padded with blanks so the ASCII
/// column always lines up. Non-printable bytes render as `.` in the ASCII
/// column. The offset is zero-padded to 8 hex digits and widens naturally
/// for huge files.
pub fn render_row(offset: usize, bytes: &[u8], bytes_per_row: usize) -> String {
    let bytes_per_row = bytes_per_row.max(1);
    let bytes = &bytes[..bytes.len().min(bytes_per_row)];
    // Hex field width is fixed for a given bytes_per_row, so rows align.
    let mut out = String::with_capacity(10 + bytes_per_row * 3 + bytes_per_row + 4);
    out.push_str(&format!("{offset:08x}"));
    out.push_str(": ");
    for i in 0..bytes_per_row {
        if let Some(&b) = bytes.get(i) {
            out.push_str(&format!("{b:02x} "));
        } else {
            out.push_str("   ");
        }
        // Classic hexdump -C group gap after the 8th byte.
        if i == 7 && bytes_per_row > 8 {
            out.push(' ');
        }
    }
    out.push_str(" |");
    for &b in bytes {
        out.push(if b.is_ascii_graphic() || b == b' ' {
            b as char
        } else {
            '.'
        });
    }
    out.push('|');
    out
}

/// A read-only-by-default hex view over a byte slice.
///
/// Construct with [`HexView::new`] (zero-copy over `&[u8]`) or
/// [`HexView::of_buffer_raw`] for a buffer's preserved raw bytes. Locked
/// until [`HexView::unlock_for_edit`] is called after the user acknowledges
/// the [`HexEditWarning`] from [`HexView::request_edit`].
#[derive(Debug, Clone, Copy)]
pub struct HexView<'a> {
    bytes: &'a [u8],
    bytes_per_row: usize,
    edit_unlocked: bool,
}

impl<'a> HexView<'a> {
    /// Zero-copy view over `bytes`. Read-only (locked) by default.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            bytes_per_row: BYTES_PER_ROW,
            edit_unlocked: false,
        }
    }

    /// View with a custom row width. Rejects 0 and widths above
    /// [`MAX_BYTES_PER_ROW`].
    pub fn with_bytes_per_row(
        bytes: &'a [u8],
        bytes_per_row: usize,
    ) -> Result<Self, HexConfigError> {
        if bytes_per_row == 0 || bytes_per_row > MAX_BYTES_PER_ROW {
            return Err(HexConfigError {
                value: bytes_per_row,
                message: format!(
                    "bytes-per-row must be between 1 and {MAX_BYTES_PER_ROW}, got {bytes_per_row}"
                ),
            });
        }
        Ok(Self {
            bytes,
            bytes_per_row,
            edit_unlocked: false,
        })
    }

    /// View over a buffer's preserved raw bytes (binary / non-UTF-8 files
    /// loaded via the encoding-aware load path).
    ///
    /// Returns `None` for plain text buffers, which keep no separate raw
    /// byte store — for those, `HexView::new(buffer.content_string().as_bytes())`
    /// views the identical bytes (the caller holds the `String`).
    pub fn of_buffer_raw(buffer: &'a Buffer) -> Option<Self> {
        buffer.original_bytes.as_deref().map(Self::new)
    }

    /// The viewed bytes.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Number of viewed bytes.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// True when there are no bytes to show.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Bytes rendered per row.
    pub fn bytes_per_row(&self) -> usize {
        self.bytes_per_row
    }

    /// Number of rows (`ceil(len / bytes_per_row)`; 0 for empty input).
    pub fn row_count(&self) -> usize {
        self.bytes.len().div_ceil(self.bytes_per_row)
    }

    /// The `index`-th row, or `None` when out of range.
    pub fn row(&self, index: usize) -> Option<HexRow<'a>> {
        let start = index.checked_mul(self.bytes_per_row)?;
        if start >= self.bytes.len() {
            return None;
        }
        let end = (start + self.bytes_per_row).min(self.bytes.len());
        Some(HexRow {
            offset: start,
            bytes: &self.bytes[start..end],
        })
    }

    /// Render the whole view, one row per line with a trailing newline.
    pub fn render(&self) -> String {
        self.render_rows(0, self.row_count())
    }

    /// Render `row_count` rows starting at `start_row` (clamped to the view).
    pub fn render_rows(&self, start_row: usize, row_count: usize) -> String {
        let end_row = (start_row + row_count).min(self.row_count());
        let mut out = String::new();
        for index in start_row..end_row {
            // `index < row_count` is guaranteed by the clamp above.
            if let Some(row) = self.row(index) {
                out.push_str(&render_row(row.offset, row.bytes, self.bytes_per_row));
                out.push('\n');
            }
        }
        out
    }

    /// Render a single row, or `None` when out of range.
    pub fn render_row(&self, index: usize) -> Option<String> {
        self.row(index)
            .map(|row| render_row(row.offset, row.bytes, self.bytes_per_row))
    }

    /// Map a (row, column) cell to the byte offset into the viewed bytes.
    ///
    /// The caller applies edits at that offset through the normal buffer
    /// edit machinery — the view itself never mutates. Offsets line up with
    /// [`Buffer`] byte positions only when the viewed bytes *are* the
    /// buffer's authoritative bytes (e.g. a UTF-8 text buffer viewed via
    /// `content_string().as_bytes()`); for lossy views over preserved raw
    /// bytes the offsets address the raw file bytes, and applying them to
    /// the buffer awaits the byte-native buffer (see module docs).
    pub fn byte_offset(&self, row: usize, col: usize) -> Option<usize> {
        if col >= self.bytes_per_row {
            return None;
        }
        let offset = row.checked_mul(self.bytes_per_row)?.checked_add(col)?;
        if offset < self.bytes.len() {
            Some(offset)
        } else {
            None
        }
    }

    // ---- Edit gate (decision C: read-only by default, warn before editing) ----

    /// True until [`HexView::unlock_for_edit`] is called.
    pub fn is_edit_locked(&self) -> bool {
        !self.edit_unlocked
    }

    /// Editing gate: `Ok(())` when unlocked, otherwise the [`HexEditWarning`]
    /// the UI must present before offering an explicit proceed action.
    pub fn request_edit(&self) -> Result<(), HexEditWarning> {
        if self.edit_unlocked {
            Ok(())
        } else {
            Err(HexEditWarning {
                message: format!(
                    "Hex view is read-only by default. Editing raw bytes bypasses \
                     text structure (line endings, encoding, syntax) and can silently \
                     corrupt the file: one wrong nibble can invalidate a multi-byte \
                     sequence or shift every offset after it. Edits go through the \
                     normal buffer machinery with full undo. Proceed only if you \
                     understand the byte layout ({} bytes).",
                    self.bytes.len()
                ),
            })
        }
    }

    /// Explicit proceed: acknowledge the warning and unlock editing.
    /// Idempotent; [`HexView::lock_edits`] re-locks.
    pub fn unlock_for_edit(&mut self) {
        self.edit_unlocked = true;
    }

    /// Re-lock the view (read-only again).
    pub fn lock_edits(&mut self) {
        self.edit_unlocked = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_row_canonical_full_row() {
        let got = render_row(0, b"0123456789abcdef", 16);
        assert_eq!(
            got,
            "00000000: 30 31 32 33 34 35 36 37  38 39 61 62 63 64 65 66  |0123456789abcdef|"
        );
    }

    #[test]
    fn render_row_short_final_row_pads_ascii_column() {
        let got = render_row(0, b"Hello", 16);
        // ASCII column starts at the same byte index as in a full row
        // (total lengths differ: 5 vs 16 ASCII chars).
        let full = render_row(0, b"0123456789abcdef", 16);
        assert_eq!(got.find('|'), full.find('|'));
        assert!(got.starts_with("00000000: 48 65 6c 6c 6f "));
        assert!(got.ends_with("|Hello|"));
        // The padding is exactly what a full row would have: 11 missing
        // bytes * 3 spaces + 1 group gap + the space before '|'.
        assert_eq!(got.find("|Hello|"), full.find("|0123456789abcdef|"));
    }

    #[test]
    fn render_row_nonprintable_becomes_dots() {
        let got = render_row(0, b"\x00\x01\x7f\x80\xffABC \t", 16);
        assert!(got.ends_with("|.....ABC .|"), "got: {got}");
        // Note the double space after 43: the hexdump -C group gap.
        assert!(got.starts_with("00000000: 00 01 7f 80 ff 41 42 43  20 09 "));
    }

    #[test]
    fn render_row_offset_and_widths() {
        // Nonzero offset.
        let got = render_row(0x10, b"ABCDEFGH", 8);
        assert_eq!(got, "00000010: 41 42 43 44 45 46 47 48  |ABCDEFGH|");
        // Huge offsets widen past 8 digits instead of truncating.
        let got = render_row(0x1_0000_0000, b"A", 16);
        assert!(got.starts_with("100000000: 41 "), "got: {got}");
        // Oversized byte slice is truncated to bytes_per_row, never panics.
        let got = render_row(0, b"0123456789abcdefEXTRA", 16);
        assert!(got.ends_with("|0123456789abcdef|"), "got: {got}");
        // Empty row still renders a well-formed line.
        let got = render_row(0x20, b"", 16);
        assert_eq!(
            got,
            "00000020:                                                   ||"
        );
    }

    #[test]
    fn view_row_counts_and_access() {
        let bytes: Vec<u8> = (0..40).collect();
        let view = HexView::new(&bytes);
        assert_eq!(view.len(), 40);
        assert!(!view.is_empty());
        assert_eq!(view.bytes_per_row(), BYTES_PER_ROW);
        assert_eq!(view.row_count(), 3); // 16 + 16 + 8

        let row = view.row(0).expect("row 0");
        assert_eq!(row.offset, 0);
        assert_eq!(row.bytes.len(), 16);
        let last = view.row(2).expect("row 2");
        assert_eq!(last.offset, 32);
        assert_eq!(last.bytes.len(), 8);
        assert_eq!(view.row(3), None);

        // Empty input: zero rows, renders to empty string.
        let empty = HexView::new(b"");
        assert!(empty.is_empty());
        assert_eq!(empty.row_count(), 0);
        assert_eq!(empty.render(), "");
    }

    #[test]
    fn view_render_whole_and_windowed() {
        let bytes: Vec<u8> = (0..32).collect();
        let view = HexView::new(&bytes);
        let whole = view.render();
        assert_eq!(whole.lines().count(), 2);
        assert!(whole.starts_with("00000000: "));
        assert!(whole.contains("00000010: "));

        let window = view.render_rows(1, 1);
        assert_eq!(window.lines().count(), 1);
        assert!(window.starts_with("00000010: "));

        // Out-of-range windows clamp instead of panicking.
        assert_eq!(view.render_rows(99, 5), "");
        assert_eq!(view.render_rows(1, 99).lines().count(), 1);

        // Single-row render matches the free function.
        assert_eq!(
            view.render_row(0).as_deref(),
            Some(render_row(0, &bytes[0..16], 16).as_str())
        );
        assert_eq!(view.render_row(7), None);
    }

    #[test]
    fn custom_bytes_per_row() {
        let bytes: Vec<u8> = (0..20).collect();
        let view = HexView::with_bytes_per_row(&bytes, 8).expect("valid width");
        assert_eq!(view.row_count(), 3);
        let first = view.render_row(0).expect("row");
        assert_eq!(first, "00000000: 00 01 02 03 04 05 06 07  |........|");

        assert!(HexView::with_bytes_per_row(&bytes, 0).is_err());
        let err = HexView::with_bytes_per_row(&bytes, MAX_BYTES_PER_ROW + 1).unwrap_err();
        assert_eq!(err.value(), MAX_BYTES_PER_ROW + 1);
        assert!(!err.message().is_empty());
    }

    #[test]
    fn byte_offset_mapping() {
        let bytes: Vec<u8> = (0..40).collect();
        let view = HexView::new(&bytes);
        assert_eq!(view.byte_offset(0, 0), Some(0));
        assert_eq!(view.byte_offset(0, 15), Some(15));
        assert_eq!(view.byte_offset(1, 0), Some(16));
        assert_eq!(view.byte_offset(2, 7), Some(39));
        // Past the end of the data or the row: None, never a panic.
        assert_eq!(view.byte_offset(2, 8), None);
        assert_eq!(view.byte_offset(3, 0), None);
        assert_eq!(view.byte_offset(0, 16), None);
        assert_eq!(view.byte_offset(usize::MAX, 0), None);
    }

    #[test]
    fn edit_gate_locks_by_default_warns_then_unlocks() {
        let bytes = b"binary data here";
        let mut view = HexView::new(bytes);
        assert!(view.is_edit_locked());

        let err = view.request_edit().expect_err("must warn while locked");
        let msg = err.message();
        assert!(msg.contains("read-only"), "message: {msg}");
        assert!(msg.contains("undo"), "message: {msg}");
        assert_eq!(err.to_string(), msg);

        // Explicit proceed unlocks; re-locking restores the warning.
        view.unlock_for_edit();
        assert!(!view.is_edit_locked());
        assert!(view.request_edit().is_ok());
        view.unlock_for_edit(); // idempotent
        assert!(view.request_edit().is_ok());
        view.lock_edits();
        assert!(view.is_edit_locked());
        assert!(view.request_edit().is_err());
    }

    #[test]
    fn of_buffer_raw_views_preserved_bytes_byte_exact() {
        // Binary buffer: original_bytes preserved by the encoding load path.
        let raw: Vec<u8> = (0..=255u8).collect();
        let mut buffer = Buffer::from_string(String::from_utf8_lossy(&raw));
        buffer.original_bytes = Some(raw.clone());

        let view = HexView::of_buffer_raw(&buffer).expect("raw bytes present");
        assert_eq!(view.bytes(), raw.as_slice());
        assert_eq!(view.len(), 256);
        assert_eq!(view.row_count(), 16);
        // Spot-check: byte 0x41 ('A') renders in the ASCII column of row 4.
        let row4 = view.render_row(4).expect("row 4");
        assert!(row4.starts_with("00000040: "));
        assert!(row4.contains("|@ABCDEFGHIJKLMNO|"), "row4: {row4}");

        // Plain text buffer: no raw store -> None (caller uses content_string).
        let plain = Buffer::from_string("hello");
        assert!(HexView::of_buffer_raw(&plain).is_none());
        let text = plain.content_string();
        let tview = HexView::new(text.as_bytes());
        assert_eq!(tview.len(), 5);
    }

    #[test]
    fn fuzz_render_never_panics() {
        // Adversarial sweep: every length 0..=300, every width 1..=17,
        // hostile byte values at every position.
        for len in 0..=300usize {
            let mut bytes = Vec::with_capacity(len);
            for i in 0..len {
                bytes.push(((i * 2654435761) >> 17) as u8 ^ 0xFF);
            }
            for width in [1usize, 7, 8, 9, 15, 16, 17] {
                let view = HexView::with_bytes_per_row(&bytes, width).expect("width");
                let rendered = view.render();
                assert_eq!(rendered.lines().count(), view.row_count());
                for (r, line) in rendered.lines().enumerate() {
                    assert!(
                        line.starts_with(&format!("{:08x}: ", r * width)),
                        "row {r} width {width}: {line}"
                    );
                }
                for row in 0..view.row_count() {
                    for col in 0..width {
                        let _ = view.byte_offset(row, col);
                    }
                }
            }
        }
    }
}
