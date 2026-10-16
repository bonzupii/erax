//! Document semantics: the typed annotation model and the snapshot API.
//!
//! This is the layer the owner's metadata direction puts in the core:
//! document meaning (token classes, diagnostics, selection, marks)
//! annotated next to the text, independent of any presentation. The
//! screen model's grid is presentation derived from these semantics;
//! headless consumers (screen readers, voice control, the future
//! assistant) read this module, never pixels.
//!
//! Annotation maintenance, per kind:
//!
//! - Diagnostics, selection, and marks are DERIVED at snapshot time
//!   from their authoritative state (the buffer's diagnostic list,
//!   the window's selection manager and mark). Nothing is maintained
//!   incrementally, so a snapshot can never show a stale annotation:
//!   whatever the source of truth says is what the snapshot carries.
//! - Token classes are the plug-in seam for the future generic
//!   highlighter: the buffer owns a `token_annotations` store the
//!   highlighter will replace wholesale, and the snapshot maps the
//!   store without knowing who filled it. The store is empty until
//!   the highlighter exists; the mapping loop is the API, not dead
//!   code.
//! - Search matches have a reserved kind and an empty mapping loop:
//!   no search-match tracking exists in the tree yet, so there is no
//!   source to derive from. When tracking lands, it populates the
//!   loop and the kind is already in the API.

use crate::core::bindings::{InputModel, ModalMode, ModalOperator};
use crate::core::app::EditorApp;
use crate::core::buffer::Buffer;
use crate::core::diagnostics::DiagnosticSeverity;
use crate::core::id::{BufferId, WindowId};

/// A byte range in a buffer. All annotation ranges are byte-based:
/// byte offsets are the tree's native addressing, so ranges stay
/// meaningful for binary-as-text buffers too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// First byte of the range, inclusive.
    pub start: usize,
    /// One past the last byte of the range.
    pub end: usize,
}

impl ByteRange {
    /// Length of the range in bytes (0 for a point).
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether the range covers no bytes.
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// Token classes for syntax highlighting. The generic highlighter
/// (a later phase) populates these; the enum exists now so the
/// snapshot API, the cue/announcement text, and the future
/// highlighter share one vocabulary with no API change later.
/// Classes are language-agnostic by design: no per-language variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenClass {
    /// Language keywords.
    Keyword,
    /// String and character literals.
    String,
    /// Comments.
    Comment,
    /// Numeric literals.
    Number,
    /// Type names.
    Type,
    /// Function and method names.
    Function,
    /// Operators and punctuation carrying meaning.
    Operator,
    /// Identifiers that are none of the above.
    Identifier,
}

/// One token annotation in the buffer's token store. The future
/// highlighter replaces the whole store via
/// [`Buffer::set_token_annotations`]; the snapshot maps whatever is
/// there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenAnnotation {
    /// Byte range the token covers.
    pub range: ByteRange,
    /// The token's class.
    pub class: TokenClass,
}

/// What an annotation means. Diagnostic annotations carry their
/// severity; token annotations carry the future highlighter's class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationKind {
    /// A syntax token (populated by the future highlighter).
    Token(TokenClass),
    /// A diagnostic span (error, warning, note, info).
    Diagnostic(DiagnosticSeverity),
    /// The active selection.
    Selection,
    /// The mark position (a point).
    Mark,
    /// A search match (reserved: no tracking exists yet).
    SearchMatch,
}

/// One annotated range on a buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    /// Byte range the annotation covers.
    pub range: ByteRange,
    /// What the range means.
    pub kind: AnnotationKind,
    /// Human-readable detail (the diagnostic message for
    /// [`AnnotationKind::Diagnostic`); `None` otherwise.
    pub detail: Option<String>,
}

/// The editor's input-model state as the snapshot reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeView {
    /// Emacs-style keytrie model.
    Emacs,
    /// CUA standard model.
    Standard,
    /// Vim modal model with its substate.
    Vim {
        /// The modal mode.
        mode: ModalMode,
        /// Pending count (`3` in `3dw`); 0 means none.
        count: usize,
        /// Pending operator (`d` in `dw`); meaningful in
        /// `OperatorPending`, otherwise `None`.
        operator: Option<ModalOperator>,
    },
}

/// The cursor as the snapshot reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorView {
    /// Cursor line, 0-based.
    pub line: usize,
    /// Cursor column in grapheme clusters, 0-based.
    pub column: usize,
    /// Cursor byte offset in the buffer.
    pub byte: usize,
}

