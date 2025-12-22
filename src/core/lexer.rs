//! Unified Token Lexer
//!
//! A pull-parser lexer that serves both syntax highlighting and tooling
//! (calculator, word completion, undo grouping, etc.)
//!
//! Key design principles:
//! - **Zero-copy:** Token.text is a &str slice of original input
//! - **O(1) memory:** Lazy iterator, no upfront allocation
//! - **State caching:** LexerState stored at line boundaries for fast restart

use std::collections::HashSet;

// =============================================================================
// TOKEN TYPES
// =============================================================================

/// Token kind for syntax highlighting and language tooling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    // Identifiers & Keywords
    Identifier, // my_var, calculate
    Keyword,    // fn, let, return (language-specific)
    Type,       // int, String, bool (language-specific)

    // Literals
    Number, // 123, 0xFF, 0b101, 3.14, 1_000_000
    String, // "hello"
    Char,   // 'a'

    // Operators
    Operator, // +, -, *, /, %, ^, &, |, ~, <<, >>, **

    // Delimiters
    Delimiter,   // (, ), {, }, [, ]
    Punctuation, // ;, ,, ., :, ::, ->

    // Comments & Preprocessor
    Comment,      // // or /* */
    Preprocessor, // #include, #define

    // Whitespace & structure
    Whitespace,
    Newline,

    // Fallback
    Unknown,
}

impl TokenKind {
    /// Returns true if this token is typically prose (for spell-checking)
    pub fn is_prose(&self) -> bool {
        matches!(self, TokenKind::String | TokenKind::Comment)
    }

    /// Returns true if this token is a word boundary for undo grouping
    pub fn is_word_boundary(&self) -> bool {
        !matches!(
            self,
            TokenKind::Identifier | TokenKind::Keyword | TokenKind::Type
        )
    }
}

// =============================================================================
// TOKEN STRUCTURE
// =============================================================================

/// A token with zero-copy text slice and position information
#[derive(Debug, Clone, PartialEq)]
pub struct Token<'a> {
    /// The kind of this token
    pub kind: TokenKind,
    /// Zero-copy slice into original input
    pub text: &'a str,
    /// Byte offset from start of input
    pub start: usize,
    /// Byte length
    pub len: usize,
}

impl<'a> Token<'a> {
    pub fn new(kind: TokenKind, text: &'a str, start: usize) -> Self {
        Self {
            kind,
            text,
            start,
            len: text.len(),
        }
    }

    /// Byte offset of end (exclusive)
    pub fn end(&self) -> usize {
        self.start + self.len
    }
}

// =============================================================================
// LEXER STATE (for multi-line constructs)
// =============================================================================

/// State for handling multi-line constructs (block comments, strings)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
// InRawString variant for future Rust raw string support
pub enum LexerState {
    #[default]
    Normal,
    InBlockComment,
    InString(char), // The quote character
}

// =============================================================================
// LANGUAGE CONFIGURATION
// =============================================================================

/// Language-specific rules for lexing
pub struct LanguageConfig {
    pub name: &'static str,
    pub keywords: HashSet<&'static str>,
    pub types: HashSet<&'static str>,
    pub line_comment: &'static str,
    pub block_comment_start: &'static str,
    pub block_comment_end: &'static str,
    pub has_preprocessor: bool,
}

impl LanguageConfig {
    pub fn c() -> Self {
        Self {
            name: "c",
            keywords: [
                "auto",
                "break",
                "case",
                "const",
                "continue",
                "default",
                "do",
                "else",
                "enum",
                "extern",
                "for",
                "goto",
                "if",
                "inline",
                "register",
                "restrict",
                "return",
                "signed",
                "sizeof",
                "static",
                "struct",
                "switch",
                "typedef",
                "union",
                "unsigned",
                "volatile",
                "while",
                "_Alignas",
                "_Alignof",
                "_Atomic",
                "_Bool",
                "_Complex",
                "_Generic",
                "_Imaginary",
                "_Noreturn",
                "_Static_assert",
                "_Thread_local",
            ]
            .into_iter()
            .collect(),
            types: [
                "void",
                "char",
                "short",
                "int",
                "long",
                "float",
                "double",
                "size_t",
                "ssize_t",
                "ptrdiff_t",
                "intptr_t",
                "uintptr_t",
                "int8_t",
                "int16_t",
                "int32_t",
                "int64_t",
                "uint8_t",
                "uint16_t",
                "uint32_t",
                "uint64_t",
            ]
            .into_iter()
            .collect(),
            line_comment: "//",
            block_comment_start: "/*",
            block_comment_end: "*/",
            has_preprocessor: true,
        }
    }

