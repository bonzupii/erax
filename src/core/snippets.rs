//! Code Snippets
//!
//! Template-based code snippets with placeholder support.
//! Provides built-in snippets for Rust and C languages.

use std::collections::HashMap;

/// Represents a code snippet with a trigger, body template, and description
#[derive(Debug, Clone)]
pub struct Snippet {
    /// The trigger text that activates this snippet (e.g., "fn", "for", "if")
    pub trigger: String,
    /// The body template with placeholders ($1, $2, etc.)
    pub body: String,
    /// Description of what this snippet does
    pub description: String,
}

/// Manages a collection of code snippets organized by programming language
#[derive(Debug, Default)]
pub struct SnippetManager {
    /// HashMap storing snippets keyed by language
    snippets: HashMap<String, Vec<Snippet>>,
}

impl SnippetManager {
    /// Creates a new SnippetManager with built-in snippets for Rust and C
    pub fn new() -> Self {
        let mut manager = SnippetManager {
            snippets: HashMap::new(),
        };

        // Add Rust snippets
        manager.register(
            "rust",
            Snippet {
                trigger: "fn".to_string(),
                body: "fn ${1:name}(${2:params}) -> ${3:return_type} {\n    ${0}\n}".to_string(),
                description: "Function definition".to_string(),
            },
        );

        manager.register(
            "rust",
            Snippet {
                trigger: "impl".to_string(),
                body: "impl ${1:type} {\n    ${0}\n}".to_string(),
                description: "Implementation block".to_string(),
            },
        );

        manager.register(
            "rust",
            Snippet {
                trigger: "struct".to_string(),
                body: "struct ${1:name} {\n    ${0}\n}".to_string(),
                description: "Struct definition".to_string(),
            },
        );

        manager.register(
            "rust",
            Snippet {
                trigger: "match".to_string(),
                body: "match ${1:expression} {\n    ${2:pattern} => ${0},\n}".to_string(),
                description: "Match expression".to_string(),
            },
        );

        manager.register(
            "rust",
            Snippet {
                trigger: "for".to_string(),
                body: "for ${1:item} in ${2:iterable} {\n    ${0}\n}".to_string(),
                description: "For loop".to_string(),
            },
        );

        manager.register(
            "rust",
            Snippet {
                trigger: "if".to_string(),
                body: "if ${1:condition} {\n    ${0}\n}".to_string(),
                description: "If statement".to_string(),
            },
        );

        manager.register(
            "rust",
            Snippet {
                trigger: "loop".to_string(),
                body: "loop {\n    ${0}\n}".to_string(),
                description: "Infinite loop".to_string(),
            },
        );

        // Add C snippets
        manager.register(
            "c",
            Snippet {
                trigger: "for".to_string(),
                body: "for (${1:i} = 0; ${1:i} < ${2:count}; ${1:i}++) {\n    ${0}\n}".to_string(),
                description: "For loop".to_string(),
            },
        );

        manager.register(
            "c",
            Snippet {
                trigger: "if".to_string(),
                body: "if (${1:condition}) {\n    ${0}\n}".to_string(),
                description: "If statement".to_string(),
            },
        );

        manager.register(
            "c",
            Snippet {
                trigger: "while".to_string(),
                body: "while (${1:condition}) {\n    ${0}\n}".to_string(),
                description: "While loop".to_string(),
            },
        );

        manager.register("c", Snippet {
            trigger: "switch".to_string(),
            body: "switch (${1:expression}) {\n    case ${2:value}:\n        ${0}\n        break;\n}".to_string(),
            description: "Switch statement".to_string(),
        });

        manager.register(
            "c",
            Snippet {
                trigger: "struct".to_string(),
                body: "struct ${1:name} {\n    ${0}\n};".to_string(),
                description: "Struct definition".to_string(),
            },
        );

        manager
    }

    /// Registers a new snippet for the specified language
    pub fn register(&mut self, language: &str, snippet: Snippet) {
        self.snippets
            .entry(language.to_lowercase())
            .or_insert_with(Vec::new)
            .push(snippet);
    }

    /// Gets all snippets for a language that match the given trigger
    pub fn get_snippets(&self, language: &str, trigger: &str) -> Vec<&Snippet> {
        self.snippets
            .get(&language.to_lowercase())
            .map_or_else(Vec::new, |snippets| {
                snippets.iter().filter(|s| s.trigger == trigger).collect()
            })
    }

    /// Expands a snippet by replacing placeholders with their default values
    /// returns (expanded_text, cursor_offset_within_expanded_text)
    pub fn expand(&self, snippet: &Snippet) -> (String, usize) {
        let mut result = String::new();
        let mut cursor_offset = None;
        let mut i = 0;
        let bytes = snippet.body.as_bytes();

        while i < bytes.len() {
            if bytes[i] == b'$' && i + 1 < bytes.len() {
                if bytes[i + 1] == b'{' {
                    // Start of ${n:placeholder}
                    if let Some(end_brace_rel) = snippet.body[i..].find('}') {
                        let end_brace = i + end_brace_rel;
                        let inner = &snippet.body[i + 2..end_brace];
                        if let Some(colon_pos) = inner.find(':') {
                            let placeholder = &inner[colon_pos + 1..];
                            if cursor_offset.is_none() {
                                cursor_offset = Some(result.len());
                            }
                            result.push_str(placeholder);
                        }
                        i = end_brace + 1;
                        continue;
                    }
                } else if bytes[i + 1].is_ascii_digit() {
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                    let num_str = &snippet.body[i + 1..j];
                    if num_str == "0" {
                        cursor_offset = Some(result.len());
                    } else if cursor_offset.is_none() {
                        cursor_offset = Some(result.len());
                    }
                    i = j;
                    continue;
                }
            }
            result.push(bytes[i] as char);
            i += 1;
        }

        let final_offset = cursor_offset.unwrap_or(result.len());
        (result, final_offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_snippet_manager() {
        let manager = SnippetManager::new();
        assert!(!manager.snippets.is_empty());
        assert!(manager.snippets.contains_key("rust"));
        assert!(manager.snippets.contains_key("c"));
    }

    #[test]
    fn test_register_snippet() {
        let mut manager = SnippetManager::new();
        manager.register(
            "python",
            Snippet {
                trigger: "def".to_string(),
                body: "def ${1:name}(${2:params}):\n    ${0}".to_string(),
                description: "Function definition".to_string(),
            },
        );

        let python_snippets = manager.get_snippets("python", "def");
        assert_eq!(python_snippets.len(), 1);
        assert_eq!(python_snippets[0].trigger, "def");
    }

    #[test]
    fn test_get_snippets() {
        let manager = SnippetManager::new();
        let rust_fn_snippets = manager.get_snippets("rust", "fn");
        assert_eq!(rust_fn_snippets.len(), 1);
        assert_eq!(rust_fn_snippets[0].trigger, "fn");
    }

    #[test]
    fn test_expand_snippet() {
        let manager = SnippetManager::new();
        let rust_snippets = manager.get_snippets("rust", "fn");
        assert!(!rust_snippets.is_empty());

        let (expanded, cursor_offset) = manager.expand(&rust_snippets[0]);
        assert!(expanded.contains("fn "));
        assert!(!expanded.contains("${1:"));

        // Check that cursor offset is valid (within string bounds)
        assert!(
            cursor_offset <= expanded.len(),
            "cursor offset {} exceeds string length {}",
            cursor_offset,
            expanded.len()
        );
    }
}