/// One window's summary for the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowView {
    /// Window identity.
    pub id: WindowId,
    /// Buffer the window shows.
    pub buffer_id: BufferId,
    /// Cursor line in the window, 0-based.
    pub cursor_line: usize,
}

/// One buffer's summary for the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferView {
    /// Buffer identity.
    pub id: BufferId,
    /// Display name (filename or `*scratch*`).
    pub name: String,
    /// Whether the buffer holds unsaved edits.
    pub modified: bool,
}

/// Diagnostic counts by severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiagnosticSummary {
    /// Error-severity diagnostics.
    pub errors: usize,
    /// Warning-severity diagnostics.
    pub warnings: usize,
    /// Note-severity diagnostics.
    pub notes: usize,
    /// Info-severity diagnostics.
    pub infos: usize,
}

/// The full document snapshot: everything a headless consumer needs
/// about the active buffer, in one typed value. This is THE surface
/// for screen readers, voice control, and the future assistant, not
/// an accessibility side door.
#[derive(Debug, Clone)]
pub struct DocumentSnapshot<'a> {
    /// The active buffer itself: text access by range goes through its
    /// normal accessors (e.g. [`Buffer::get_range_as_string`]).
    pub buffer: &'a Buffer,
    /// Active buffer identity.
    pub buffer_id: BufferId,
    /// Display name (filename or `*scratch*`).
    pub buffer_name: String,
    /// Whether the buffer holds unsaved edits.
    pub modified: bool,
    /// Line count of the buffer.
    pub line_count: usize,
    /// Cursor state in the active window.
    pub cursor: CursorView,
    /// Active selection as a byte range (`None` when empty or absent).
    pub selection: Option<ByteRange>,
    /// Mark position as a byte offset (`None` when unset).
    pub mark: Option<usize>,
    /// All annotations on the active buffer, sorted by range.
    pub annotations: Vec<Annotation>,
    /// Input-model state, including vim substate.
    pub mode: ModeView,
    /// The current echo-area message, if any.
    pub message: Option<String>,
    /// Diagnostic counts on the active buffer.
    pub diagnostics: DiagnosticSummary,
    /// All windows, active first is NOT guaranteed; match by id.
    pub windows: Vec<WindowView>,
    /// All buffers.
    pub buffers: Vec<BufferView>,
}

/// The cheap projection of a snapshot used for change detection. The
/// web server diffs this per frame; building the full annotation vec
/// every frame would cost O(buffer) per frame, so the summary carries
/// only what announcements compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotSummary {
    /// Active buffer identity.
    pub buffer_id: BufferId,
    /// Display name (filename or `*scratch*`).
    pub buffer_name: String,
    /// Input-model state, including vim substate.
    pub mode: ModeView,
    /// The current echo-area message, if any.
    pub message: Option<String>,
    /// Cursor line, 0-based.
    pub cursor_line: usize,
    /// Selected bytes (0 when the selection is empty or absent).
    pub selection_bytes: usize,
    /// Error-severity diagnostics on the active buffer.
    pub errors: usize,
    /// Warning-severity diagnostics on the active buffer.
    pub warnings: usize,
}

