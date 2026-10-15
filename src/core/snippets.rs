//! Code snippets: templates with tab stops.
//!
//! Template-based code snippets with tab-stop placeholders.
//!
//! Snippets are typed `const` data keyed by [`SnippetLanguage`] — no string
//! keys, no hash maps, no per-registration heap allocation (N-180).
//!
//! ## Supported syntax (documented subset)
//!
//! The expander implements this grammar over `&str` (never over raw bytes,
//! so non-ASCII bodies expand intact — N-178):
//!
//! - `$1`, `${1}` — tab stop with an empty default (N-179: a bare `$1`
//!   is a real tab stop, not silent nothing).
//! - `${1:default}` — tab stop with default text.
//! - `$0`, `${0}` — the final tab stop: where the cursor lands after the
//!   last placeholder. Emits no text.
//! - Mirroring: every occurrence of `$n` expands to the same text — the
//!   default from the first `${n:default}` in body order, or empty. All
//!   occurrences are reported in [`TabStop::spans`].
//! - Escapes: `\$`, `\}`, `\\` emit the literal character.
//!
//! Anything outside the subset is left literal and documented as such:
//! `$name` / `${name}` variables are not expanded, an unterminated `${`
//! stays as-is, and a `}` inside a default must be escaped (`\}`) because
//! the first unescaped `}` ends the placeholder. Defaults are literal text;
//! nested placeholders inside defaults are not expanded.

use std::collections::HashMap;

// =============================================================================
// TYPED SNIPPET DATA (N-180)
// =============================================================================

/// Language a snippet applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnippetLanguage {
    Rust,
    C,
    Python,
    JavaScript,
    Go,
    Generic,
}

impl SnippetLanguage {
    /// Map a file extension (without the dot, any ASCII case) to a snippet
    /// language. Unknown or non-ASCII extensions map to [`SnippetLanguage::Generic`].
    pub fn from_extension(ext: &str) -> Self {
        if ext.len() <= 8 && ext.is_ascii() {
            let mut lower = [0u8; 8];
            for (i, b) in ext.bytes().enumerate() {
                lower[i] = b.to_ascii_lowercase();
            }
            match &lower[..ext.len()] {
                b"rs" => Self::Rust,
                b"c" | b"h" => Self::C,
                b"py" | b"pyw" => Self::Python,
                b"js" | b"ts" => Self::JavaScript,
                b"go" => Self::Go,
                _ => Self::Generic,
            }
        } else {
            Self::Generic
        }
    }
}

/// A code snippet: trigger text plus a body template with placeholders.
///
/// All fields are `&'static str`: snippets are compile-time data and never
/// allocate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snippet {
    /// The trigger text that activates this snippet (e.g., "fn", "for", "if")
    pub trigger: &'static str,
    /// The body template with placeholders (`$1`, `${1:default}`, `$0`)
    pub body: &'static str,
    /// Description of what this snippet does
    pub description: &'static str,
}

/// One language's snippet table.
struct LanguageEntry {
    language: SnippetLanguage,
    snippets: &'static [Snippet],
}

/// The complete built-in snippet registry: typed const data.
static SNIPPETS: &[LanguageEntry] = &[
    LanguageEntry {
        language: SnippetLanguage::Rust,
        snippets: &[
            Snippet {
                trigger: "fn",
                body: "fn ${1:name}(${2:params}) -> ${3:return_type} {\n    ${0}\n}",
                description: "Function definition",
            },
            Snippet {
                trigger: "impl",
                body: "impl ${1:type} {\n    ${0}\n}",
                description: "Implementation block",
            },
            Snippet {
                trigger: "struct",
                body: "struct ${1:name} {\n    ${0}\n}",
                description: "Struct definition",
            },
            Snippet {
                trigger: "match",
                body: "match ${1:expression} {\n    ${2:pattern} => ${0},\n}",
                description: "Match expression",
            },
            Snippet {
                trigger: "for",
                body: "for ${1:item} in ${2:iterable} {\n    ${0}\n}",
                description: "For loop",
            },
            Snippet {
                trigger: "if",
                body: "if ${1:condition} {\n    ${0}\n}",
                description: "If statement",
            },
            Snippet {
                trigger: "loop",
                body: "loop {\n    ${0}\n}",
                description: "Infinite loop",
            },
        ],
    },
    LanguageEntry {
        language: SnippetLanguage::C,
        snippets: &[
            Snippet {
                trigger: "for",
                body: "for (${1:i} = 0; ${1:i} < ${2:count}; ${1:i}++) {\n    ${0}\n}",
                description: "For loop",
            },
            Snippet {
                trigger: "if",
                body: "if (${1:condition}) {\n    ${0}\n}",
                description: "If statement",
            },
            Snippet {
                trigger: "while",
                body: "while (${1:condition}) {\n    ${0}\n}",
                description: "While loop",
            },
            Snippet {
                trigger: "switch",
                body: "switch (${1:expression}) {\n    case ${2:value}:\n        ${0}\n        break;\n}",
                description: "Switch statement",
            },
            Snippet {
                trigger: "struct",
                body: "struct ${1:name} {\n    ${0}\n};",
                description: "Struct definition",
            },
        ],
    },
    LanguageEntry {
        language: SnippetLanguage::Python,
        snippets: &[
            Snippet {
                trigger: "def",
                body: "def ${1:name}(${2:params}):\n    ${0}",
                description: "Function definition",
            },
            Snippet {
                trigger: "if",
                body: "if ${1:condition}:\n    ${0}",
                description: "If statement",
            },
            Snippet {
                trigger: "for",
                body: "for ${1:item} in ${2:iterable}:\n    ${0}",
                description: "For loop",
            },
        ],
    },
];

