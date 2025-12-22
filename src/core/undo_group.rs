//! Smart Undo Grouping
//!
//! Intelligent undo grouping that understands code structure and token boundaries.
//! Groups consecutive edits that belong to the same "logical operation" for better
//! undo/redo experience.

use super::buffer::Edit;
use super::lexer::{LexerState, TokenKind};

/// A group of edits that should be undone/redone together
///
/// Represents a logical unit of editing that should be treated as a single
/// undo/redo operation. For example, typing a word, deleting a line, or
/// modifying a function call.
#[derive(Debug, Clone, Default)]
pub struct UndoGroup {
    /// The edits that belong to this group
    pub edits: Vec<Edit>,
    /// Optional description of what this group represents
    pub description: Option<String>,
}

impl UndoGroup {
    /// Create a new empty undo group
    pub fn new() -> Self {
        Self {
            edits: Vec::new(),
            description: None,
        }
    }

    /// Create an undo group with a specific description
    pub fn with_description(description: impl Into<String>) -> Self {
        Self {
            edits: Vec::new(),
            description: Some(description.into()),
        }
    }

    /// Add an edit to this group
    pub fn add_edit(&mut self, edit: Edit) {
        self.edits.push(edit);
    }

    /// Check if this group is empty
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    /// Get the number of edits in this group
    pub fn len(&self) -> usize {
        self.edits.len()
    }
}

/// Determines how edits should be grouped for undo/redo operations
///
/// Uses lexical analysis to intelligently group edits based on token boundaries.
/// Also batches rapid small edits together to reduce undo stack pressure.
/// This provides a more intuitive undo/redo experience that respects code structure.
#[derive(Debug)]
pub struct UndoGrouper {
    /// Current lexer state for handling multi-line constructs
    current_state: LexerState,
    /// Count of consecutive small edits in current batch
    small_edit_count: usize,
}

impl UndoGrouper {
    /// Maximum small edits to batch together (reduces undo stack pressure)
    const SMALL_EDIT_BATCH_SIZE: usize = 100;

    /// Create a new UndoGrouper
    pub fn new() -> Self {
        Self {
            current_state: LexerState::Normal,
            small_edit_count: 0,
        }
    }

    /// Determine if two consecutive edits should be grouped together
    ///
    /// Uses token boundary analysis to decide whether edits belong to the same
    /// logical operation. Edits are grouped together when they:
    /// - Occur within the same word/identifier
    /// - Are part of the same lexical token
    /// - Maintain the same lexer state
    ///
    /// # Arguments
    /// * `prev_edit` - The previous edit
    /// * `curr_edit` - The current edit being considered
    /// * `lexer_state` - The current lexer state
    ///
    /// # Returns
    /// `true` if the edits should be grouped together, `false` if a new group should be created
    pub fn should_group(
        &mut self,
        prev_edit: &Edit,
        curr_edit: &Edit,
        lexer_state: LexerState,
    ) -> bool {
        // If lexer state changed, we're likely in a different context (e.g., entered/exited string)
        if self.current_state != lexer_state {
            self.current_state = lexer_state;
            self.small_edit_count = 0;
            return false;
        }

        // Check if edits are adjacent or overlapping
        let edits_are_adjacent = match (prev_edit, curr_edit) {
            (
                Edit::Insert {
                    pos: prev_pos,
                    text: prev_text,
                },
                Edit::Insert { pos: curr_pos, .. },
            ) => {
                // Insertions are adjacent if current starts where previous ended
                *curr_pos == *prev_pos + prev_text.len_chars()
            }
            (
                Edit::Delete {
                    pos: prev_pos,
                    text: prev_text,
                },
                Edit::Delete { pos: curr_pos, .. },
            ) => {
                // Deletions are adjacent if they're at the same position or consecutive
                *curr_pos == *prev_pos || *curr_pos == *prev_pos + prev_text.len_chars()
            }
            _ => false, // Different edit types shouldn't be grouped
        };

        if !edits_are_adjacent {
            self.small_edit_count = 0;
            return false;
        }

        // Track if this is a small single-char edit for batch grouping
        let is_small_edit = match curr_edit {
            Edit::Insert { text, .. } => text.len_chars() <= 1,
            Edit::Delete { text, .. } => text.len_chars() <= 1,
        };

        // For adjacent small edits within a batch, group them together
        // This reduces undo stack pressure during rapid typing
        if is_small_edit {
            self.small_edit_count += 1;
            if self.small_edit_count <= Self::SMALL_EDIT_BATCH_SIZE {
                return true; // Group adjacent small edits together
            }
            // Start new batch after threshold
            self.small_edit_count = 1;
        } else {
            self.small_edit_count = 0;
        }

        // Analyze the content to determine if we're at a word boundary
        // We need to check if the edits cross a token boundary
        let crosses_boundary = self.crosses_token_boundary(prev_edit, curr_edit);

        !crosses_boundary
    }