/// Capture the full snapshot of the active buffer. Returns `None`
/// when there is no active window or buffer (not reachable in a live
/// editor; the option keeps the API honest instead of panicking).
pub fn capture(app: &EditorApp) -> Option<DocumentSnapshot<'_>> {
    let window = app.windows.get(&app.active_window)?;
    let buffer = app.buffers.get(&window.buffer_id)?;
    let buffer_id = window.buffer_id;

    let byte = window.get_byte_offset(buffer).unwrap_or(0);
    let cursor = CursorView {
        line: window.cursor_y,
        column: window.cursor_x,
        byte,
    };

    let selection = window
        .selection_manager
        .get_selection()
        .filter(|s| !s.is_empty())
        .map(|s| ByteRange {
            start: s.start(),
            end: s.end(),
        });

    let mark = window
        .mark
        .map(|(x, y)| window.get_byte_offset_at(buffer, x, y).unwrap_or(0));

    let mut annotations = Vec::new();
    // Diagnostics: derived from the buffer's diagnostic list at
    // snapshot time. The list is authoritative, so the ranges track
    // edits with no incremental bookkeeping.
    for diag in &buffer.diagnostics {
        if diag.line == 0 {
            continue;
        }
        let line_idx = diag.line - 1;
        let Some(start) = buffer.line_to_byte(line_idx) else {
            continue;
        };
        let end = buffer.line_to_byte(line_idx + 1).unwrap_or_else(|| buffer.len());
        annotations.push(Annotation {
            range: ByteRange { start, end },
            kind: AnnotationKind::Diagnostic(diag.severity),
            detail: Some(diag.message.clone()),
        });
    }
    // Selection and mark: derived from the window's live state.
    if let Some(range) = selection {
        annotations.push(Annotation {
            range,
            kind: AnnotationKind::Selection,
            detail: None,
        });
    }
    if let Some(at) = mark {
        annotations.push(Annotation {
            range: ByteRange { start: at, end: at },
            kind: AnnotationKind::Mark,
            detail: None,
        });
    }
    // Token classes: mapped from the buffer's token store, whatever
    // the future highlighter put there (empty until it exists).
    for token in &buffer.token_annotations {
        annotations.push(Annotation {
            range: token.range,
            kind: AnnotationKind::Token(token.class),
            detail: None,
        });
    }
    annotations.sort_by_key(|a| (a.range.start, a.range.end));

    let windows = app
        .windows
        .iter()
        .map(|(&id, w)| WindowView {
            id,
            buffer_id: w.buffer_id,
            cursor_line: w.cursor_y,
        })
        .collect();
    let buffers = app
        .buffers
        .iter()
        .map(|(&id, b)| BufferView {
            id,
            name: b.display_name(),
            modified: b.modified,
        })
        .collect();

    Some(DocumentSnapshot {
        buffer,
        buffer_id,
        buffer_name: buffer.display_name(),
        modified: buffer.modified,
        line_count: buffer.line_count(),
        cursor,
        selection,
        mark,
        annotations,
        mode: mode_view(app),
        message: app.message.clone(),
        diagnostics: diagnostic_summary(buffer),
        windows,
        buffers,
    })
}

/// The input-model state as a [`ModeView`]: the same mapping
/// [`capture`] uses, shared so the summary path does not duplicate it.
fn mode_view(app: &EditorApp) -> ModeView {
    match crate::config::keys::INPUT_MODEL {
        InputModel::KeyTrie => ModeView::Emacs,
        InputModel::Standard => ModeView::Standard,
        InputModel::Modal => ModeView::Vim {
            mode: app.modal_engine.mode(),
            count: app.modal_engine.pending_count(),
            operator: app.modal_engine.pending_operator(),
        },
    }
}

/// Count diagnostics by severity: the same tally [`capture`] uses,
/// shared so the summary path does not duplicate it.
fn diagnostic_summary(buffer: &Buffer) -> DiagnosticSummary {
    let mut diagnostics = DiagnosticSummary::default();
    for diag in &buffer.diagnostics {
        match diag.severity {
            DiagnosticSeverity::Error => diagnostics.errors += 1,
            DiagnosticSeverity::Warning => diagnostics.warnings += 1,
            DiagnosticSeverity::Note => diagnostics.notes += 1,
            DiagnosticSeverity::Info => diagnostics.infos += 1,
        }
    }
    diagnostics
}

/// Capture the cheap change-detection summary of the active buffer.
/// `None` under the same conditions as [`capture`].
///
/// The summary is built directly from the pieces announcements
/// compare: it never constructs the annotation vec (which costs a
/// clone and a sort per diagnostic). The websocket loop calls this
/// every iteration, so the projection stays O(diagnostics), not
/// O(buffer).
pub fn capture_summary(app: &EditorApp) -> Option<SnapshotSummary> {
    let window = app.windows.get(&app.active_window)?;
    let buffer = app.buffers.get(&window.buffer_id)?;
    let diagnostics = diagnostic_summary(buffer);
    let selection_bytes = window
        .selection_manager
        .get_selection()
        .filter(|s| !s.is_empty())
        .map(|s| s.end() - s.start())
        .unwrap_or(0);
    Some(SnapshotSummary {
        buffer_id: window.buffer_id,
        buffer_name: buffer.display_name(),
        mode: mode_view(app),
        message: app.message.clone(),
        cursor_line: window.cursor_y,
        selection_bytes,
        errors: diagnostics.errors,
        warnings: diagnostics.warnings,
    })
}

