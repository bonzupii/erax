//! Terminal diagnostic parsing (compiler output to locations).
//!
//! Parses compiler output from GCC, Rust, and Make to extract errors,
//! warnings, and file locations. This provides IDE-like features for
//! build output without requiring a language server.
//!
//! Each supported format is described by a real grammar (compiled regexes in
//! [`DiagnosticParser`]); no ad-hoc colon-index guessing is used, so paths
//! containing colons (e.g. Windows drive letters like `C:\...`) parse
//! correctly.
//!
//! Rust diagnostics arrive as two lines — a header (`error[E0308]: msg`) and
//! a location (`--> src/main.rs:10:5`) — which the parser assembles into ONE
//! [`Diagnostic`], preserving the header's severity (a warning stays a
//! warning). Duplicate diagnostics (same file/line/column/message) are merged
//! into one.

// DiagnosticParser is used by prompt.rs for shell command output

use std::collections::HashSet;
use std::hash::Hash;
use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;

// =============================================================================
// DIAGNOSTIC TYPES
// =============================================================================

/// Severity level of a diagnostic
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Note,
    Info,
}

/// A parsed diagnostic from compiler output
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Severity level
    pub severity: DiagnosticSeverity,
    /// File path (may be relative)
    pub file: PathBuf,
    /// Line number (1-indexed)
    pub line: usize,
    /// Column number (1-indexed, optional)
    pub column: Option<usize>,
    /// End line (for spans)
    pub end_line: Option<usize>,
    /// End column (for spans)
    pub end_column: Option<usize>,
    /// Error code (e.g., E0308 for Rust)
    pub code: Option<String>,
    /// The diagnostic message
    pub message: String,
    /// Additional context lines
    pub context: Vec<String>,
}

impl Diagnostic {
    /// Create a new diagnostic
    pub fn new(
        severity: DiagnosticSeverity,
        file: impl Into<PathBuf>,
        line: usize,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            file: file.into(),
            line,
            column: None,
            end_line: None,
            end_column: None,
            code: None,
            message: message.into(),
            context: Vec::new(),
        }
    }

    /// Set column
    pub fn with_column(mut self, col: usize) -> Self {
        self.column = Some(col);
        self
    }

    /// Set error code
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Add context line
    pub fn add_context(&mut self, line: impl Into<String>) {
        self.context.push(line.into());
    }

    /// Check if this is an error
    pub fn is_error(&self) -> bool {
        matches!(self.severity, DiagnosticSeverity::Error)
    }

    /// Check if this is a warning
    pub fn is_warning(&self) -> bool {
        matches!(self.severity, DiagnosticSeverity::Warning)
    }
}

// =============================================================================
// FORMAT GRAMMARS
// =============================================================================

/// GCC/Clang: `file:line:column: severity: message` (column optional).
///
/// The file group is lazy (`+?`) and anchored by the `:line:` that must
/// follow, so paths containing colons — Windows drive letters (`C:\...`),
/// for example — are captured whole instead of being split at the first
/// colon.
static GCC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<file>.+?):(?P<line>\d+):(?:(?P<col>\d+):)?\s*(?P<sev>(?i:error|warning|note|info))\s*:\s*(?P<msg>.*)$",
    )
    .expect("GCC diagnostic regex is static and valid")
});

/// Rust header: `error[E0308]: message` or `warning: message`.
static RUST_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<sev>error|warning)(?:\[(?P<code>[^\]:]+)\])?:\s*(?P<msg>.*)$")
        .expect("Rust header regex is static and valid")
});

/// Rust location: `--> src/main.rs:10:5` (column optional; drive-letter paths OK).
static RUST_LOCATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*-->\s*(?P<file>.+?):(?P<line>\d+)(?::(?P<col>\d+))?\s*$")
        .expect("Rust location regex is static and valid")
});

/// Make failure: `make: *** [target] Error 2` / `make[1]: *** [target] Error 2`.
static MAKE_FAIL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:.*\s)?make(?:\[\d+\])?: \*\*\* \[(?P<target>[^\]]*)\] Error (?P<code>\d+)\s*$")
        .expect("Make failure regex is static and valid")
});

/// Make recipe failure: `Makefile:10: recipe for target 'foo' failed`.
static MAKE_RECIPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<file>[Mm]akefile[^:]*):(?P<line>\d+):\s*recipe for target '(?P<target>[^']*)' failed\s*$")
        .expect("Make recipe regex is static and valid")
});