// =============================================================================
// SNIPPET MANAGER
// =============================================================================

/// Manages the snippet registry.
///
/// A unit struct: the registry itself is [`SNIPPETS`] const data, so a
/// manager costs nothing to construct and shares everything by reference.
#[derive(Debug, Clone, Copy, Default)]
pub struct SnippetManager;

impl SnippetManager {
    /// Creates a new SnippetManager over the built-in const snippet tables.
    pub fn new() -> Self {
        Self
    }

    /// Gets all snippets for a language that match the given trigger.
    pub fn get_snippets(
        &self,
        language: SnippetLanguage,
        trigger: &str,
    ) -> Vec<&'static Snippet> {
        SNIPPETS
            .iter()
            .find(|entry| entry.language == language)
            .map_or_else(Vec::new, |entry| {
                entry
                    .snippets
                    .iter()
                    .filter(|s| s.trigger == trigger)
                    .collect()
            })
    }

    /// Expands a snippet body into text plus tab-stop metadata.
    pub fn expand(&self, snippet: &Snippet) -> ExpandedSnippet {
        expand_snippet(snippet.body)
    }
}

// =============================================================================
// EXPANSION
// =============================================================================

/// One tab stop: every place `$n` occurred in the expanded text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabStop {
    /// Tab stop number (`$1`, `$2`, ...).
    pub number: u32,
    /// Byte ranges of each occurrence (mirrors included) in the expanded text.
    pub spans: Vec<(usize, usize)>,
}

/// A snippet body expanded into final text plus tab-stop metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandedSnippet {
    /// Expanded text with each placeholder replaced by its default.
    pub text: String,
    /// Tab stops in tab order (`$1`, `$2`, ...). `$0` is not listed here;
    /// see [`ExpandedSnippet::final_offset`].
    pub tab_stops: Vec<TabStop>,
    /// Byte offset of the final tab stop (`$0`), or the end of the text when
    /// the body has no `$0`.
    pub final_offset: usize,
}

impl ExpandedSnippet {
    /// Byte offset where the cursor should land first: the start of `$1`,
    /// falling back to the final offset when the body has no tab stops.
    pub fn initial_cursor_offset(&self) -> usize {
        self.tab_stops
            .first()
            .and_then(|stop| stop.spans.first())
            .map(|&(start, _)| start)
            .unwrap_or(self.final_offset)
    }
}

/// A parsed piece of a snippet body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token<'a> {
    /// Literal text (escapes not yet processed).
    Text(&'a str),
    /// `$n` / `${n}` / `${n:default}`.
    TabStop { number: u32, default: Option<&'a str> },
    /// `$0` / `${0}`: final cursor position, emits no text.
    Final,
}

/// Split `${...}` inner text into `(number, default)`.
///
/// Returns `None` for non-numeric placeholders (`${name}`), which the
/// documented subset leaves literal.
fn split_placeholder(inner: &str) -> Option<(u32, Option<&str>)> {
    let (num_part, default) = match inner.find(':') {
        Some(p) => (&inner[..p], Some(&inner[p + 1..])),
        None => (inner, None),
    };
    if num_part.is_empty() || !num_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let number: u32 = num_part.parse().ok()?;
    Some((number, default))
}

/// Push `body[*text_start..end]` as literal text, if non-empty.
fn flush_text<'a>(tokens: &mut Vec<Token<'a>>, body: &'a str, text_start: &mut usize, end: usize) {
    if *text_start < end {
        tokens.push(Token::Text(&body[*text_start..end]));
    }
    *text_start = end;
}