/// Describe the input-model state in words for announcements.
fn mode_phrase(mode: &ModeView) -> &'static str {
    match mode {
        ModeView::Emacs => "Emacs mode",
        ModeView::Standard => "Standard mode",
        ModeView::Vim { mode, .. } => match mode {
            ModalMode::Normal => "Normal mode",
            ModalMode::Insert => "Insert mode",
            ModalMode::Visual => "Visual mode",
            ModalMode::OperatorPending => "Operator pending mode",
            ModalMode::CommandLine => "Command line mode",
        },
    }
}

/// The vim mode's short name for the spoken state line.
fn vim_mode_word(mode: ModalMode) -> &'static str {
    match mode {
        ModalMode::Normal => "normal",
        ModalMode::Insert => "insert",
        ModalMode::Visual => "visual",
        ModalMode::OperatorPending => "operator pending",
        ModalMode::CommandLine => "command line",
    }
}

fn operator_word(op: ModalOperator) -> &'static str {
    match op {
        ModalOperator::Delete => "delete",
        ModalOperator::Yank => "yank",
        ModalOperator::Change => "change",
    }
}

fn plural(count: usize, singular: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {singular}s")
    }
}

/// Render the snapshot as one spoken-style line: buffer identity,
/// position, mode, and whatever state is present. Each element
/// appears if and only if the snapshot carries it.
pub fn render_spoken_state(snap: &DocumentSnapshot) -> String {
    let mut parts = Vec::new();

    let mut head = snap.buffer_name.clone();
    if snap.modified {
        head.push_str(" (modified)");
    }
    parts.push(head);
    parts.push(format!(
        "line {} of {}",
        snap.cursor.line + 1,
        snap.line_count
    ));
    parts.push(format!("column {}", snap.cursor.column + 1));

    match &snap.mode {
        ModeView::Emacs => parts.push("Emacs".to_string()),
        ModeView::Standard => parts.push("Standard".to_string()),
        ModeView::Vim {
            mode,
            count,
            operator,
        } => {
            let mut vim = format!("Vim {}", vim_mode_word(*mode));
            if let Some(op) = operator {
                vim.push_str(&format!(", {} pending", operator_word(*op)));
            }
            if *count > 0 {
                vim.push_str(&format!(", count {count}"));
            }
            parts.push(vim);
        }
    }

    if let Some(range) = snap.selection
        && !range.is_empty()
    {
        parts.push(format!("{} selected", plural(range.len(), "byte")));
    }

    let d = &snap.diagnostics;
    if d.errors > 0 {
        parts.push(plural(d.errors, "error"));
    }
    if d.warnings > 0 {
        parts.push(plural(d.warnings, "warning"));
    }

    parts.join(", ")
}

/// Diff two summaries into one announcement string, or `None` when
/// nothing announcement-worthy changed. Rules: announce on semantic
/// change only: buffer switch, mode change, new message, diagnostic
/// count change, selection appearance/clearing, cursor LINE change
/// (columns are never announced). Multiple simultaneous changes join
/// with "; " into a single live-region update.
pub fn announce_diff(
    old: &SnapshotSummary,
    new: &SnapshotSummary,
    announce_cursor_line: bool,
) -> Option<String> {
    let mut parts = Vec::new();

    if old.buffer_id != new.buffer_id {
        parts.push(format!("Buffer {}", new.buffer_name));
    }
    if old.mode != new.mode {
        parts.push(mode_phrase(&new.mode).to_string());
    }
    if old.message.as_deref() != new.message.as_deref()
        && let Some(message) = &new.message
        && !message.is_empty()
    {
        parts.push(message.clone());
    }
    if old.errors != new.errors || old.warnings != new.warnings {
        let mut diag = Vec::new();
        if new.errors > 0 {
            diag.push(plural(new.errors, "error"));
        }
        if new.warnings > 0 {
            diag.push(plural(new.warnings, "warning"));
        }
        parts.push(if diag.is_empty() {
            "no errors or warnings".to_string()
        } else {
            diag.join(", ")
        });
    }
    match (old.selection_bytes, new.selection_bytes) {
        (0, n) if n > 0 => parts.push(format!("{} selected", plural(n, "byte"))),
        (n, 0) if n > 0 => parts.push("selection cleared".to_string()),
        _ => {}
    }
    if announce_cursor_line && old.cursor_line != new.cursor_line {
        parts.push(format!("line {}", new.cursor_line + 1));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}
