//! Diff hunk computation for the interactive sed review (N-138, N-139, N-140).
//!
//! [`DiffView`] owns the two documents; the [`Hunk`]s it produces borrow
//! their line text from those documents, so computing hunks copies no line
//! text (N-139). Hunks are built with [`similar::TextDiff`] at the abstraction
//! it provides (N-265): the old/new spans come straight from each hunk's own
//! op ranges — the exact values `similar` prints in its `@@ -a,b +c,d @@`
//! headers — never from hand-rolled index arithmetic (N-138). Every hunk
//! carries [`HUNK_CONTEXT_RADIUS`] context lines on each side so it is
//! decidable on its own (N-140).

use similar::{DiffTag, TextDiff};

/// Context lines kept on each side of a hunk (N-140): the unified-diff
/// default of 3, so every hunk is decidable on its own.
pub const HUNK_CONTEXT_RADIUS: usize = 3;

/// Owns the two documents a diff is computed over.
///
/// Hunks returned by [`compute_hunks`](DiffView::compute_hunks) borrow their
/// line text from these strings: no per-line copies (N-139).
pub struct DiffView {
    original: String,
    modified: String,
}

/// One diff hunk: explicit old/new spans plus borrowed lines.
#[derive(Debug, Clone)]
pub struct Hunk<'a> {
    /// 1-based `(start_line, end_line_exclusive)` span in the original
    /// document. Empty (`start == end`) for a pure insertion.
    pub old_span: (usize, usize),
    /// 1-based `(start_line, end_line_exclusive)` span in the modified
    /// document. Empty (`start == end`) for a pure deletion.
    pub new_span: (usize, usize),
    /// Context, removed, and added lines in hunk order, borrowed from the
    /// [`DiffView`] (N-139). Line text excludes the line terminator.
    pub lines: Vec<DiffLine<'a>>,
}

/// One line of a hunk.
#[derive(Debug, Clone, Copy)]
pub struct DiffLine<'a> {
    pub kind: DiffKind,
    /// Borrowed line text without the trailing newline (N-139).
    pub text: &'a str,
}

/// What a hunk line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// Unchanged context line (N-140).
    Context,
    /// Line present only in the original document.
    Removed,
    /// Line present only in the modified document.
    Added,
}

impl DiffView {
    /// Build a view owning both documents.
    pub fn new(original: String, modified: String) -> Self {
        Self { original, modified }
    }

    /// Compute hunks with [`HUNK_CONTEXT_RADIUS`] context lines on each side.
    ///
    /// The spans are derived from the hunk's own op ranges — the same values
    /// `similar` reports in its `@@ -a,b +c,d @@` headers (N-138) — converted
    /// from 0-based to 1-based, end-exclusive spans.
    ///
    /// Line text is sliced from the owned documents by the op line ranges
    /// (similar 3.x borrows change values from the `TextDiff` itself, so the
    /// documents — not the transient diff — are the borrow source).
    pub fn compute_hunks(&self) -> Vec<Hunk<'_>> {
        let diff = TextDiff::from_lines(&self.original, &self.modified);
        let old_starts = line_starts(&self.original);
        let new_starts = line_starts(&self.modified);
        diff.unified_diff()
            .context_radius(HUNK_CONTEXT_RADIUS)
            .iter_hunks()
            .map(|hunk| {
                let ops = hunk.ops();
                // `iter_hunks` never yields an empty op list; stay total anyway.
                let (old_start, old_end, new_start, new_end) =
                    match (ops.first(), ops.last()) {
                        (Some(first), Some(last)) => (
                            first.old_range().start,
                            last.old_range().end,
                            first.new_range().start,
                            last.new_range().end,
                        ),
                        _ => (0, 0, 0, 0),
                    };
                let mut lines = Vec::new();
                for op in ops {
                    // Equal ops cover the same lines on both sides; emit once.
                    // Replace is removed lines followed by added lines.
                    match op.tag() {
                        DiffTag::Equal => push_lines(
                            &mut lines,
                            &self.original,
                            &old_starts,
                            op.old_range(),
                            DiffKind::Context,
                        ),
                        DiffTag::Delete => push_lines(
                            &mut lines,
                            &self.original,
                            &old_starts,
                            op.old_range(),
                            DiffKind::Removed,
                        ),
                        DiffTag::Insert => push_lines(
                            &mut lines,
                            &self.modified,
                            &new_starts,
                            op.new_range(),
                            DiffKind::Added,
                        ),
                        DiffTag::Replace => {
                            push_lines(
                                &mut lines,
                                &self.original,
                                &old_starts,
                                op.old_range(),
                                DiffKind::Removed,
                            );
                            push_lines(
                                &mut lines,
                                &self.modified,
                                &new_starts,
                                op.new_range(),
                                DiffKind::Added,
                            );
                        }
                    }
                }
                Hunk {
                    old_span: (old_start + 1, old_end + 1),
                    new_span: (new_start + 1, new_end + 1),
                    lines,
                }
            })
            .collect()
    }
}