/// A parsed Rust `-->` location line, waiting to be attached to its header.
#[derive(Debug)]
struct RustLocation {
    file: PathBuf,
    line: usize,
    column: Option<usize>,
}

fn parse_severity_word(word: &str) -> Option<DiagnosticSeverity> {
    match word.to_ascii_lowercase().as_str() {
        "error" => Some(DiagnosticSeverity::Error),
        "warning" => Some(DiagnosticSeverity::Warning),
        "note" => Some(DiagnosticSeverity::Note),
        "info" => Some(DiagnosticSeverity::Info),
        _ => None,
    }
}

// =============================================================================
// DIAGNOSTIC PARSER
// =============================================================================

/// Key used to merge duplicate diagnostics: same file/line/column/message.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DiagnosticKey {
    file: PathBuf,
    line: usize,
    column: Option<usize>,
    message: String,
}

/// Parser for extracting diagnostics from build output
#[derive(Debug, Default)]
pub struct DiagnosticParser {
    /// Accumulated diagnostics
    pub diagnostics: Vec<Diagnostic>,
    /// Current partial diagnostic being built
    current: Option<Diagnostic>,
    /// True when `current` is a Rust header still waiting for its `-->`
    /// location line (N-191: the two halves merge into one record).
    awaiting_rust_location: bool,
    /// Keys of diagnostics already emitted; duplicates are merged (N-192 queue).
    seen: HashSet<DiagnosticKey>,
}

impl DiagnosticParser {
    /// Create a new parser
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a line of output
    pub fn parse_line(&mut self, line: &str) {
        if let Some(diag) = Self::parse_gcc(line) {
            self.finish_current();
            self.current = Some(diag);
            self.awaiting_rust_location = false;
        } else if let Some(diag) = Self::parse_rust_header(line) {
            // A new Rust header starts a new diagnostic; its `-->` location
            // line (if any) is attached below instead of becoming a second
            // record.
            self.finish_current();
            self.current = Some(diag);
            self.awaiting_rust_location = true;
        } else if let Some(loc) = Self::parse_rust_location(line) {
            if self.awaiting_rust_location {
                if let Some(current) = self.current.as_mut() {
                    current.file = loc.file;
                    current.line = loc.line;
                    current.column = loc.column;
                }
                self.awaiting_rust_location = false;
            } else if let Some(current) = self.current.as_mut() {
                // A secondary location (notes, multi-span errors) is context
                // for the diagnostic it belongs to, not a new record.
                if !line.trim().is_empty() {
                    current.add_context(line);
                }
            } else {
                // A location line with no header: keep it as a diagnostic
                // rather than dropping information.
                let mut diag = Diagnostic::new(DiagnosticSeverity::Error, loc.file, loc.line, "");
                diag.column = loc.column;
                self.current = Some(diag);
            }
        } else if let Some(diag) = Self::parse_make(line) {
            self.finish_current();
            self.current = Some(diag);
            self.awaiting_rust_location = false;
        } else if let Some(current) = self.current.as_mut() {
            // Add as context to current diagnostic
            if !line.trim().is_empty() {
                current.add_context(line);
            }
        }
    }

    /// Finish parsing and return all diagnostics
    pub fn finish(mut self) -> Vec<Diagnostic> {
        self.finish_current();
        self.diagnostics
    }

    /// Finish the current diagnostic and push to list, merging duplicates
    fn finish_current(&mut self) {
        if let Some(diag) = self.current.take() {
            self.awaiting_rust_location = false;
            let key = DiagnosticKey {
                file: diag.file.clone(),
                line: diag.line,
                column: diag.column,
                message: diag.message.clone(),
            };
            if self.seen.insert(key) {
                self.diagnostics.push(diag);
            }
        }
    }

    /// Parse GCC/Clang error format: `file:line:column: severity: message`
    fn parse_gcc(line: &str) -> Option<Diagnostic> {
        let caps = GCC_RE.captures(line)?;
        let file = caps.name("file")?.as_str().trim();
        if file.is_empty() {
            return None;
        }
        let line_num: usize = caps.name("line")?.as_str().parse().ok()?;
        let column: Option<usize> = caps
            .name("col")
            .map(|m| m.as_str().parse().ok())
            .unwrap_or(None);
        let severity = parse_severity_word(caps.name("sev")?.as_str())?;
        let message = caps.name("msg")?.as_str().trim();

        let mut diag = Diagnostic::new(severity, file, line_num, message);
        diag.column = column;
        Some(diag)
    }