    /// Check if edits cross a token boundary using lexical analysis
    fn crosses_token_boundary(&self, prev_edit: &Edit, curr_edit: &Edit) -> bool {
        // Get the text content from both edits
        let (prev_text, curr_text) = match (prev_edit, curr_edit) {
            (Edit::Insert { text: p_text, .. }, Edit::Insert { text: c_text, .. }) => {
                (p_text, c_text)
            }
            (Edit::Delete { text: p_text, .. }, Edit::Delete { text: c_text, .. }) => {
                (p_text, c_text)
            }
            _ => return true, // Different edit types are always at boundaries
        };

        // If either text is empty, we can't analyze boundaries
        if prev_text.len_chars() == 0 || curr_text.len_chars() == 0 {
            return true;
        }

        // Check the boundary between the two edits
        // The boundary is between the last character of prev_text and first character of curr_text

        // Get the last character of previous edit
        let last_char = match prev_text.chars().last() {
            Some(c) => c,
            None => return true,
        };
        // Get the first character of current edit
        let first_char = match curr_text.chars().next() {
            Some(c) => c,
            None => return true,
        };

        // Determine token types for both characters
        let last_char_token = self.classify_char_token(last_char);
        let first_char_token = self.classify_char_token(first_char);

        // Check if we're crossing a word boundary
        // A word boundary occurs when moving from word-like tokens to non-word-like tokens
        last_char_token.is_word_boundary() != first_char_token.is_word_boundary()
    }

    /// Classify a character into a token type for boundary detection
    fn classify_char_token(&self, ch: char) -> TokenKind {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            // Word-like characters (identifiers, keywords, types)
            TokenKind::Identifier
        } else if ch.is_ascii_whitespace() {
            TokenKind::Whitespace
        } else if ch.is_ascii_punctuation() {
            // Most punctuation is considered operators/delimiters
            match ch {
                '(' | ')' | '{' | '}' | '[' | ']' => TokenKind::Delimiter,
                ';' | ',' | '.' | ':' => TokenKind::Punctuation,
                _ => TokenKind::Operator,
            }
        } else if ch == '"' || ch == '\'' {
            TokenKind::String
        } else if ch == '\n' {
            TokenKind::Newline
        } else {
            TokenKind::Unknown
        }
    }

    /// Create an undo group from a vector of edits
    ///
    /// # Arguments
    /// * `edits` - Vector of edits to group together
    ///
    /// # Returns
    /// A new `UndoGroup` containing the provided edits
    pub fn create_group(&self, edits: Vec<Edit>) -> UndoGroup {
        let mut group = UndoGroup::new();
        group.edits = edits;
        group
    }

    /// Create an undo group with a description from a vector of edits
    pub fn create_group_with_description(
        &self,
        edits: Vec<Edit>,
        description: impl Into<String>,
    ) -> UndoGroup {
        let mut group = UndoGroup::with_description(description);
        group.edits = edits;
        group
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::buffer::Edit;

    #[test]
    fn test_undo_group_creation() {
        let group = UndoGroup::new();
        assert!(group.is_empty());
        assert_eq!(group.len(), 0);

        let group_with_desc = UndoGroup::with_description("test group");
        assert_eq!(group_with_desc.description, Some("test group".to_string()));
    }

    #[test]
    fn test_undo_group_operations() {
        let mut group = UndoGroup::new();
        assert!(group.is_empty());

        group.add_edit(Edit::Insert {
            pos: 0,
            text: "hello".into(),
        });
        group.add_edit(Edit::Insert {
            pos: 5,
            text: " world".into(),
        });

        assert_eq!(group.len(), 2);
        assert!(!group.is_empty());
    }

    #[test]
    fn test_undo_grouper_creation() {
        let grouper = UndoGrouper::new();
        assert_eq!(grouper.current_state, LexerState::Normal);
    }

    #[test]
    fn test_should_group_adjacent_insertions() {
        let mut grouper = UndoGrouper::new();

        let edit1 = Edit::Insert {
            pos: 0,
            text: "hello".into(),
        };
        let edit2 = Edit::Insert {
            pos: 5,
            text: "world".into(),
        };

        // These should be grouped (same word-like content)
        assert!(grouper.should_group(&edit1, &edit2, LexerState::Normal));
    }

    #[test]
    fn test_small_edits_group_across_word_boundary() {
        let mut grouper = UndoGrouper::new();

        let edit1 = Edit::Insert {
            pos: 0,
            text: "hello".into(),
        };
        let edit2 = Edit::Insert {
            pos: 5,
            text: " ".into(),
        };

        // Single-char space IS grouped with previous (batch grouping for small edits)
        // This intentionally bypasses word boundary for rapid typing
        assert!(grouper.should_group(&edit1, &edit2, LexerState::Normal));
    }

    #[test]
    fn test_should_not_group_different_edit_types() {
        let mut grouper = UndoGrouper::new();

        let edit1 = Edit::Insert {
            pos: 0,
            text: "hello".into(),
        };
        let edit2 = Edit::Delete {
            pos: 0,
            text: "hello".into(),
        };

        // Different edit types shouldn't be grouped
        assert!(!grouper.should_group(&edit1, &edit2, LexerState::Normal));
    }

    #[test]
    fn test_create_group() {
        let grouper = UndoGrouper::new();
        let edits = vec![
            Edit::Insert {
                pos: 0,
                text: "test".into(),
            },
            Edit::Insert {
                pos: 4,
                text: " content".into(),
            },
        ];

        let group = grouper.create_group(edits);
        assert_eq!(group.len(), 2);
        assert!(group.description.is_none());

        let group_with_desc = grouper.create_group_with_description(
            vec![Edit::Insert {
                pos: 0,
                text: "single".into(),
            }],
            "test description",
        );
        assert_eq!(
            group_with_desc.description,
            Some("test description".to_string())
        );
    }
}
