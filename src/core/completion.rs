//! Word Completion
//!
//! Buffer-local word completion (no LSP required).
//! Uses the unified Lexer to extract identifiers from buffer content.

use std::collections::HashSet;

use super::buffer::Buffer;
use super::lexer::{LanguageConfig, Lexer, TokenKind};

/// Word completer for buffer-local completion
///
/// Collects identifiers from the buffer and provides completions
/// based on a prefix match. No external LSP required.
#[derive(Debug, Default)]
pub struct WordCompleter {
    /// Cached words from the buffer
    cached_words: HashSet<String>,
    /// ID of the buffer the cache belongs to
    last_buffer_id: Option<crate::core::id::BufferId>,
    /// Version of the buffer when words were collected
    last_buffer_version: u64,
}

impl WordCompleter {
    /// Create a new empty WordCompleter
    pub fn new() -> Self {
        Self {
            cached_words: HashSet::new(),
            last_buffer_id: None,
            last_buffer_version: 0,
        }
    }

    /// Collect all identifier words from buffer content
    ///
    /// Uses the Lexer to tokenize and extract Identifier tokens.
    /// Returns a HashSet of unique words.
    pub fn collect_words(buffer_content: &str, config: &LanguageConfig) -> HashSet<String> {
        let mut words = HashSet::new();
        let lexer = Lexer::with_state(buffer_content, config, super::lexer::LexerState::Normal);

        for token in lexer {
            if token.kind == TokenKind::Identifier {
                // Only include identifiers that are reasonably sized
                if token.text.len() >= 2 && token.text.len() <= 100 {
                    words.insert(token.text.to_string());
                }
            }
        }

        words
    }

    /// Get completions matching a prefix
    ///
    /// Returns a sorted list of words that start with the given prefix.
    /// The prefix itself is excluded from results.
    pub fn get_completions(prefix: &str, words: &HashSet<String>) -> Vec<String> {
        if prefix.is_empty() {
            return Vec::new();
        }

        let mut matches: Vec<String> = words
            .iter()
            .filter(|w| w.starts_with(prefix) && w.as_str() != prefix)
            .cloned()
            .collect();

        // Sort alphabetically, then by length (shorter first)
        matches.sort_by(|a, b| match a.len().cmp(&b.len()) {
            std::cmp::Ordering::Equal => a.cmp(b),
            other => other,
        });

        matches
    }

    /// Complete at cursor position in buffer
    ///
    /// Finds the word prefix at the cursor position and returns completions.
    /// Returns empty Vec if cursor is not in a word.
    pub fn complete_at_cursor(
        &mut self,
        buffer: &Buffer,
        buffer_id: crate::core::id::BufferId,
        cursor_byte: usize,
    ) -> Vec<String> {
        let content = buffer.to_string();

        // Find word start by scanning backwards from cursor
        let prefix_start = content[..cursor_byte]
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_ascii_alphanumeric() || *c == '_')
            .last()
            .map(|(i, _)| i)
            .unwrap_or(cursor_byte);

        if prefix_start >= cursor_byte {
            return Vec::new();
        }

        let prefix = &content[prefix_start..cursor_byte];
        if prefix.is_empty() {
            return Vec::new();
        }

        // Collect words if cache is empty or buffer changed
        if self.cached_words.is_empty()
            || self.last_buffer_id != Some(buffer_id)
            || self.last_buffer_version != buffer.version
        {
            let config = LanguageConfig::generic();
            self.cached_words = Self::collect_words(&content, &config);
            self.last_buffer_id = Some(buffer_id);
            self.last_buffer_version = buffer.version;
        }

        Self::get_completions(prefix, &self.cached_words)
    }

    /// Clear the word cache (call on buffer modification)
    pub fn invalidate_cache(&mut self) {
        self.cached_words.clear();
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_words() {
        let content = "fn main() { let foo = bar + baz; }";
        let config = LanguageConfig::generic();
        let words = WordCompleter::collect_words(content, &config);

        assert!(words.contains("main"));
        assert!(words.contains("foo"));
        assert!(words.contains("bar"));
        assert!(words.contains("baz"));
        // "fn" and "let" are keywords, not identifiers in generic config
    }

    #[test]
    fn test_get_completions() {
        let mut words = HashSet::new();
        words.insert("foobar".to_string());
        words.insert("foobaz".to_string());
        words.insert("football".to_string());
        words.insert("other".to_string());

        let completions = WordCompleter::get_completions("foo", &words);
        assert_eq!(completions.len(), 3);
        assert!(completions.iter().all(|c| c.starts_with("foo")));
    }

    #[test]
    fn test_empty_prefix() {
        let words: HashSet<String> = ["foo", "bar"].iter().map(|s| s.to_string()).collect();
        let completions = WordCompleter::get_completions("", &words);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_no_self_match() {
        let words: HashSet<String> = ["foo", "foobar"].iter().map(|s| s.to_string()).collect();
        let completions = WordCompleter::get_completions("foo", &words);

        // Should not include "foo" itself
        assert!(!completions.contains(&"foo".to_string()));
        assert!(completions.contains(&"foobar".to_string()));
    }
}
