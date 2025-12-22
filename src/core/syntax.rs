//! Syntax Highlighting Module
//!
//! This module provides the syntax highlighting interface for the editor.
//! It uses the unified lexer from `lexer.rs` as the sole tokenization engine.

use std::collections::HashMap;

use crate::core::lexer::{LanguageConfig, Lexer, LexerState, TokenKind};

// Re-export LexerState for callers that need it
pub use crate::core::lexer::LexerState as SyntaxLexerState;

// =============================================================================
// TOKEN TYPE (for theme color mapping)
// =============================================================================

/// Token types for syntax highlighting (maps to theme colors)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenType {
    /// Keywords (if, else, for, while, etc.)
    Keyword,
    /// Type names (int, char, void, struct, etc.)
    Type,
    /// String literals
    String,
    /// Character literals
    Char,
    /// Numeric literals
    Number,
    /// Comments
    Comment,
    /// Preprocessor directives (#include, #define)
    Preprocessor,
    /// Function names (identifier followed by parenthesis)
    Function,
    /// Operators (+, -, *, /, =, etc.)
    Operator,
    /// Punctuation (braces, parens, semicolons, etc.)
    Punctuation,
    /// Normal text / fallback
    Normal,
}

impl From<TokenKind> for TokenType {
    fn from(kind: TokenKind) -> Self {
        match kind {
            TokenKind::Keyword => TokenType::Keyword,
            TokenKind::Type => TokenType::Type,
            TokenKind::String => TokenType::String,
            TokenKind::Char => TokenType::Char,
            TokenKind::Number => TokenType::Number,
            TokenKind::Comment => TokenType::Comment,
            TokenKind::Preprocessor => TokenType::Preprocessor,
            TokenKind::Identifier => TokenType::Normal, // Plain identifiers are normal text
            TokenKind::Operator => TokenType::Operator,
            TokenKind::Delimiter => TokenType::Punctuation,
            TokenKind::Punctuation => TokenType::Punctuation,
            TokenKind::Whitespace | TokenKind::Newline | TokenKind::Unknown => TokenType::Normal,
        }
    }
}

// =============================================================================
// HIGHLIGHT SPAN (for rendering)
// =============================================================================

/// A highlighted region in a line
#[derive(Debug, Clone)]
pub struct HighlightSpan {
    /// Start byte offset within the line
    pub start: usize,
    /// End byte offset within the line (exclusive)
    pub end: usize,
    /// Token type for this span
    pub token_type: TokenType,
}

impl HighlightSpan {
    pub fn new(start: usize, end: usize, token_type: TokenType) -> Self {
        Self {
            start,
            end,
            token_type,
        }
    }
}

// =============================================================================
// LANGUAGE REGISTRY
// =============================================================================

/// Registry mapping file extensions to language configurations
pub struct LanguageRegistry {
    /// Extension -> Language name mapping
    extension_map: HashMap<String, String>,
    /// Language name -> Config mapping  
    configs: HashMap<String, LanguageConfig>,
}

impl LanguageRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            extension_map: HashMap::new(),
            configs: HashMap::new(),
        };

        // Register C
        registry.register("c", LanguageConfig::c(), &["c", "h"]);

        // Register Rust
        registry.register("rust", LanguageConfig::rust(), &["rs"]);

        // Register Python
        registry.register("python", LanguageConfig::python(), &["py", "pyw"]);

        // Register Go
        registry.register("go", LanguageConfig::go(), &["go"]);

        // Register JavaScript/TypeScript
        registry.register(
            "javascript",
            LanguageConfig::javascript(),
            &["js", "jsx", "ts", "tsx", "mjs", "cjs"],
        );

        // Register generic (fallback)
        registry.register("generic", LanguageConfig::generic(), &["txt", "md", "log"]);

        registry
    }

    fn register(&mut self, name: &str, config: LanguageConfig, extensions: &[&str]) {
        self.configs.insert(name.to_string(), config);
        for ext in extensions {
            self.extension_map.insert(ext.to_string(), name.to_string());
        }
    }

    /// Get the language config for a file extension
    pub fn get_config(&self, extension: &str) -> &LanguageConfig {
        let lang_name = self
            .extension_map
            .get(extension)
            .map(|s| s.as_str())
            .unwrap_or("generic");

        self.configs
            .get(lang_name)
            .or_else(|| self.configs.get("generic"))
            .expect("generic config must exist")
    }
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// SYNTAX HIGHLIGHTER
// =============================================================================

/// Syntax highlighter using the unified lexer
pub struct SyntaxHighlighter {
    registry: LanguageRegistry,
}

impl SyntaxHighlighter {
    pub fn new() -> Self {
        Self {
            registry: LanguageRegistry::new(),
        }
    }

