//! Terminal Diagnostic Parsing
//!
//! Parses compiler output from GCC, Rust, and Make to extract errors,
//! warnings, and file locations. This provides LSP-like features for
//! build output without requiring a language server.

// DiagnosticParser is used by event_handler.rs for shell command output

use std::path::PathBuf;

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
// DIAGNOSTIC PARSER
// =============================================================================

/// Parser for extracting diagnostics from build output
#[derive(Debug, Default)]
pub struct DiagnosticParser {
    /// Accumulated diagnostics
    pub diagnostics: Vec<Diagnostic>,
    /// Current partial diagnostic being built
    current: Option<Diagnostic>,
}

impl DiagnosticParser {
    /// Create a new parser
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a line of output
    pub fn parse_line(&mut self, line: &str) {
        // Try each parser format
        if let Some(diag) = Self::parse_gcc(line) {
            self.finish_current();
            self.current = Some(diag);
        } else if let Some(diag) = Self::parse_rust(line) {
            self.finish_current();
            self.current = Some(diag);
        } else if let Some(diag) = Self::parse_make(line) {
            self.finish_current();
            self.current = Some(diag);
        } else if let Some(ref mut current) = self.current {
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

    /// Finish the current diagnostic and push to list
    fn finish_current(&mut self) {
        if let Some(diag) = self.current.take() {
            self.diagnostics.push(diag);
        }
    }

    /// Parse GCC/Clang error format: file:line:column: severity: message
    fn parse_gcc(line: &str) -> Option<Diagnostic> {
        // Pattern: file.c:10:5: error: message
        // Or: file.c:10: error: message (no column)

        // Find the first colon to get the file
        let first_colon = line.find(':')?;
        let file = line[..first_colon].trim();
        if file.is_empty() {
            return None;
        }

        let rest = &line[first_colon + 1..];

        // Find line number
        let second_colon = rest.find(':')?;
        let line_num: usize = rest[..second_colon].trim().parse().ok()?;

        let rest = &rest[second_colon + 1..];

        // Check if next part is column (a number) or severity
        let third_colon = rest.find(':')?;
        let third_part = rest[..third_colon].trim();

        let (column, rest) = if let Ok(col) = third_part.parse::<usize>() {
            // Has column: file:line:col: severity: message
            (Some(col), &rest[third_colon + 1..])
        } else {
            // No column, third_part is the severity
            (None, rest)
        };

        // Find severity
        let severity_colon = rest.find(':')?;
        let severity_str = rest[..severity_colon].trim().to_lowercase();
        let severity = match severity_str.as_str() {
            "error" => DiagnosticSeverity::Error,
            "warning" => DiagnosticSeverity::Warning,
            "note" => DiagnosticSeverity::Note,
            _ => return None,
        };

        let message = rest[severity_colon + 1..].trim();

        let mut diag = Diagnostic::new(severity, file, line_num, message);
        if let Some(col) = column {
            diag = diag.with_column(col);
        }
        Some(diag)
    }

    /// Parse Rust compiler error format
    fn parse_rust(line: &str) -> Option<Diagnostic> {
        // Pattern: error[E0308]: message
        //    --> src/main.rs:10:5
        if line.starts_with("error") || line.starts_with("warning") {
            let is_error = line.starts_with("error");
            let severity = if is_error {
                DiagnosticSeverity::Error
            } else {
                DiagnosticSeverity::Warning
            };

            // Extract error code like [E0308]
            let code = if let Some(start) = line.find('[') {
                if let Some(end) = line.find(']') {
                    Some(line[start + 1..end].to_string())
                } else {
                    None
                }
            } else {
                None
            };

            // Extract message after ]: or :
            let message = if let Some(idx) = line.find("]: ") {
                line[idx + 3..].trim()
            } else if let Some(idx) = line.find(": ") {
                line[idx + 2..].trim()
            } else {
                ""
            };

            // Create diagnostic with placeholder file (will be updated by --> line)
            let mut diag = Diagnostic::new(severity, "", 0, message);
            if let Some(c) = code {
                diag = diag.with_code(c);
            }
            return Some(diag);
        }

        // Pattern:    --> src/main.rs:10:5
        if line.trim_start().starts_with("-->") {
            let location = line.trim_start().trim_start_matches("--> ");
            let parts: Vec<&str> = location.split(':').collect();
            if parts.len() >= 2 {
                let file = parts[0];
                if let Ok(line_num) = parts[1].parse::<usize>() {
                    let column = parts.get(2).and_then(|s| s.parse::<usize>().ok());
                    let mut diag = Diagnostic::new(
                        DiagnosticSeverity::Error, // Will be corrected if we have context
                        file,
                        line_num,
                        "",
                    );
                    if let Some(col) = column {
                        diag = diag.with_column(col);
                    }
                    return Some(diag);
                }
            }
        }

        None
    }

    /// Parse Make error format
    fn parse_make(line: &str) -> Option<Diagnostic> {
        // Pattern: make: *** [target] Error N
        // Or: make[N]: *** [target] Error N
        if line.contains("***") && line.contains("Error") {
            let message = line.trim();
            return Some(Diagnostic::new(
                DiagnosticSeverity::Error,
                "Makefile",
                0,
                message,
            ));
        }

        // Pattern: Makefile:10: recipe for target 'foo' failed
        if line.contains("Makefile:") && line.contains("recipe for target") {
            let parts: Vec<&str> = line.splitn(3, ':').collect();
            if parts.len() >= 2 {
                if let Ok(line_num) = parts[1].trim().parse::<usize>() {
                    return Some(Diagnostic::new(
                        DiagnosticSeverity::Error,
                        "Makefile",
                        line_num,
                        match parts.get(2) {
                            Some(s) => s.trim(),
                            None => "",
                        },
                    ));
                }
            }
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

    #[test]
    fn test_parse_gcc_error() {
        let mut parser = DiagnosticParser::new();
        parser.parse_line("main.c:10:5: error: expected ';' before '}'");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].is_error());
        if let Some(path_str) = diags[0].file.to_str() {
            assert_eq!(path_str, "main.c");
        } else {
            panic!("Invalid path");
        }
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
    }

    #[test]
    fn test_parse_rust_error() {
        let mut parser = DiagnosticParser::new();
        parser.parse_line("error[E0308]: mismatched types");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].is_error());
        assert_eq!(diags[0].code, Some("E0308".to_string()));
        assert!(diags[0].message.contains("mismatched types"));
    }

    #[test]
    fn test_parse_rust_location() {
        let mut parser = DiagnosticParser::new();
        parser.parse_line("  --> src/main.rs:42:10");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        if let Some(path_str) = diags[0].file.to_str() {
            assert_eq!(path_str, "src/main.rs");
        } else {
            panic!("Invalid path");
        }
        assert_eq!(diags[0].line, 42);
        assert_eq!(diags[0].column, Some(10));
    }

    #[test]
    fn test_parse_make_error() {
        let mut parser = DiagnosticParser::new();
        parser.parse_line("make: *** [all] Error 2");
        let diags = parser.finish();

        assert_eq!(diags.len(), 1);
        assert!(diags[0].is_error());
        if let Some(path_str) = diags[0].file.to_str() {
            assert_eq!(path_str, "Makefile");
        } else {
            panic!("Invalid path");
        }
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
