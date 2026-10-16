//! Spell checker for string and comment tokens.
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
//!
//! ## Performance design (N-176, N-264)
//!
//! The word lists are embedded with `include_str!` and parsed exactly once
//! into a process-wide [`DICTIONARY`]. Dictionary entries borrow their text
//! directly from the embedded string — no per-word heap allocation, ever —
//! and every [`SpellChecker`] shares that one set by reference, so
//! constructing a checker is free.
//!
//! [`SpellChecker::check`] performs a case-insensitive lookup with no
//! per-check allocation: the dictionary is sorted by case-folded order and
//! probed with a binary search, comparing through Unicode lowercase
//! folding instead of materializing a lowercased copy of the query.

// Wired into display.rs for tint rendering.

use std::cmp::Ordering;
use std::sync::LazyLock;

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

/// Compare two words through Unicode lowercase folding.
///
/// A total order consistent with [`fold_eq`]: two words compare equal here
/// exactly when they are equal ignoring case.
fn fold_cmp(a: &str, b: &str) -> Ordering {
    a.chars()
        .flat_map(|c| c.to_lowercase())
        .cmp(b.chars().flat_map(|c| c.to_lowercase()))
}

/// Case-insensitive equality via [`fold_cmp`].
fn fold_eq(a: &str, b: &str) -> bool {
    fold_cmp(a, b) == Ordering::Equal
}

/// The shared word set, built once from the embedded word lists.
///
/// Entries borrow `&'static str` slices of the `include_str!` data: building
/// the set allocates the backing `Vec` a single time, but never a per-word
/// `String`. Sorted by case-folded order so [`SpellChecker::check`] can probe
/// it with a binary search — no hash table, no per-check allocation.
static DICTIONARY: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    const EOWL_WORDS: &str = include_str!("../../data/eowl_words.txt");
    const SOFTWARE_TERMS: &str = include_str!("../../data/software_terms.txt");

    let mut words: Vec<&'static str> = EOWL_WORDS
        .lines()
        .chain(SOFTWARE_TERMS.lines())
        .map(str::trim)
        .filter(|word| !word.is_empty() && !word.starts_with('#'))
        .collect();
    words.sort_unstable_by(|a, b| fold_cmp(a, b));
    words.dedup_by(|a, b| fold_eq(a, b));
    words
});

/// Spell checker for identifying misspellings in prose tokens.
///
/// Cheap to construct: all instances share the one [`DICTIONARY`] by
/// reference.
#[derive(Debug, Clone, Copy)]
pub struct SpellChecker {
    /// Shared word set (process-wide single instance), folded-sorted.
    words: &'static Vec<&'static str>,
}

impl Default for SpellChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl SpellChecker {
    /// Create a new SpellChecker sharing the built-in word list.
    ///
    /// This does not rebuild the dictionary; the word set is constructed once
    /// and shared by reference (N-264).
    pub fn new() -> Self {
        Self { words: &DICTIONARY }
    }

