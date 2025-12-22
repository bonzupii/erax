use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone)]
pub struct Hunk {
    pub start_line: usize,
    pub end_line: usize,
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
}

pub struct DiffView {
    pub original: String,
    pub modified: String,
}

impl DiffView {
    pub fn new(original: String, modified: String) -> Self {
        Self { original, modified }
    }

    pub fn compute_hunks(&self) -> Vec<Hunk> {
        let diff = TextDiff::from_lines(&self.original, &self.modified);
        let mut hunks = Vec::new();

        for hunk in diff.unified_diff().iter_hunks() {
            let mut old_lines = Vec::new();
            let mut new_lines = Vec::new();
            let mut start_line = 0;
            let mut end_line = 0;
            let mut first = true;

            for change in hunk.iter_changes() {
                if first {
                    start_line = change.old_index().unwrap_or(0);
                    first = false;
                }
                end_line = change.old_index().unwrap_or(end_line);

                match change.tag() {
                    ChangeTag::Delete => {
                        old_lines.push(change.value().to_string());
                    }
                    ChangeTag::Insert => {
                        new_lines.push(change.value().to_string());
                    }
                    ChangeTag::Equal => {
                        // In a hunk, we might want to keep some context or just the changes.
                        // For a simple side-by-side diff preview, we usually want the changed lines.
                    }
                }
            }

            if !old_lines.is_empty() || !new_lines.is_empty() {
                hunks.push(Hunk {
                    start_line,
                    end_line,
                    old_lines,
                    new_lines,
                });
            }
        }

        hunks
    }
}

pub struct DiffState {
    pub hunks: Vec<Hunk>,
    pub current_hunk: usize,
    pub original_buffer_id: crate::core::id::BufferId,
    pub original_window_id: crate::core::id::WindowId,
}