    pub fn rust() -> Self {
        Self {
            name: "rust",
            keywords: [
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
                "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
                "super", "trait", "true", "type", "unsafe", "use", "where", "while",
            ]
            .into_iter()
            .collect(),
            types: [
                "bool", "char", "str", "String", "i8", "i16", "i32", "i64", "i128", "u8", "u16",
                "u32", "u64", "u128", "isize", "usize", "f32", "f64", "Vec", "Option", "Result",
                "Box", "Rc", "Arc", "Cell", "RefCell",
            ]
            .into_iter()
            .collect(),
            line_comment: "//",
            block_comment_start: "/*",
            block_comment_end: "*/",
            has_preprocessor: false,
        }
    }

    pub fn python() -> Self {
        Self {
            name: "python",
            keywords: [
                "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class",
                "continue", "def", "del", "elif", "else", "except", "finally", "for", "from",
                "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass",
                "raise", "return", "try", "while", "with", "yield",
            ]
            .into_iter()
            .collect(),
            types: [
                "int",
                "float",
                "str",
                "bool",
                "list",
                "dict",
                "tuple",
                "set",
                "frozenset",
                "bytes",
                "bytearray",
                "complex",
                "range",
                "slice",
            ]
            .into_iter()
            .collect(),
            line_comment: "#",
            block_comment_start: "\"\"\"",
            block_comment_end: "\"\"\"",
            has_preprocessor: false,
        }
    }

    pub fn generic() -> Self {
        Self {
            name: "generic",
            keywords: [
                "if", "else", "for", "while", "return", "break", "continue", "switch", "case",
                "default", "do", "class", "struct", "enum", "function", "fn", "def", "var", "let",
                "const", "import", "export",
            ]
            .into_iter()
            .collect(),
            types: ["int", "float", "double", "char", "bool", "void", "string"]
                .into_iter()
                .collect(),
            line_comment: "//",
            block_comment_start: "/*",
            block_comment_end: "*/",
            has_preprocessor: false,
        }
    }

    /// Go language configuration
    pub fn go() -> Self {
        Self {
            name: "go",
            keywords: [
                // Control flow
                "if",
                "else",
                "for",
                "switch",
                "case",
                "default",
                "break",
                "continue",
                "goto",
                "fallthrough",
                "return",
                "select",
                // Declarations
                "var",
                "const",
                "type",
                "func",
                "package",
                "import",
                // Concurrency
                "go",
                "chan",
                "defer",
                "range",
                // Other
                "struct",
                "interface",
                "map",
                "make",
                "new",
                "nil",
            ]
            .into_iter()
            .collect(),
            types: [
                "bool",
                "string",
                "int",
                "int8",
                "int16",
                "int32",
                "int64",
                "uint",
                "uint8",
                "uint16",
                "uint32",
                "uint64",
                "uintptr",
                "byte",
                "rune",
                "float32",
                "float64",
                "complex64",
                "complex128",
                "error",
                "any",
            ]
            .into_iter()
            .collect(),
            line_comment: "//",
            block_comment_start: "/*",
            block_comment_end: "*/",
            has_preprocessor: false,
        }
    }