/// Tokenize a snippet body over `&str` (never raw bytes — N-178).
fn tokenize(body: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let bytes = body.as_bytes();
    let mut text_start = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' && i + 1 < bytes.len() && matches!(bytes[i + 1], b'$' | b'}' | b'\\') {
            // Escape sequence: emit the escaped char literally.
            flush_text(&mut tokens, body, &mut text_start, i);
            tokens.push(Token::Text(&body[i + 1..i + 2]));
            i += 2;
            text_start = i;
            continue;
        }
        if c != b'$' {
            i += 1;
            continue;
        }
        let j = i + 1;
        if j < bytes.len() && bytes[j] == b'$' {
            // `$$` -> literal `$`.
            flush_text(&mut tokens, body, &mut text_start, i);
            tokens.push(Token::Text(&body[j..j + 1]));
            i = j + 1;
            text_start = i;
            continue;
        }
        if j < bytes.len() && bytes[j] == b'{' {
            // `${...}`: scan to the first unescaped `}`.
            let mut k = j + 1;
            let mut end = None;
            while k < bytes.len() {
                if bytes[k] == b'\\' && k + 1 < bytes.len() {
                    k += 2;
                    continue;
                }
                if bytes[k] == b'}' {
                    end = Some(k);
                    break;
                }
                k += 1;
            }
            if let Some(end) = end {
                // `{` and `}` are ASCII, so these are char boundaries.
                let inner = &body[j + 1..end];
                if let Some((number, default)) = split_placeholder(inner) {
                    flush_text(&mut tokens, body, &mut text_start, i);
                    if number == 0 {
                        tokens.push(Token::Final);
                    } else {
                        tokens.push(Token::TabStop { number, default });
                    }
                    i = end + 1;
                    text_start = i;
                    continue;
                }
                // Non-numeric `${name}`: outside the subset, leave literal.
                i += 1;
                continue;
            }
            // Unterminated `${`: leave literal.
            i += 1;
            continue;
        }
        if j < bytes.len() && bytes[j].is_ascii_digit() {
            let mut k = j;
            while k < bytes.len() && bytes[k].is_ascii_digit() {
                k += 1;
            }
            // ASCII digits: char boundaries hold.
            let number: u32 = body[j..k].parse().unwrap_or(u32::MAX);
            flush_text(&mut tokens, body, &mut text_start, i);
            if number == 0 {
                tokens.push(Token::Final);
            } else {
                tokens.push(Token::TabStop {
                    number,
                    default: None,
                });
            }
            i = k;
            text_start = i;
            continue;
        }
        // `$` followed by anything else (a letter, end of body, ...): literal.
        i += 1;
    }
    flush_text(&mut tokens, body, &mut text_start, bytes.len());
    tokens
}

/// Append `s`, processing `\$`, `\}`, `\\` escapes.
///
/// All slice boundaries are at ASCII bytes (`\`, `$`, `}`, `\`), hence at
/// char boundaries — non-ASCII text passes through untouched.
fn push_escaped(out: &mut String, s: &str) {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut start = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() && matches!(bytes[i + 1], b'$' | b'}' | b'\\') {
            out.push_str(&s[start..i]);
            out.push_str(&s[i + 1..i + 2]);
            i += 2;
            start = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&s[start..]);
}