    /// Parse a Rust compiler header line: `error[E0308]: message`.
    ///
    /// Returns a diagnostic with severity, code, and message filled in; the
    /// file/line/column are attached when the following `-->` location line
    /// is parsed, so one error yields one diagnostic (N-191).
    fn parse_rust_header(line: &str) -> Option<Diagnostic> {
        let caps = RUST_HEADER_RE.captures(line)?;
        let severity = match caps.name("sev")?.as_str() {
            "error" => DiagnosticSeverity::Error,
            _ => DiagnosticSeverity::Warning,
        };
        let message = caps.name("msg")?.as_str().trim();

        let mut diag = Diagnostic::new(severity, "", 0, message);
        if let Some(code) = caps.name("code") {
            diag = diag.with_code(code.as_str());
        }
        Some(diag)
    }

    /// Parse a Rust location line: `--> src/main.rs:10:5`.
    fn parse_rust_location(line: &str) -> Option<RustLocation> {
        let caps = RUST_LOCATION_RE.captures(line)?;
        let file = caps.name("file")?.as_str().trim();
        if file.is_empty() {
            return None;
        }
        let line_num: usize = caps.name("line")?.as_str().parse().ok()?;
        let column: Option<usize> = caps
            .name("col")
            .map(|m| m.as_str().parse().ok())
            .unwrap_or(None);
        Some(RustLocation {
            file: PathBuf::from(file),
            line: line_num,
            column,
        })
    }