    /// Highlight a single line and return spans with the resulting lexer state.
    ///
    /// # Arguments
    /// * `ext` - File extension to determine the language
    /// * `line` - The single line string to highlight
    /// * `state` - The current lexer state (for multi-line constructs)
    ///
    /// # Returns
    /// A tuple of (vector of `HighlightSpan`s, resulting lexer state)
    pub fn highlight_line_with_state(
        &self,
        ext: &str,
        line: &str,
        state: LexerState,
    ) -> (Vec<HighlightSpan>, LexerState) {
        let config = self.registry.get_config(ext);
        let mut lexer = Lexer::with_state(line, config, state);
        let mut spans = Vec::new();

        // Collect all tokens first for lookahead
        let tokens: Vec<_> = lexer.by_ref().collect();

        for (i, token) in tokens.iter().enumerate() {
            // Skip whitespace and newlines - they don't need highlighting
            if matches!(token.kind, TokenKind::Whitespace | TokenKind::Newline) {
                continue;
            }

            // Function detection: identifier followed by ( is a function call
            let token_type = if token.kind == TokenKind::Identifier {
                // Look ahead for ( after possible whitespace
                let mut next_idx = i + 1;
                while next_idx < tokens.len() && tokens[next_idx].kind == TokenKind::Whitespace {
                    next_idx += 1;
                }
                if next_idx < tokens.len() && tokens[next_idx].text == "(" {
                    TokenType::Function
                } else {
                    TokenType::Normal
                }
            } else {
                token.kind.into()
            };

            spans.push(HighlightSpan::new(token.start, token.end(), token_type));
        }

        (spans, lexer.state())
    }
}

impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_rust_keywords() {
        let highlighter = SyntaxHighlighter::new();
        let (spans, state) =
            highlighter.highlight_line_with_state("rs", "fn main() {", LexerState::Normal);

        assert_eq!(state, LexerState::Normal);
        assert!(!spans.is_empty());

        // First token should be "fn" keyword
        assert_eq!(spans[0].token_type, TokenType::Keyword);
    }

    #[test]
    fn test_highlight_c_preprocessor() {
        let highlighter = SyntaxHighlighter::new();
        let (spans, state) =
            highlighter.highlight_line_with_state("c", "#include <stdio.h>", LexerState::Normal);

        assert_eq!(state, LexerState::Normal);
        assert!(!spans.is_empty());

        // First token should be preprocessor
        assert_eq!(spans[0].token_type, TokenType::Preprocessor);
    }

    #[test]
    fn test_highlight_multiline_comment() {
        let highlighter = SyntaxHighlighter::new();

        // Start a block comment that doesn't close
        let (spans1, state1) =
            highlighter.highlight_line_with_state("c", "/* comment starts", LexerState::Normal);
        assert_eq!(state1, LexerState::InBlockComment);
        assert!(!spans1.is_empty());
        assert_eq!(spans1[0].token_type, TokenType::Comment);

        // Continue the comment
        let (spans2, state2) =
            highlighter.highlight_line_with_state("c", "comment continues", state1);
        assert_eq!(state2, LexerState::InBlockComment);
        assert!(!spans2.is_empty());
        assert_eq!(spans2[0].token_type, TokenType::Comment);

        // End the comment
        let (spans3, state3) = highlighter.highlight_line_with_state("c", "end */ int x;", state2);
        assert_eq!(state3, LexerState::Normal);
        assert!(spans3.len() >= 2); // Comment and then identifiers
    }

    #[test]
    fn test_highlight_string_literal() {
        let highlighter = SyntaxHighlighter::new();
        let (spans, state) =
            highlighter.highlight_line_with_state("rs", "let s = \"hello\";", LexerState::Normal);

        assert_eq!(state, LexerState::Normal);

        // Find the string token
        let string_span = spans.iter().find(|s| s.token_type == TokenType::String);
        assert!(string_span.is_some());
    }

    #[test]
    fn test_highlight_numbers() {
        let highlighter = SyntaxHighlighter::new();
        let (spans, _) =
            highlighter.highlight_line_with_state("rs", "let x = 42;", LexerState::Normal);

        // Find the number token
        let number_span = spans.iter().find(|s| s.token_type == TokenType::Number);
        assert!(number_span.is_some());
    }

    #[test]
    fn test_token_type_conversion() {
        assert_eq!(TokenType::from(TokenKind::Keyword), TokenType::Keyword);
        assert_eq!(TokenType::from(TokenKind::String), TokenType::String);
        assert_eq!(TokenType::from(TokenKind::Comment), TokenType::Comment);
        assert_eq!(TokenType::from(TokenKind::Whitespace), TokenType::Normal);
    }
}