/// Expand a snippet body: full tab-stop semantics with mirroring (N-179).
fn expand_snippet(body: &str) -> ExpandedSnippet {
    let tokens = tokenize(body);

    // Mirroring: the first `${n:default}` in body order supplies the default
    // for every occurrence of `$n`.
    let mut defaults: HashMap<u32, &str> = HashMap::new();
    for token in &tokens {
        if let Token::TabStop {
            number,
            default: Some(default),
        } = token
        {
            defaults.entry(*number).or_insert(default);
        }
    }

    let mut text = String::with_capacity(body.len());
    let mut spans: HashMap<u32, Vec<(usize, usize)>> = HashMap::new();
    let mut numbers: Vec<u32> = Vec::new();
    let mut final_offset = 0usize;
    let mut saw_final = false;

    for token in tokens {
        match token {
            Token::Text(s) => push_escaped(&mut text, s),
            Token::Final => {
                final_offset = text.len();
                saw_final = true;
            }
            Token::TabStop { number, .. } => {
                let default = defaults.get(&number).copied().unwrap_or("");
                let start = text.len();
                push_escaped(&mut text, default);
                let end = text.len();
                spans
                    .entry(number)
                    .or_insert_with(|| {
                        numbers.push(number);
                        Vec::new()
                    })
                    .push((start, end));
            }
        }
    }
    if !saw_final {
        final_offset = text.len();
    }

    numbers.sort_unstable();
    let tab_stops = numbers
        .into_iter()
        .map(|number| TabStop {
            number,
            spans: spans.remove(&number).unwrap_or_default(),
        })
        .collect();

    ExpandedSnippet {
        text,
        tab_stops,
        final_offset,
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_snippet_manager() {
        let manager = SnippetManager::new();
        // Typed registry: no string keys, const data shared by reference.
        assert!(!manager.get_snippets(SnippetLanguage::Rust, "fn").is_empty());
        assert!(!manager.get_snippets(SnippetLanguage::C, "for").is_empty());
        assert!(!manager.get_snippets(SnippetLanguage::Python, "def").is_empty());
    }

    #[test]
    fn test_get_snippets_unknown_trigger() {
        let manager = SnippetManager::new();
        assert!(manager.get_snippets(SnippetLanguage::Rust, "nonexistent").is_empty());
        // JavaScript has no built-ins yet: honest empty, not an error.
        assert!(manager.get_snippets(SnippetLanguage::JavaScript, "fn").is_empty());
    }

    #[test]
    fn test_snippet_data_is_static_str() {
        // N-180: no owned Strings anywhere in the registry.
        let manager = SnippetManager::new();
        let snippet: &'static Snippet = manager.get_snippets(SnippetLanguage::Rust, "fn")[0];
        assert_eq!(snippet.trigger, "fn");
        assert_eq!(snippet.description, "Function definition");
        let _: &'static str = snippet.body;
    }

    #[test]
    fn test_from_extension() {
        assert_eq!(SnippetLanguage::from_extension("rs"), SnippetLanguage::Rust);
        assert_eq!(SnippetLanguage::from_extension("RS"), SnippetLanguage::Rust);
        assert_eq!(SnippetLanguage::from_extension("c"), SnippetLanguage::C);
        assert_eq!(SnippetLanguage::from_extension("h"), SnippetLanguage::C);
        assert_eq!(SnippetLanguage::from_extension("py"), SnippetLanguage::Python);
        assert_eq!(SnippetLanguage::from_extension("pyw"), SnippetLanguage::Python);
        assert_eq!(SnippetLanguage::from_extension("js"), SnippetLanguage::JavaScript);
        assert_eq!(SnippetLanguage::from_extension("ts"), SnippetLanguage::JavaScript);
        assert_eq!(SnippetLanguage::from_extension("go"), SnippetLanguage::Go);
        assert_eq!(SnippetLanguage::from_extension("txt"), SnippetLanguage::Generic);
        assert_eq!(SnippetLanguage::from_extension(""), SnippetLanguage::Generic);
    }

    #[test]
    fn test_expand_fn_snippet() {
        let manager = SnippetManager::new();
        let snippet = manager.get_snippets(SnippetLanguage::Rust, "fn")[0];
        let expanded = manager.expand(snippet);

        assert_eq!(expanded.text, "fn name(params) -> return_type {\n    \n}");
        assert!(!expanded.text.contains("${1:"));

        let stops: Vec<u32> = expanded.tab_stops.iter().map(|s| s.number).collect();
        assert_eq!(stops, vec![1, 2, 3], "tab stops in tab order");

        // Cursor lands at the start of $1.
        assert_eq!(expanded.initial_cursor_offset(), 3);
        let (start, end) = expanded.tab_stops[0].spans[0];
        assert_eq!(&expanded.text[start..end], "name");

        // $0 became the final offset (before the closing newline).
        assert_eq!(expanded.final_offset, expanded.text.len() - 2);
    }

    #[test]
    fn test_expand_bare_tab_stops() {
        // N-179: a bare `$1` is a real tab stop, not silent nothing.
        let manager = SnippetManager::new();
        let snippet = Snippet {
            trigger: "t",
            body: "for $1 in $2",
            description: "test",
        };
        let expanded = manager.expand(&snippet);

        assert_eq!(expanded.text, "for  in ");
        assert_eq!(expanded.tab_stops.len(), 2);
        assert_eq!(expanded.tab_stops[0].number, 1);
        assert_eq!(expanded.tab_stops[0].spans, vec![(4, 4)]);
        assert_eq!(expanded.tab_stops[1].spans, vec![(8, 8)]);
        assert_eq!(expanded.initial_cursor_offset(), 4);
        // No $0: final offset is the end of the text.
        assert_eq!(expanded.final_offset, expanded.text.len());
    }

    #[test]
    fn test_expand_mirroring() {
        // N-179: repeated `$n` occurrences mirror the same default text.
        let manager = SnippetManager::new();
        let snippet = manager.get_snippets(SnippetLanguage::C, "for")[0];
        let expanded = manager.expand(snippet);

        assert_eq!(
            expanded.text,
            "for (i = 0; i < count; i++) {\n    \n}"
        );
        let stop1 = &expanded.tab_stops[0];
        assert_eq!(stop1.number, 1);
        assert_eq!(stop1.spans.len(), 3, "$1 occurs three times");
        for &(start, end) in &stop1.spans {
            assert_eq!(&expanded.text[start..end], "i");
        }
        let stop2 = &expanded.tab_stops[1];
        assert_eq!(stop2.number, 2);
        assert_eq!(stop2.spans.len(), 1);
    }

    #[test]
    fn test_expand_mirror_before_definition() {
        // A bare `$1` before `${1:default}` still mirrors the default.
        let manager = SnippetManager::new();
        let snippet = Snippet {
            trigger: "t",
            body: "$1 = ${1:value}",
            description: "test",
        };
        let expanded = manager.expand(&snippet);

        assert_eq!(expanded.text, "value = value");
        assert_eq!(expanded.tab_stops[0].spans.len(), 2);
    }

    #[test]
    fn test_expand_non_ascii_intact() {
        // N-178: non-ASCII bodies must not become mojibake.
        let manager = SnippetManager::new();
        let snippet = Snippet {
            trigger: "t",
            body: "// héllo ${1:wörld} — $0 ✓",
            description: "test",
        };
        let expanded = manager.expand(&snippet);

        assert_eq!(expanded.text, "// héllo wörld —  ✓");
        // Offsets are byte offsets on char boundaries.
        assert!(expanded.text.is_char_boundary(expanded.initial_cursor_offset()));
        assert!(expanded.text.is_char_boundary(expanded.final_offset));
        let (start, end) = expanded.tab_stops[0].spans[0];
        assert_eq!(&expanded.text[start..end], "wörld");
    }

    #[test]
    fn test_expand_escapes() {
        let manager = SnippetManager::new();
        let snippet = Snippet {
            trigger: "t",
            body: "cost: $$5, brace: \\}, backslash: \\\\",
            description: "test",
        };
        let expanded = manager.expand(&snippet);

        assert_eq!(expanded.text, "cost: $5, brace: }, backslash: \\");
        assert!(expanded.tab_stops.is_empty());
    }

    #[test]
    fn test_expand_unknown_variable_left_literal() {
        // Documented subset: `$name` variables are not expanded.
        let manager = SnippetManager::new();
        let snippet = Snippet {
            trigger: "t",
            body: "file: $TM_FILENAME",
            description: "test",
        };
        let expanded = manager.expand(&snippet);

        assert_eq!(expanded.text, "file: $TM_FILENAME");
    }

    #[test]
    fn test_expand_unterminated_placeholder_literal() {
        let manager = SnippetManager::new();
        let snippet = Snippet {
            trigger: "t",
            body: "oops ${1:abc",
            description: "test",
        };
        let expanded = manager.expand(&snippet);

        assert_eq!(expanded.text, "oops ${1:abc");
    }

    #[test]
    fn test_tab_stops_sorted_regardless_of_body_order() {
        let manager = SnippetManager::new();
        let snippet = Snippet {
            trigger: "t",
            body: "$2 then $1",
            description: "test",
        };
        let expanded = manager.expand(&snippet);

        let stops: Vec<u32> = expanded.tab_stops.iter().map(|s| s.number).collect();
        assert_eq!(stops, vec![1, 2]);
        // `$2` is at offset 0, `$1` after " then ".
        assert_eq!(expanded.initial_cursor_offset(), 6);
    }
}