    /// Parse Make error formats.
    fn parse_make(line: &str) -> Option<Diagnostic> {
        if let Some(caps) = MAKE_FAIL_RE.captures(line) {
            let target = caps.name("target").map_or("", |m| m.as_str());
            let code = caps.name("code").map_or("", |m| m.as_str());
            return Some(Diagnostic::new(
                DiagnosticSeverity::Error,
                "Makefile",
                0,
                format!("make: *** [{target}] Error {code}"),
            ));
        }

        if let Some(caps) = MAKE_RECIPE_RE.captures(line) {
            let line_num: usize = caps.name("line")?.as_str().parse().ok()?;
            let target = caps.name("target").map_or("", |m| m.as_str());
            return Some(Diagnostic::new(
                DiagnosticSeverity::Error,
                "Makefile",
                line_num,
                format!("recipe for target '{target}' failed"),
            ));
        }

        None
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn file_str(diag: &Diagnostic) -> &str {
        diag.file.to_str().expect("test paths are valid UTF-8")
    }

    #[test]
    fn test_parse_gcc_error() {
        let mut parser = DiagnosticParser::new();
        parser.parse_line("main.c:10:5: error: expected ';' before '}'");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].is_error());
        assert_eq!(file_str(&diags[0]), "main.c");
        assert_eq!(diags[0].line, 10);
        assert_eq!(diags[0].column, Some(5));
        assert!(diags[0].message.contains("expected ';'"));
    }

    #[test]
    fn test_parse_gcc_warning() {
        let mut parser = DiagnosticParser::new();
        parser.parse_line("foo.c:25: warning: unused variable 'x'");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].is_warning());
        assert_eq!(diags[0].line, 25);
        assert_eq!(diags[0].column, None);
    }

    #[test]
    fn test_parse_gcc_windows_drive_path() {
        // N-192: a drive-letter colon must not split the path.
        let mut parser = DiagnosticParser::new();
        parser.parse_line(r"C:\projects\app\main.c:10:5: error: expected ';' before '}'");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].is_error());
        assert_eq!(file_str(&diags[0]), r"C:\projects\app\main.c");
        assert_eq!(diags[0].line, 10);
        assert_eq!(diags[0].column, Some(5));
    }

    #[test]
    fn test_parse_gcc_note_severity() {
        let mut parser = DiagnosticParser::new();
        parser.parse_line("foo.c:10:5: note: in expansion of macro 'FOO'");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Note);
    }

    #[test]
    fn test_parse_gcc_message_with_colons() {
        // Colons inside the message must not confuse the parser.
        let mut parser = DiagnosticParser::new();
        parser.parse_line("foo.c:3:1: error: expected ':' before '}' token");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        assert_eq!(file_str(&diags[0]), "foo.c");
        assert_eq!(diags[0].line, 3);
        assert!(diags[0].message.contains("expected ':'"));
    }

    #[test]
    fn test_parse_rust_error_merges_header_and_location() {
        // N-191: one diagnostic per error, assembled from both lines.
        let mut parser = DiagnosticParser::new();
        parser.parse_line("error[E0308]: mismatched types");
        parser.parse_line("  --> src/main.rs:42:10");
        parser.parse_line("   |");
        parser.parse_line("42 |     let x: i32 = \"hello\";");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1, "header + location must merge into one record");
        let d = &diags[0];
        assert!(d.is_error());
        assert_eq!(d.code, Some("E0308".to_string()));
        assert!(d.message.contains("mismatched types"));
        assert_eq!(file_str(d), "src/main.rs");
        assert_eq!(d.line, 42);
        assert_eq!(d.column, Some(10));
        assert!(
            d.context.iter().any(|c| c.contains("let x")),
            "source snippet lines become context"
        );
    }

    #[test]
    fn test_parse_rust_warning_keeps_severity() {
        // N-191: a warning merged with its location must stay a warning.
        let mut parser = DiagnosticParser::new();
        parser.parse_line("warning: unused variable: `x`");
        parser.parse_line(" --> src/main.rs:7:9");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].is_warning(), "warning severity must be preserved");
        assert!(!diags[0].is_error());
        assert_eq!(file_str(&diags[0]), "src/main.rs");
        assert_eq!(diags[0].line, 7);
        assert_eq!(diags[0].column, Some(9));
        assert!(diags[0].message.contains("unused variable"));
    }

    #[test]
    fn test_parse_rust_secondary_location_becomes_context() {
        let mut parser = DiagnosticParser::new();
        parser.parse_line("error[E0308]: mismatched types");
        parser.parse_line(" --> src/main.rs:10:5");
        parser.parse_line(" --> src/lib.rs:3:1");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1, "still one diagnostic per error");
        assert_eq!(file_str(&diags[0]), "src/main.rs");
        assert!(
            diags[0].context.iter().any(|c| c.contains("src/lib.rs")),
            "secondary location is kept as context"
        );
    }

    #[test]
    fn test_parse_rust_header_without_location() {
        let mut parser = DiagnosticParser::new();
        parser.parse_line("error: could not compile `foo` due to previous error");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].is_error());
        assert!(diags[0].message.contains("could not compile"));
    }

    #[test]
    fn test_parse_rust_lone_location_line() {
        // A location line with no preceding header is kept, not dropped.
        let mut parser = DiagnosticParser::new();
        parser.parse_line("  --> src/main.rs:42:10");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        assert_eq!(file_str(&diags[0]), "src/main.rs");
        assert_eq!(diags[0].line, 42);
        assert_eq!(diags[0].column, Some(10));
    }

    #[test]
    fn test_parse_rust_windows_drive_location() {
        let mut parser = DiagnosticParser::new();
        parser.parse_line("error[E0308]: mismatched types");
        parser.parse_line(r" --> C:\projects\app\src\main.rs:42:10");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        assert_eq!(file_str(&diags[0]), r"C:\projects\app\src\main.rs");
        assert_eq!(diags[0].line, 42);
        assert_eq!(diags[0].column, Some(10));
    }

    #[test]
    fn test_duplicate_diagnostics_merge() {
        let mut parser = DiagnosticParser::new();
        parser.parse_line("main.c:10:5: error: expected ';' before '}'");
        parser.parse_line("main.c:10:5: error: expected ';' before '}'");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1, "identical diagnostics merge into one");
    }

    #[test]
    fn test_distinct_diagnostics_do_not_merge() {
        let mut parser = DiagnosticParser::new();
        parser.parse_line("main.c:10:5: error: expected ';' before '}'");
        parser.parse_line("main.c:11:5: error: expected ';' before '}'");
        let diags = parser.finish();

        assert_eq!(diags.len(), 2, "different lines must not merge");
    }

    #[test]
    fn test_parse_make_error() {
        let mut parser = DiagnosticParser::new();
        parser.parse_line("make: *** [all] Error 2");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].is_error());
        assert_eq!(file_str(&diags[0]), "Makefile");
        assert!(diags[0].message.contains("Error 2"));
    }

    #[test]
    fn test_parse_make_recipe_failure() {
        let mut parser = DiagnosticParser::new();
        parser.parse_line("Makefile:10: recipe for target 'all' failed");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].is_error());
        assert_eq!(diags[0].line, 10);
    }

    #[test]
    fn test_diagnostic_builder() {
        let diag = Diagnostic::new(DiagnosticSeverity::Error, "test.c", 10, "test message")
            .with_column(5)
            .with_code("E001");

        assert_eq!(diag.column, Some(5));
        assert_eq!(diag.code, Some("E001".to_string()));
    }
}