    /// JavaScript/TypeScript language configuration
    pub fn javascript() -> Self {
        Self {
            name: "javascript",
            keywords: [
                // Control flow
                "if",
                "else",
                "for",
                "while",
                "do",
                "switch",
                "case",
                "default",
                "break",
                "continue",
                "return",
                "throw",
                "try",
                "catch",
                "finally",
                // Declarations
                "var",
                "let",
                "const",
                "function",
                "class",
                "extends",
                "static",
                // Modules
                "import",
                "export",
                "from",
                "as",
                "default",
                // Async
                "async",
                "await",
                "yield",
                // Operators
                "new",
                "delete",
                "typeof",
                "instanceof",
                "in",
                "of",
                // Other
                "this",
                "super",
                "with",
                "debugger",
            ]
            .into_iter()
            .collect(),
            types: [
                "undefined",
                "null",
                "boolean",
                "number",
                "string",
                "symbol",
                "bigint",
                "object",
                "Array",
                "Object",
                "Function",
                "Promise",
                "Map",
                "Set",
                "void",
                "any",
                "never",
                "unknown", // TypeScript types
            ]
            .into_iter()
            .collect(),
            line_comment: "//",
            block_comment_start: "/*",
            block_comment_end: "*/",
            has_preprocessor: false,
        }
    }
}

// =============================================================================
// LEXER ITERATOR
// =============================================================================