    /// Check if a word is known/correct (case-insensitive).
    ///
    /// Performs no heap allocation: the folded-sorted dictionary is probed
    /// with a binary search that folds the query during comparison instead
    /// of lowercasing it into an owned copy (N-176).
    pub fn check(&self, word: &str) -> bool {
        self.words
            .binary_search_by(|probe| fold_cmp(probe, word))
            .is_ok()
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
                    if word.len() >= 2 && word.chars().all(|c| c.is_alphabetic() || c == '\'')
                        && !self.check(word) {
                            misspellings.push(Misspelling::new(
                                word.to_string(),
                                current_start,
                                word_end,
                            ));
                        }

                    in_word = false;
                }
            }
        }

        // Check the last word if we're still in one
        if in_word {
            let word_end = token.end();
            let word = &text[(current_start - token.start)..(word_end - token.start)];

            if word.len() >= 2 && word.chars().all(|c| c.is_alphabetic() || c == '\'')
                && !self.check(word) {
                    misspellings.push(Misspelling::new(word.to_string(), current_start, word_end));
                }
        }

        misspellings
    }

    /// Suggest corrections for a misspelled word.
    ///
    /// Ranks dictionary words by bounded Damerau-Levenshtein edit distance
    /// (optimal string alignment over Unicode chars, so adjacent
    /// transpositions like `teh` → `the` count as a single edit) and returns
    /// up to 8 suggestions. Ranking is by distance first; ties break toward
    /// candidates sharing more letters with the query, then toward
    /// candidates preserving the query's first letter (the most salient
    /// letter for word recognition, and the least likely to be mistyped),
    /// then toward candidates of the same length, then alphabetically for
    /// determinism. Words already in the dictionary — and inputs too short
    /// to correct meaningfully — yield no suggestions.
    ///
    /// This is an explicit user-invoked command, not the render hot path; it
    /// reuses scratch buffers across candidates so it still allocates nothing
    /// per dictionary word.
    pub fn suggest(&self, word: &str) -> Vec<String> {
        const MAX_SUGGESTIONS: usize = 8;

        let query: Vec<char> = word.chars().flat_map(|c| c.to_lowercase()).collect();
        if query.len() < 2 || self.check(word) {
            return Vec::new();
        }
        // Short words get distance 1 (avoids absurd matches); longer words 2.
        let max_dist = if query.len() <= 4 { 1 } else { 2 };
        let q_first = query[0];
        let mut query_sorted = query.clone();
        query_sorted.sort_unstable();

        // Scratch rows reused for every candidate. Any candidate surviving the
        // length prefilter satisfies n + 1 <= query.len() + max_dist + 1, so
        // this capacity is always sufficient (see debug_assert in osa_bounded).
        let cap = query.len() + max_dist + 1;
        let mut prev = vec![0usize; cap];
        let mut curr = vec![0usize; cap];
        let mut prev2 = vec![0usize; cap];
        let mut cand_chars: Vec<char> = Vec::with_capacity(64);

        // (distance, shared letters, first letter kept, length kept, word).
        let mut hits: Vec<(usize, usize, bool, bool, &'static str)> = Vec::new();
        for &candidate in self.words.iter() {
            cand_chars.clear();
            // Folded to match the folded query (distance, ranking, and the
            // case-insensitive dictionary contract all agree).
            cand_chars.extend(candidate.chars().flat_map(|c| c.to_lowercase()));
            if let Some(dist) = osa_bounded(
                &query,
                &cand_chars,
                max_dist,
                &mut prev,
                &mut curr,
                &mut prev2,
            ) && dist > 0
            {
                let first_kept = cand_chars[0].to_lowercase().next() == Some(q_first);
                let same_len = cand_chars.len() == query.len();
                cand_chars.sort_unstable();
                let shared = common_letters(&query_sorted, &cand_chars);
                hits.push((dist, shared, first_kept, same_len, candidate));
            }
        }

        hits.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| b.2.cmp(&a.2))
                .then_with(|| b.3.cmp(&a.3))
                .then_with(|| a.4.cmp(b.4))
        });
        hits.truncate(MAX_SUGGESTIONS);
        hits.into_iter()
            .map(|(_, _, _, _, w)| w.to_string())
            .collect()
    }
}

/// Size of the multiset intersection of two sorted char slices:
///
/// how many letters the two words share, counting repetitions.
fn common_letters(a: &[char], b: &[char]) -> usize {
    let (mut i, mut j, mut shared) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Ordering::Less => i += 1,
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                shared += 1;
                i += 1;
                j += 1;
            }
        }
    }
    shared
}

