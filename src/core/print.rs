//! Printing support for the editor
//!
//! Provides a trait-based printing system using CUPS for Linux printing.

// Use standard error type instead of anyhow
use std::io::Write;
use std::process::{Command, Stdio};

/// Trait for print backends
pub trait PrintBackend {
    /// Print content with a job name
    fn print(&self, job_name: &str, content: &str) -> Result<(), Box<dyn std::error::Error>>;
}

/// CUPS print backend using the `lp` command
pub struct CupsBackend;

impl CupsBackend {
    /// Create a new CUPS backend
    pub fn new() -> Self {
        Self
    }

    /// Check if CUPS is available
    pub fn is_available() -> bool {
        match Command::new("which")
            .arg("lp")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
        {
            Ok(v) => v,
            Err(_) => false,
        }
    }
}

impl Default for CupsBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl PrintBackend for CupsBackend {
    fn print(&self, job_name: &str, content: &str) -> Result<(), Box<dyn std::error::Error>> {
        if !Self::is_available() {
            return Err("CUPS printing not available: 'lp' command not found".into());
        }

        // Sanitize job name to prevent argument injection
        // Filenames starting with '-' could be interpreted as flags
        let safe_job_name: String = if job_name.starts_with('-') {
            format!("./{}", job_name)
        } else {
            job_name.to_string()
        };

        let mut child = Command::new("lp")
            .arg("-t")
            .arg(&safe_job_name)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start lp command: {}", e))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(content.as_bytes())
                .map_err(|e| format!("Failed to write to lp stdin: {}", e))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|e| format!("Failed to wait for lp: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("Print failed: {}", stderr).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock printer for testing
    pub struct MockPrinter {
        pub jobs: std::cell::RefCell<Vec<(String, String)>>,
    }

    impl MockPrinter {
        pub fn new() -> Self {
            Self {
                jobs: std::cell::RefCell::new(Vec::new()),
            }
        }

        pub fn job_count(&self) -> usize {
            self.jobs.borrow().len()
        }
    }

    impl Default for MockPrinter {
        fn default() -> Self {
            Self::new()
        }
    }

    impl PrintBackend for MockPrinter {
        fn print(&self, job_name: &str, content: &str) -> Result<(), Box<dyn std::error::Error>> {
            self.jobs
                .borrow_mut()
                .push((job_name.to_string(), content.to_string()));
            Ok(())
        }
    }

    #[test]
    fn test_mock_printer() {
        let printer = MockPrinter::new();
        assert!(printer.print("test_job", "Hello, World!").is_ok());
        assert_eq!(printer.job_count(), 1);
    }

    #[test]
    fn test_cups_backend_creation() {
        let _backend = CupsBackend::new();
        // Just test that it can be created
    }
}
