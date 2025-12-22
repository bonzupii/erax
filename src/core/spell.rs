//! Spell Checker for String and Comment tokens
//!
//! This module provides spell-checking functionality for prose content
//! in source code, specifically targeting String and Comment tokens
//! identified by TokenKind::is_prose().
//!
//! ## Word List Attribution
//!
//! ### English Open Word List (EOWL)
//!
//! **EOWL v1.1.2** © Ken Loge, 2000-2012  
//! Website: <http://diginoodles.com>
//!
//! The EOWL is a public domain word list containing approximately 128,000 English
//! words. It is released to the public domain and may be freely used and redistributed.
//!
//! ### cspell Software Terms Dictionary
//!
//! **cspell-dicts** © 2017-2025 Street Side Software  
//! License: MIT License  
//! Source: <https://github.com/streetsidesoftware/cspell-dicts>
//!
//! Software terminology dictionary (~3,000 terms) for programming and tech jargon.

// Wired into display.rs for tint rendering.

use std::collections::HashSet;

use crate::core::lexer::Token;

/// Represents a misspelled word with its position information
#[derive(Debug, Clone, PartialEq)]
pub struct Misspelling {
    /// The misspelled word
    pub word: String,
    /// Byte offset from start of input where the word begins
    pub start: usize,
    /// Byte offset from start of input where the word ends (exclusive)
    pub end: usize,
}

impl Misspelling {
    /// Create a new Misspelling instance
    pub fn new(word: String, start: usize, end: usize) -> Self {
        Self { word, start, end }
    }
}

/// Spell checker for identifying misspellings in prose tokens
#[derive(Debug, Default)]
pub struct SpellChecker {
    /// Set of known correct words
    known_words: HashSet<String>,
}

impl SpellChecker {
    /// Create a new SpellChecker with built-in word list
    pub fn new() -> Self {
        let mut checker = Self {
            known_words: HashSet::new(),
        };

        // Add built-in word list
        checker.add_builtin_words();
        checker
    }

    /// Check if a word is known/correct
    pub fn check(&self, word: &str) -> bool {
        self.known_words.contains(&word.to_lowercase())
    }

    /// Check a token for misspellings
    ///
    /// Returns a vector of Misspelling instances found in the token.
    /// Only checks tokens that are prose (String or Comment).
    pub fn check_token(&self, token: &Token) -> Vec<Misspelling> {
        if !token.kind.is_prose() {
            return Vec::new();
        }

        let mut misspellings = Vec::new();
        let text = token.text;

        // Split text into words, handling various separators
        let mut current_start = token.start;
        let mut in_word = false;

        for (i, ch) in text.char_indices() {
            let byte_pos = token.start + i;
            let is_word_char = ch.is_alphabetic() || ch == '\'';

            if is_word_char {
                if !in_word {
                    current_start = byte_pos;
                    in_word = true;
                }
            } else {
                if in_word {
                    let word_end = byte_pos;
                    let word = &text[(current_start - token.start)..(word_end - token.start)];

                    // Skip very short "words" and words with non-alphabetic characters
                    if word.len() >= 2 && word.chars().all(|c| c.is_alphabetic() || c == '\'') {
                        if !self.check(word) {
                            misspellings.push(Misspelling::new(
                                word.to_string(),
                                current_start,
                                word_end,
                            ));
                        }
                    }

                    in_word = false;
                }
            }
        }

        // Check the last word if we're still in one
        if in_word {
            let word_end = token.end();
            let word = &text[(current_start - token.start)..(word_end - token.start)];

            if word.len() >= 2 && word.chars().all(|c| c.is_alphabetic() || c == '\'') {
                if !self.check(word) {
                    misspellings.push(Misspelling::new(word.to_string(), current_start, word_end));
                }
            }
        }

        misspellings
    }

    /// Suggest corrections for a misspelled word
    ///
    /// Currently a placeholder that returns an empty vector.
    /// Future implementation could use edit distance algorithms.
    pub fn suggest(&self, _word: &str) -> Vec<String> {
        Vec::new()
    }

    /// Add built-in word lists:
    /// - EOWL (English Open Word List) - ~128,000 English words (public domain)
    /// - cspell software-terms - ~3,000 programming terms (MIT license)
    fn add_builtin_words(&mut self) {
        // Load EOWL word list at compile time (128,983 words)
        // EOWL v1.1.2 © Ken Loge, 2000-2012 (http://diginoodles.com) - Public Domain
        const EOWL_WORDS: &str = include_str!("../../data/eowl_words.txt");

        for line in EOWL_WORDS.lines() {
            let word = line.trim();
            if !word.is_empty() {
                self.known_words.insert(word.to_lowercase());
            }
        }

        // Load cspell software-terms dictionary (3,170 words)
        // © 2017-2025 Street Side Software - MIT License
        // https://github.com/streetsidesoftware/cspell-dicts
        const SOFTWARE_TERMS: &str = include_str!("../../data/software_terms.txt");

        for line in SOFTWARE_TERMS.lines() {
            let word = line.trim();
            if !word.is_empty() && !word.starts_with('#') {
                self.known_words.insert(word.to_lowercase());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::lexer::{LanguageConfig, Lexer};

    #[test]
    fn test_spell_checker_creation() {
        let checker = SpellChecker::new();
        // "function" is in programming terms, "the" is in English words
        assert!(checker.check("function"));
        assert!(checker.check("the"));
        assert!(!checker.check("xyzzy123"));
    }

    #[test]
    fn test_check_token() {
        let checker = SpellChecker::new();
        let config = LanguageConfig::rust();
        // Use words from the dictionary: "is", "a", "comment", "the"
        let input = r#"// This is a comment with the code"#;

        let tokens: Vec<_> = Lexer::new(input, &config).collect();

        for token in &tokens {
            let misspellings = checker.check_token(token);
            if !token.kind.is_prose() {
                // Non-prose tokens should have no misspellings
                assert!(misspellings.is_empty());
            }
            // Prose tokens may have misspellings for words not in dictionary
        }
    }

    #[test]
    fn test_suggest_placeholder() {
        let checker = SpellChecker::new();
        let suggestions = checker.suggest("misspelled");
        assert!(suggestions.is_empty());
    }
}