/// Byte offset of the start of every line in `s` (lines include their
/// terminator). The split points match `similar`'s `tokenize_lines` exactly —
/// after `\r\n`, after a lone `\r`, and after a lone `\n` — so the line
/// indices in `similar`'s op ranges index this table.
fn line_starts(s: &str) -> Vec<usize> {
    let bytes = s.as_bytes();
    let mut starts = vec![0];
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                i += 1;
                if bytes.get(i) == Some(&b'\n') {
                    i += 1;
                }
                starts.push(i);
            }
            b'\n' => {
                i += 1;
                starts.push(i);
            }
            _ => {
                i += 1;
            }
        }
    }
    starts
}

/// Push the lines in `range` (line indices into `doc`) as [`DiffLine`]s with
/// the line terminator stripped (N-139: text is borrowed from the document).
fn push_lines<'a>(
    out: &mut Vec<DiffLine<'a>>,
    doc: &'a str,
    starts: &[usize],
    range: std::ops::Range<usize>,
    kind: DiffKind,
) {
    for i in range {
        let start = starts[i];
        // `starts` ends with a sentinel only when the document ends in `\n`;
        // stay total for a final unterminated line.
        let end = starts.get(i + 1).copied().unwrap_or(doc.len());
        let raw = &doc[start..end];
        let text = raw
            .strip_suffix('\n')
            .map(|t| t.strip_suffix('\r').unwrap_or(t))
            .unwrap_or(raw);
        out.push(DiffLine { kind, text });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("line {i}")).collect()
    }

    fn view(old: &[String], new: &[String]) -> DiffView {
        DiffView::new(old.join("\n"), new.join("\n"))
    }

    #[test]
    fn change_hunk_has_context_and_correct_spans() {
        // N-138/N-140: change line 5 of 10; expect 3 context lines each side.
        let old = numbered(10);
        let mut new = old.clone();
        new[4] = "line FIVE".to_string();
        let view = view(&old, &new);
        let hunks = view.compute_hunks();

        assert_eq!(hunks.len(), 1);
        let h = &hunks[0];
        // 1-based, end-exclusive: old lines 2..=8.
        assert_eq!(h.old_span, (2, 9));
        assert_eq!(h.new_span, (2, 9));
        // 3 context + 1 removed + 1 added + 3 context.
        assert_eq!(h.lines.len(), 8);
        assert!(h.lines.iter().take(3).all(|l| l.kind == DiffKind::Context));
        assert_eq!(h.lines[3].kind, DiffKind::Removed);
        assert_eq!(h.lines[3].text, "line 5");
        assert_eq!(h.lines[4].kind, DiffKind::Added);
        assert_eq!(h.lines[4].text, "line FIVE");
        assert!(h.lines.iter().skip(5).all(|l| l.kind == DiffKind::Context));
    }

    #[test]
    fn insert_only_hunk_spans() {
        // N-138: insert two lines after line 10 of 20.
        let old = numbered(20);
        let mut new = old.clone();
        new.insert(10, "inserted 1".to_string());
        new.insert(11, "inserted 2".to_string());
        let view = view(&old, &new);
        let hunks = view.compute_hunks();

        assert_eq!(hunks.len(), 1);
        let h = &hunks[0];
        // Old: context lines 8..=13 -> (8, 14). New: +2 inserted -> (8, 16).
        assert_eq!(h.old_span, (8, 14));
        assert_eq!(h.new_span, (8, 16));
        assert_eq!(h.lines.len(), 8);
        assert!(h.lines.iter().take(3).all(|l| l.kind == DiffKind::Context));
        assert!(h.lines[3..5].iter().all(|l| l.kind == DiffKind::Added));
        assert_eq!(h.lines[3].text, "inserted 1");
        assert_eq!(h.lines[4].text, "inserted 2");
        assert!(h.lines.iter().skip(5).all(|l| l.kind == DiffKind::Context));
    }

    #[test]
    fn delete_only_hunk_spans() {
        // N-138: delete lines 10-11 of 20.
        let old = numbered(20);
        let mut new = old.clone();
        new.drain(9..11);
        let view = view(&old, &new);
        let hunks = view.compute_hunks();

        assert_eq!(hunks.len(), 1);
        let h = &hunks[0];
        // Old: context 7..=9, removed 10..=11, context 12..=14 -> (7, 15).
        // New: context only -> (7, 13).
        assert_eq!(h.old_span, (7, 15));
        assert_eq!(h.new_span, (7, 13));
        assert_eq!(h.lines.len(), 8);
        assert!(h.lines.iter().take(3).all(|l| l.kind == DiffKind::Context));
        assert!(h.lines[3..5].iter().all(|l| l.kind == DiffKind::Removed));
        assert_eq!(h.lines[3].text, "line 10");
        assert_eq!(h.lines[4].text, "line 11");
        assert!(h.lines.iter().skip(5).all(|l| l.kind == DiffKind::Context));
    }

    #[test]
    fn insert_at_start_has_empty_old_span_start() {
        let old = vec!["m".to_string(), "n".to_string()];
        let new = vec!["a".to_string(), "m".to_string(), "n".to_string()];
        let view = view(&old, &new);
        let hunks = view.compute_hunks();

        assert_eq!(hunks.len(), 1);
        let h = &hunks[0];
        assert_eq!(h.old_span, (1, 3));
        assert_eq!(h.new_span, (1, 4));
        assert_eq!(h.lines.len(), 3);
        assert_eq!(h.lines[0].kind, DiffKind::Added);
        assert_eq!(h.lines[0].text, "a");
    }

    #[test]
    fn insert_into_empty_document() {
        let view = DiffView::new(String::new(), "a\nb".to_string());
        let hunks = view.compute_hunks();

        assert_eq!(hunks.len(), 1);
        let h = &hunks[0];
        // Empty old side: start == end.
        assert_eq!(h.old_span, (1, 1));
        assert_eq!(h.new_span, (1, 3));
        assert!(h.lines.iter().all(|l| l.kind == DiffKind::Added));
    }

    #[test]
    fn identical_documents_have_no_hunks() {
        let old = numbered(5);
        let view = view(&old, &old);
        let hunks = view.compute_hunks();
        assert!(hunks.is_empty());
    }

    #[test]
    fn hunk_line_text_has_no_trailing_newline() {
        let old = numbered(10);
        let mut new = old.clone();
        new[4] = "line FIVE".to_string();
        let view = view(&old, &new);
        let hunks = view.compute_hunks();

        assert!(!hunks.is_empty());
        for line in hunks.iter().flat_map(|h| h.lines.iter()) {
            assert!(
                !line.text.ends_with('\n'),
                "hunk line {line:?} carries a newline"
            );
        }
    }

    #[test]
    fn hunk_lines_are_borrowed_not_copied() {
        // N-139: every hunk line's pointer must lie inside one of the two
        // owned documents. A copy would live on a fresh allocation outside
        // both ranges, so this fails if the implementation ever copies.
        let old = numbered(10);
        let mut new = old.clone();
        new[4] = "line FIVE".to_string();
        let view = view(&old, &new);
        let hunks = view.compute_hunks();

        assert_eq!(hunks.len(), 1);
        let doc_range = |s: &str| {
            let start = s.as_ptr() as usize;
            start..start + s.len()
        };
        let original = doc_range(&view.original);
        let modified = doc_range(&view.modified);
        for line in &hunks[0].lines {
            let ptr = line.text.as_ptr() as usize;
            let end = ptr + line.text.len();
            let in_original = ptr >= original.start && end <= original.end;
            let in_modified = ptr >= modified.start && end <= modified.end;
            assert!(
                in_original || in_modified,
                "hunk line {:?} is not borrowed from either document",
                line.text
            );
        }

        // Content is still exactly right.
        let texts: Vec<&str> = hunks[0].lines.iter().map(|l| l.text).collect();
        assert_eq!(
            texts,
            [
                "line 2",
                "line 3",
                "line 4",
                "line 5",
                "line FIVE",
                "line 6",
                "line 7",
                "line 8"
            ]
        );
    }
}