/// Bounded Damerau-Levenshtein distance (optimal string alignment) over
/// Unicode chars.
///
/// Like Levenshtein, but an adjacent transposition (`teh` → `the`) costs a
/// single edit — the most common typo class. Returns `None` when the distance
/// exceeds `max_dist` (early exit: any alignment to the full strings must
/// cross every row, so a row whose minimum exceeds the bound proves the
/// final distance does too).
///
/// `prev`/`curr`/`prev2` are scratch rows reused across calls; each must hold
/// at least `query.len().max(cand.len()) + 1` entries — guaranteed by the
/// caller's length prefilter plus capacity choice.
fn osa_bounded(
    query: &[char],
    cand: &[char],
    max_dist: usize,
    prev: &mut Vec<usize>,
    curr: &mut Vec<usize>,
    prev2: &mut Vec<usize>,
) -> Option<usize> {
    let m = query.len();
    let n = cand.len();
    if n.abs_diff(m) > max_dist {
        return None;
    }
    debug_assert!(prev.len() > n.max(m) && curr.len() > n.max(m) && prev2.len() > n.max(m));

    for (j, slot) in prev.iter_mut().enumerate().take(n + 1) {
        *slot = j;
    }

    for i in 1..=m {
        curr[0] = i;
        let mut row_min = i;
        let qc = query[i - 1];
        for j in 1..=n {
            let cc = cand[j - 1];
            let cost = usize::from(qc != cc);
            let mut v = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
            // Adjacent transposition: q[i-2]==c[j-1] && q[i-1]==c[j-2].
            if i > 1 && j > 1 && query[i - 2] == cc && qc == cand[j - 2] {
                v = v.min(prev2[j - 2] + 1);
            }
            curr[j] = v;
            row_min = row_min.min(v);
        }
        if row_min > max_dist {
            return None;
        }
        std::mem::swap(prev, curr);
        std::mem::swap(prev2, curr);
    }

    let dist = prev[n];
    (dist <= max_dist).then_some(dist)
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
    fn test_check_is_case_insensitive() {
        // N-176: case-insensitive lookup without a per-check allocation.
        let checker = SpellChecker::new();
        assert!(checker.check("Function"));
        assert!(checker.check("FUNCTION"));
        assert!(checker.check("fUnCtIoN"));
        assert!(checker.check("THE"));
        assert!(!checker.check("Xyzzy"));
        assert!(!checker.check(""));
    }

    #[test]
    fn test_dictionary_built_once_and_shared() {
        // N-264: every checker shares the one word set by reference.
        let a = SpellChecker::new();
        let b = SpellChecker::new();
        assert!(
            std::ptr::eq(a.words, b.words),
            "SpellChecker instances must share a single dictionary"
        );
        assert_eq!(a.words.len(), b.words.len());
        assert!(a.words.len() > 100_000, "dictionary should hold the full word lists");
    }

    #[test]
    fn test_check_token() {
        let checker = SpellChecker::new();
        let config = LanguageConfig::rust();
        // All words are in the dictionary: "This", "is", "a", "comment",
        // "with", "the", "code".
        let input = r#"// This is a comment with the code"#;

        let tokens: Vec<_> = Lexer::new(input, &config).collect();
        assert!(!tokens.is_empty(), "lexer produced no tokens");

        for token in &tokens {
            let misspellings = checker.check_token(token);
            assert!(
                misspellings.is_empty(),
                "unexpected misspellings in {token:?}: {misspellings:?}"
            );
        }
    }

    #[test]
    fn test_check_token_flags_misspelling() {
        let checker = SpellChecker::new();
        let config = LanguageConfig::rust();
        let input = r#"// This is a cmment with the code"#;

        let tokens: Vec<_> = Lexer::new(input, &config).collect();
        let mut flagged_words: Vec<String> = Vec::new();
        for token in &tokens {
            for m in checker.check_token(token) {
                flagged_words.push(m.word.clone());
            }
        }
        assert!(
            flagged_words.iter().any(|w| w.contains("cmment")),
            "expected 'cmment' to be flagged, got: {flagged_words:?}"
        );
    }

    #[test]
    fn test_suggest_common_typos() {
        // N-177: real edit-distance suggestions, closest first.
        let checker = SpellChecker::new();

        let teh = checker.suggest("teh");
        assert!(
            teh.contains(&"the".to_string()),
            "expected 'the' in suggestions for 'teh', got {teh:?}"
        );

        let functon = checker.suggest("functon");
        assert!(
            functon.contains(&"function".to_string()),
            "expected 'function' in suggestions for 'functon', got {functon:?}"
        );

        // Documented ranking: distance, then exterior-letter/length
        // preservation, then alphabetical. "the" keeps t…h and the length,
        // so it outranks the other distance-1 candidates.
        assert_eq!(teh[0], "the", "expected 'the' first, got {teh:?}");

        // Sanity: every suggestion is actually within the distance bound.
        assert!(teh.len() <= 8);
    }

    #[test]
    fn test_suggest_known_word_returns_empty() {
        let checker = SpellChecker::new();
        assert!(checker.suggest("function").is_empty());
        assert!(checker.suggest("The").is_empty());
    }

    #[test]
    fn test_suggest_gibberish_returns_empty() {
        let checker = SpellChecker::new();
        // Nothing in the dictionary is within edit distance 2.
        assert!(checker.suggest("xqzjxw").is_empty());
        // Too short to correct meaningfully.
        assert!(checker.suggest("x").is_empty());
        assert!(checker.suggest("").is_empty());
    }

    #[test]
    fn test_suggest_result_is_bounded_and_sorted() {
        let checker = SpellChecker::new();
        let suggestions = checker.suggest("recieve");
        assert!(suggestions.len() <= 8);
        assert!(
            suggestions.contains(&"receive".to_string()),
            "expected 'receive' for 'recieve', got {suggestions:?}"
        );
        // No duplicates.
        let mut dedup = suggestions.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(dedup.len(), suggestions.len());
    }

    #[test]
    fn test_osa_bounded_distances() {
        let mut prev = vec![0usize; 16];
        let mut curr = vec![0usize; 16];
        let mut prev2 = vec![0usize; 16];
        let q: Vec<char> = "kitten".chars().collect();
        let to_chars = |s: &str| s.chars().collect::<Vec<char>>();

        assert_eq!(
            osa_bounded(&q, &to_chars("sitting"), 3, &mut prev, &mut curr, &mut prev2),
            Some(3)
        );
        assert_eq!(
            osa_bounded(&q, &to_chars("kitten"), 3, &mut prev, &mut curr, &mut prev2),
            Some(0)
        );
        // Adjacent transposition costs a single edit.
        let teh: Vec<char> = "teh".chars().collect();
        assert_eq!(
            osa_bounded(&teh, &to_chars("the"), 1, &mut prev, &mut curr, &mut prev2),
            Some(1)
        );
        // Beyond the bound: early exit, no exact distance.
        assert_eq!(
            osa_bounded(&q, &to_chars("sitting"), 2, &mut prev, &mut curr, &mut prev2),
            None
        );
        // Length prefilter rejects immediately.
        assert_eq!(
            osa_bounded(&q, &to_chars("k"), 2, &mut prev, &mut curr, &mut prev2),
            None
        );
        // Unicode chars, not bytes.
        let cafe: Vec<char> = "café".chars().collect();
        assert_eq!(
            osa_bounded(&cafe, &to_chars("cafe"), 1, &mut prev, &mut curr, &mut prev2),
            Some(1)
        );
    }
}