/// Pull-parser lexer that yields tokens lazily
pub struct Lexer<'a> {
    input: &'a str,
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    pos: usize,
    state: LexerState,
    config: &'a LanguageConfig,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer with Normal state
    pub fn new(input: &'a str, config: &'a LanguageConfig) -> Self {
        Self {
            input,
            chars: input.char_indices().peekable(),
            pos: 0,
            state: LexerState::Normal,
            config,
        }
    }

    /// Create a new lexer starting in a specific state (for mid-file restart)
    pub fn with_state(input: &'a str, config: &'a LanguageConfig, state: LexerState) -> Self {
        Self {
            input,
            chars: input.char_indices().peekable(),
            pos: 0,
            state,
            config,
        }
    }

    /// Get current lexer state (for caching at line boundaries)
    pub fn state(&self) -> LexerState {
        self.state
    }

    /// Peek the next character without consuming
    fn peek(&mut self) -> Option<char> {
        self.chars.peek().map(|&(_, ch)| ch)
    }

    /// Peek nth character without consuming
    fn peek_nth(&self, n: usize) -> Option<char> {
        self.input[self.pos..].chars().nth(n)
    }

    /// Consume and return next character
    fn advance(&mut self) -> Option<char> {
        self.chars.next().map(|(idx, ch)| {
            self.pos = idx + ch.len_utf8();
            ch
        })
    }

    /// Consume characters while predicate is true, return the slice
    fn take_while<F>(&mut self, start: usize, predicate: F) -> &'a str
    where
        F: Fn(char) -> bool,
    {
        while let Some(ch) = self.peek() {
            if predicate(ch) {
                self.advance();
            } else {
                break;
            }
        }
        &self.input[start..self.pos]
    }

    /// Check if input at current position starts with string
    fn starts_with(&self, s: &str) -> bool {
        self.input[self.pos..].starts_with(s)
    }

    /// Skip n bytes (for multi-char tokens)
    fn skip(&mut self, n: usize) {
        for _ in 0..n {
            self.advance();
        }
    }

    /// Lex a number (decimal, hex, binary, octal, float)
    fn lex_number(&mut self, start: usize) -> Token<'a> {
        // Check for special prefixes
        if self.starts_with("0x") || self.starts_with("0X") {
            self.skip(2);
            self.take_while(self.pos, |ch| ch.is_ascii_hexdigit() || ch == '_');
        } else if self.starts_with("0b") || self.starts_with("0B") {
            self.skip(2);
            self.take_while(self.pos, |ch| ch == '0' || ch == '1' || ch == '_');
        } else if self.starts_with("0o") || self.starts_with("0O") {
            self.skip(2);
            self.take_while(self.pos, |ch| ('0'..='7').contains(&ch) || ch == '_');
        } else {
            // Decimal
            self.take_while(self.pos, |ch| ch.is_ascii_digit() || ch == '_');

            // Float with decimal point
            if self.peek() == Some('.') {
                if let Some(next) = self.peek_nth(1) {
                    if next.is_ascii_digit() {
                        self.advance(); // consume '.'
                        self.take_while(self.pos, |ch| ch.is_ascii_digit() || ch == '_');
                    }
                }
            }

            // Exponent
            if matches!(self.peek(), Some('e') | Some('E')) {
                self.advance();
                if matches!(self.peek(), Some('+') | Some('-')) {
                    self.advance();
                }
                self.take_while(self.pos, |ch| ch.is_ascii_digit() || ch == '_');
            }
        }

        // Type suffix (u8, i32, f64, etc.)
        self.take_while(self.pos, |ch| ch.is_ascii_alphanumeric());

        Token::new(TokenKind::Number, &self.input[start..self.pos], start)
    }

    /// Lex a string literal
    fn lex_string(&mut self, start: usize, quote: char) -> Token<'a> {
        self.advance(); // consume opening quote
        while let Some(ch) = self.advance() {
            if ch == '\\' {
                self.advance(); // skip escaped char
            } else if ch == quote {
                return Token::new(TokenKind::String, &self.input[start..self.pos], start);
            } else if ch == '\n' {
                // Unterminated string on this line
                self.state = LexerState::InString(quote);
                return Token::new(TokenKind::String, &self.input[start..self.pos], start);
            }
        }
        // EOF - unterminated
        self.state = LexerState::InString(quote);
        Token::new(TokenKind::String, &self.input[start..self.pos], start)
    }

    /// Lex a character literal
    fn lex_char(&mut self, start: usize) -> Token<'a> {
        self.advance(); // consume '
        if self.peek() == Some('\\') {
            self.advance(); // backslash
            self.advance(); // escaped char
        } else {
            self.advance(); // single char
        }
        if self.peek() == Some('\'') {
            self.advance(); // closing '
        }
        Token::new(TokenKind::Char, &self.input[start..self.pos], start)
    }

    /// Lex a line comment
    fn lex_line_comment(&mut self, start: usize) -> Token<'a> {
        self.take_while(start, |ch| ch != '\n');
        Token::new(TokenKind::Comment, &self.input[start..self.pos], start)
    }

    /// Lex a block comment (may span lines)
    fn lex_block_comment(&mut self, start: usize) -> Token<'a> {
        let end_marker = self.config.block_comment_end;
        let start_marker = self.config.block_comment_start;

        // Skip opening marker
        self.skip(start_marker.len());
        let mut depth = 1;

        while depth > 0 {
            if self.starts_with(end_marker) {
                self.skip(end_marker.len());
                depth -= 1;
            } else if self.starts_with(start_marker) {
                self.skip(start_marker.len());
                depth += 1;
            } else if self.advance().is_none() {
                // EOF in comment
                self.state = LexerState::InBlockComment;
                break;
            }
        }

        if depth == 0 {
            self.state = LexerState::Normal;
        } else {
            self.state = LexerState::InBlockComment;
        }

        Token::new(TokenKind::Comment, &self.input[start..self.pos], start)
    }

    /// Continue lexing a block comment from previous line
    fn continue_block_comment(&mut self, start: usize) -> Token<'a> {
        let end_marker = self.config.block_comment_end;

        while !self.starts_with(end_marker) {
            if self.advance().is_none() {
                // Still in comment at EOF/EOL
                return Token::new(TokenKind::Comment, &self.input[start..self.pos], start);
            }
        }

        self.skip(end_marker.len());
        self.state = LexerState::Normal;
        Token::new(TokenKind::Comment, &self.input[start..self.pos], start)
    }

    /// Continue lexing a string from previous line
    fn continue_string(&mut self, start: usize, quote: char) -> Token<'a> {
        while let Some(ch) = self.advance() {
            if ch == '\\' {
                self.advance();
            } else if ch == quote {
                self.state = LexerState::Normal;
                return Token::new(TokenKind::String, &self.input[start..self.pos], start);
            }
        }
        // Still in string
        Token::new(TokenKind::String, &self.input[start..self.pos], start)
    }

    /// Lex an identifier or keyword
    fn lex_identifier(&mut self, start: usize) -> Token<'a> {
        let text = self.take_while(start, |ch| ch.is_ascii_alphanumeric() || ch == '_');

        let kind = if self.config.keywords.contains(text) {
            TokenKind::Keyword
        } else if self.config.types.contains(text) {
            TokenKind::Type
        } else {
            TokenKind::Identifier
        };

        Token::new(kind, text, start)
    }

    /// Lex preprocessor directive
    fn lex_preprocessor(&mut self, start: usize) -> Token<'a> {
        // Take #directive and rest of line (except continuation)
        self.take_while(start, |ch| ch != '\n');
        Token::new(TokenKind::Preprocessor, &self.input[start..self.pos], start)
    }

    /// Lex an operator (single or multi-char)
    fn lex_operator(&mut self, start: usize, first: char) -> Token<'a> {
        self.advance(); // consume first char

        // Check for multi-char operators
        match first {
            '<' if self.peek() == Some('<') => {
                self.advance();
            }
            '>' if self.peek() == Some('>') => {
                self.advance();
            }
            '*' if self.peek() == Some('*') => {
                self.advance();
            } // **
            '-' if self.peek() == Some('>') => {
                self.advance();
            } // ->
            '=' if self.peek() == Some('=') => {
                self.advance();
            } // ==
            '!' if self.peek() == Some('=') => {
                self.advance();
            } // !=
            '<' if self.peek() == Some('=') => {
                self.advance();
            } // <=
            '>' if self.peek() == Some('=') => {
                self.advance();
            } // >=
            '&' if self.peek() == Some('&') => {
                self.advance();
            } // &&
            '|' if self.peek() == Some('|') => {
                self.advance();
            } // ||
            '+' if self.peek() == Some('=') => {
                self.advance();
            } // +=
            '-' if self.peek() == Some('=') => {
                self.advance();
            } // -=
            '*' if self.peek() == Some('=') => {
                self.advance();
            } // *=
            '/' if self.peek() == Some('=') => {
                self.advance();
            } // /=
            _ => {}
        }

        Token::new(TokenKind::Operator, &self.input[start..self.pos], start)
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        // Handle continuation from previous state
        let start = self.pos;

        match self.state {
            LexerState::InBlockComment => {
                if start < self.input.len() {
                    return Some(self.continue_block_comment(start));
                }
                return None;
            }
            LexerState::InString(quote) => {
                if start < self.input.len() {
                    return Some(self.continue_string(start, quote));
                }
                return None;
            }
            LexerState::Normal => {}
        }

        let (_, ch) = self.chars.peek()?.clone();

        Some(match ch {
            // Whitespace
            ' ' | '\t' | '\r' => {
                let text = self.take_while(start, |c| matches!(c, ' ' | '\t' | '\r'));
                Token::new(TokenKind::Whitespace, text, start)
            }

            // Newline
            '\n' => {
                self.advance();
                Token::new(TokenKind::Newline, &self.input[start..self.pos], start)
            }

            // Numbers
            '0'..='9' => self.lex_number(start),

            // Strings
            '"' => self.lex_string(start, '"'),

            // Chars
            '\'' => self.lex_char(start),

            // Comments or operators
            '/' => {
                if self.peek_nth(1) == Some('/') {
                    self.lex_line_comment(start)
                } else if self.starts_with(self.config.block_comment_start) {
                    self.lex_block_comment(start)
                } else {
                    self.lex_operator(start, ch)
                }
            }

            // Python/shell comments
            '#' if self.config.line_comment == "#" => self.lex_line_comment(start),

            // C preprocessor
            '#' if self.config.has_preprocessor => self.lex_preprocessor(start),

            // Identifiers
            'a'..='z' | 'A'..='Z' | '_' => self.lex_identifier(start),

            // Delimiters
            '(' | ')' | '{' | '}' | '[' | ']' => {
                self.advance();
                Token::new(TokenKind::Delimiter, &self.input[start..self.pos], start)
            }

            // Punctuation
            ';' | ',' | '.' | ':' | '@' | '`' => {
                self.advance();
                // Handle :: for Rust
                if ch == ':' && self.peek() == Some(':') {
                    self.advance();
                }
                Token::new(TokenKind::Punctuation, &self.input[start..self.pos], start)
            }

            // Operators
            '+' | '-' | '*' | '%' | '=' | '<' | '>' | '!' | '&' | '|' | '^' | '~' | '?' => {
                self.lex_operator(start, ch)
            }

            // Unknown
            _ => {
                self.advance();
                Token::new(TokenKind::Unknown, &self.input[start..self.pos], start)
            }
        })
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lex_numbers() {
        let config = LanguageConfig::rust();
        let input = "123 0xFF 0b101 3.14 1_000_000 1e10";
        let tokens: Vec<_> = Lexer::new(input, &config)
            .filter(|t| t.kind != TokenKind::Whitespace)
            .collect();

        assert_eq!(tokens.len(), 6);
        assert!(tokens.iter().all(|t| t.kind == TokenKind::Number));
        assert_eq!(tokens[0].text, "123");
        assert_eq!(tokens[1].text, "0xFF");
        assert_eq!(tokens[2].text, "0b101");
        assert_eq!(tokens[3].text, "3.14");
        assert_eq!(tokens[4].text, "1_000_000");
        assert_eq!(tokens[5].text, "1e10");
    }

    #[test]
    fn test_lex_keywords_and_identifiers() {
        let config = LanguageConfig::rust();
        let input = "fn main let x";
        let tokens: Vec<_> = Lexer::new(input, &config)
            .filter(|t| t.kind != TokenKind::Whitespace)
            .collect();

        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].kind, TokenKind::Keyword);
        assert_eq!(tokens[0].text, "fn");
        assert_eq!(tokens[1].kind, TokenKind::Identifier);
        assert_eq!(tokens[1].text, "main");
        assert_eq!(tokens[2].kind, TokenKind::Keyword);
        assert_eq!(tokens[2].text, "let");
        assert_eq!(tokens[3].kind, TokenKind::Identifier);
        assert_eq!(tokens[3].text, "x");
    }

    #[test]
    fn test_lex_strings_and_chars() {
        let config = LanguageConfig::rust();
        let input = r#""hello" 'a' "esc\"ape""#;
        let tokens: Vec<_> = Lexer::new(input, &config)
            .filter(|t| t.kind != TokenKind::Whitespace)
            .collect();

        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].kind, TokenKind::String);
        assert_eq!(tokens[0].text, "\"hello\"");
        assert_eq!(tokens[1].kind, TokenKind::Char);
        assert_eq!(tokens[1].text, "'a'");
        assert_eq!(tokens[2].kind, TokenKind::String);
    }

    #[test]
    fn test_lex_comments() {
        let config = LanguageConfig::rust();
        let input = "x // comment\ny /* block */";
        let tokens: Vec<_> = Lexer::new(input, &config).collect();

        let comments: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Comment)
            .collect();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].text, "// comment");
        assert_eq!(comments[1].text, "/* block */");
    }

    #[test]
    fn test_lex_operators() {
        let config = LanguageConfig::rust();
        let input = "+ - * / % << >> ** -> ==";
        let tokens: Vec<_> = Lexer::new(input, &config)
            .filter(|t| t.kind != TokenKind::Whitespace)
            .collect();

        assert_eq!(tokens.len(), 10);
        assert!(tokens.iter().all(|t| t.kind == TokenKind::Operator));
    }

    #[test]
    fn test_c_preprocessor() {
        let config = LanguageConfig::c();
        let input = "#include <stdio.h>\nint main";
        let tokens: Vec<_> = Lexer::new(input, &config)
            .filter(|t| !matches!(t.kind, TokenKind::Whitespace | TokenKind::Newline))
            .collect();

        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].kind, TokenKind::Preprocessor);
        assert!(tokens[0].text.starts_with("#include"));
    }

    #[test]
    fn test_token_positions() {
        let config = LanguageConfig::rust();
        let input = "let x = 5;";
        let tokens: Vec<_> = Lexer::new(input, &config)
            .filter(|t| t.kind != TokenKind::Whitespace)
            .collect();

        // Verify positions allow slicing back into original
        for token in &tokens {
            assert_eq!(&input[token.start..token.end()], token.text);
        }
    }
}
