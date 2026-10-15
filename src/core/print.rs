//! Cross-platform printing support for the editor.
//!
//! Printing is delegated to the `printers` crate, which talks to CUPS on
//! Linux and macOS and to WinSpool on Windows: one print path for every
//! platform (N-267). Buffer content is handed to the spooler as raw bytes;
//! the buffer is never stringified for printing (N-193).

use std::fmt;

use printers::common::base::errors::{PrintersError, PrintersFailure};
use printers::common::base::job::PrinterJobOptions;
use printers::common::base::printer::{Printer, PrinterState};
use printers::common::converters::Converter;

use crate::core::app::EditorApp;
use crate::core::buffer::Buffer;
use crate::core::dispatcher::DispatchResult;

/// A print job: a title for the spooler queue plus the raw bytes to print.
pub struct PrintJob<'a> {
    /// Job title shown in the print queue.
    pub job_name: String,
    /// Raw content bytes handed to the spooler (never re-encoded).
    pub content: &'a [u8],
}

/// Typed print failure: every failure path is representable, so no failure
/// can be silently reported as success (N-193).
#[derive(Debug)]
pub enum PrintError {
    /// No printers are installed or visible on this system.
    NoPrinters,
    /// A printer was named (or the default resolved) but cannot take the job.
    PrinterUnavailable { name: String, reason: String },
    /// The spooler rejected the job.
    PrintFailed { printer: String, reason: String },
}

impl fmt::Display for PrintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrintError::NoPrinters => write!(f, "no printers available"),
            PrintError::PrinterUnavailable { name, reason } => {
                write!(f, "printer '{name}' unavailable: {reason}")
            }
            PrintError::PrintFailed { printer, reason } => {
                write!(f, "print to '{printer}' failed: {reason}")
            }
        }
    }
}

impl std::error::Error for PrintError {}

/// A print destination. This trait is the test seam: production code uses
/// [`PrintersBackend`]; tests substitute [`MockPrinter`].
pub trait PrintBackend {
    /// Submit `job` to the spooler and return the spooler-assigned job id.
    /// A returned `Ok` means the spooler accepted the job; any failure is
    /// an `Err` — success is reported only after a successful print.
    fn print(&self, job: &PrintJob) -> Result<u64, PrintError>;
}

/// The single cross-platform print backend, built on the `printers` crate.
pub struct PrintersBackend {
    /// Print to this printer instead of the system default.
    pub printer_name: Option<String>,
}

impl PrintersBackend {
    /// Backend targeting the system default printer.
    pub fn new() -> Self {
        Self { printer_name: None }
    }

    /// Backend targeting a named printer.
    pub fn with_printer(name: impl Into<String>) -> Self {
        Self {
            printer_name: Some(name.into()),
        }
    }

    /// Resolve the destination printer or produce a typed error.
    fn resolve_printer(&self) -> Result<Printer, PrintError> {
        match &self.printer_name {
            Some(name) => printers::get_printer_by_name(name).ok_or_else(|| {
                PrintError::PrinterUnavailable {
                    name: name.clone(),
                    reason: "no such printer".to_string(),
                }
            }),
            None => printers::get_default_printer().ok_or(PrintError::NoPrinters),
        }
    }
}

impl Default for PrintersBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl PrintBackend for PrintersBackend {
    fn print(&self, job: &PrintJob) -> Result<u64, PrintError> {
        let printer = self.resolve_printer()?;
        // A queued job on a paused/offline printer would sit unprinted while
        // we reported success; fail fast with a typed reason instead.
        if matches!(printer.state, PrinterState::OFFLINE | PrinterState::PAUSED) {
            return Err(PrintError::PrinterUnavailable {
                name: printer.name.clone(),
                reason: format!("printer is {:?}", printer.state),
            });
        }
        // Bytes go straight to the spooler; the crate spools via its own
        // temp file on Unix, so no temp file is needed on our side.
        // `Converter::None` keeps the bytes untouched (no ghostscript pass).
        let options = PrinterJobOptions {
            name: Some(job.job_name.as_str()),
            raw_properties: &[],
            converter: Converter::None,
        };
        printer
            .print(job.content, options)
            .map_err(|err| map_printers_error(&printer.name, err))
    }
}

/// Fold a `printers`-crate failure into the typed error model, preserving
/// the failure class and the spooler's message.
fn map_printers_error(printer_name: &str, err: PrintersError) -> PrintError {
    let kind = match err.failure {
        PrintersFailure::FileFailure => "spool file",
        PrintersFailure::PrintFailure => "spooler",
        PrintersFailure::ConverterFailure => "converter",
        PrintersFailure::JobFailure => "job",
        PrintersFailure::GenericFailure => "backend",
    };
    PrintError::PrintFailed {
        printer: printer_name.to_string(),
        reason: format!("{kind} error: {}", err.message),
    }
}

/// Raw bytes of a buffer's content, collected straight from the B-tree's
/// chunks: byte-exact, a single pass, and no whole-buffer `String`.
pub fn buffer_bytes(buffer: &Buffer) -> Vec<u8> {
    buffer.bytes()
}

/// Print the active window's buffer through `backend`.
///
/// This is the single orchestration shared by every print command: resolve
/// the active buffer, extract its bytes, submit the job, and report the
/// outcome as an editor message. `job_name` derives the spooler job title
/// from the buffer.
pub fn print_active_buffer(
    app: &mut EditorApp,
    job_name: impl FnOnce(&Buffer) -> String,
    backend: &dyn PrintBackend,
) -> DispatchResult {
    let active = app.active_window;
    let buffer_id = match app.windows.get(&active) {
        Some(window) => window.buffer_id,
        None => {
            app.message = Some("No active window".to_string());
            return DispatchResult::Success;
        }
    };
    let (name, content) = match app.buffers.get_mut(&buffer_id) {
        Some(buffer) => {
            let name = job_name(buffer);
            let bytes = buffer_bytes(buffer);
            (name, bytes)
        }
        None => {
            app.message = Some("No active buffer".to_string());
            return DispatchResult::Success;
        }
    };
    let job = PrintJob {
        job_name: name,
        content: &content,
    };
    match backend.print(&job) {
        Ok(job_id) => {
            app.message = Some(format!("Buffer sent to printer (job {job_id})"));
        }
        Err(err) => {
            app.message = Some(format!("Print failed: {err}"));
        }
    }
    DispatchResult::Success
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// In-memory backend recording submitted jobs: the test seam for
    /// print-job logic that must not touch a real spooler.
    pub struct MockPrinter {
        jobs: RefCell<Vec<(String, Vec<u8>)>>,
        next_id: RefCell<u64>,
        fail_reason: RefCell<Option<String>>,
    }

    impl MockPrinter {
        pub fn new() -> Self {
            Self {
                jobs: RefCell::new(Vec::new()),
                next_id: RefCell::new(1),
                fail_reason: RefCell::new(None),
            }
        }

        /// Make subsequent `print` calls fail with a spooler rejection.
        pub fn fail_with(&self, reason: &str) {
            *self.fail_reason.borrow_mut() = Some(reason.to_string());
        }

        pub fn job_count(&self) -> usize {
            self.jobs.borrow().len()
        }

        pub fn job(&self, index: usize) -> (String, Vec<u8>) {
            self.jobs.borrow()[index].clone()
        }
    }

    impl Default for MockPrinter {
        fn default() -> Self {
            Self::new()
        }
    }

    impl PrintBackend for MockPrinter {
        fn print(&self, job: &PrintJob) -> Result<u64, PrintError> {
            if let Some(reason) = self.fail_reason.borrow().clone() {
                return Err(PrintError::PrintFailed {
                    printer: "mock".to_string(),
                    reason,
                });
            }
            let id = *self.next_id.borrow();
            *self.next_id.borrow_mut() += 1;
            self.jobs
                .borrow_mut()
                .push((job.job_name.clone(), job.content.to_vec()));
            Ok(id)
        }
    }

    #[test]
    fn mock_records_job_name_and_bytes_exactly() {
        let printer = MockPrinter::new();
        let content: &[u8] = "Hello, 世界!\r\nline2\n".as_bytes();
        let job = PrintJob {
            job_name: "notes.txt".to_string(),
            content,
        };
        let id = printer.print(&job).expect("mock print should succeed");
        assert_eq!(id, 1);
        assert_eq!(printer.job_count(), 1);
        let (name, bytes) = printer.job(0);
        assert_eq!(name, "notes.txt");
        assert_eq!(bytes, content, "printed bytes must match the job exactly");
    }

    #[test]
    fn mock_failure_is_an_error_never_a_fake_success() {
        let printer = MockPrinter::new();
        printer.fail_with("paper jam");
        let job = PrintJob {
            job_name: "j".to_string(),
            content: b"x",
        };
        match printer.print(&job) {
            Err(PrintError::PrintFailed { printer, reason }) => {
                assert_eq!(printer, "mock");
                assert!(reason.contains("paper jam"), "reason: {reason}");
            }
            other => panic!("expected PrintFailed, got {other:?}"),
        }
        assert_eq!(
            printer.job_count(),
            0,
            "a failed job must not be recorded as printed"
        );
    }

    #[test]
    fn mock_hands_out_distinct_job_ids() {
        let printer = MockPrinter::new();
        let mk = |n: &str| PrintJob {
            job_name: n.to_string(),
            content: b"data",
        };
        assert_eq!(printer.print(&mk("a")).unwrap(), 1);
        assert_eq!(printer.print(&mk("b")).unwrap(), 2);
        assert_eq!(printer.job_count(), 2);
    }

    #[test]
    fn printers_error_mapping_preserves_printer_and_message() {
        for (failure, kind) in [
            (PrintersFailure::FileFailure, "spool file"),
            (PrintersFailure::PrintFailure, "spooler"),
            (PrintersFailure::ConverterFailure, "converter"),
            (PrintersFailure::JobFailure, "job"),
            (PrintersFailure::GenericFailure, "backend"),
        ] {
            let err = PrintersError {
                message: "cupsPrintFile returned 0".to_string(),
                failure,
                backtrace: std::backtrace::Backtrace::capture(),
            };
            let mapped = map_printers_error("OfficeJet", err);
            match mapped {
                PrintError::PrintFailed { printer, reason } => {
                    assert_eq!(printer, "OfficeJet");
                    assert!(
                        reason.contains(kind) && reason.contains("cupsPrintFile returned 0"),
                        "reason: {reason}"
                    );
                }
                other => panic!("expected PrintFailed, got {other:?}"),
            }
        }
    }

    #[test]
    fn print_error_display_is_human_readable() {
        assert_eq!(PrintError::NoPrinters.to_string(), "no printers available");
        let e = PrintError::PrinterUnavailable {
            name: "lp0".to_string(),
            reason: "printer is OFFLINE".to_string(),
        };
        assert_eq!(e.to_string(), "printer 'lp0' unavailable: printer is OFFLINE");
        let e = PrintError::PrintFailed {
            printer: "lp0".to_string(),
            reason: "spooler error: boom".to_string(),
        };
        assert_eq!(e.to_string(), "print to 'lp0' failed: spooler error: boom");
    }

    #[test]
    fn buffer_bytes_is_byte_exact() {
        let buf = Buffer::from_string("line1\nline2\r\nline3\u{1F600}\n");
        let bytes = buffer_bytes(&buf);
        assert_eq!(bytes, "line1\nline2\r\nline3\u{1F600}\n".as_bytes());
    }

    #[test]
    fn buffer_bytes_handles_empty_buffer() {
        let buf = Buffer::new();
        assert!(buffer_bytes(&buf).is_empty());
    }

    #[test]
    fn print_active_buffer_reports_failure_as_error_message() {
        // Failure must surface as a "Print failed" message, never as success.
        let mut app = EditorApp::new();
        let printer = MockPrinter::new();
        printer.fail_with("no toner");
        let result = print_active_buffer(&mut app, |b| b.display_name(), &printer);
        assert!(matches!(result, DispatchResult::Success));
        let msg = app.message.clone().expect("a message must be set");
        assert!(
            msg.starts_with("Print failed:"),
            "failure must not be reported as success, got: {msg}"
        );
        assert!(msg.contains("no toner"), "got: {msg}");
    }

    #[test]
    fn print_active_buffer_reports_success_with_job_id() {
        let mut app = EditorApp::new();
        let printer = MockPrinter::new();
        let result = print_active_buffer(&mut app, |b| b.display_name(), &printer);
        assert!(matches!(result, DispatchResult::Success));
        let msg = app.message.clone().expect("a message must be set");
        assert!(
            msg.starts_with("Buffer sent to printer (job "),
            "got: {msg}"
        );
        assert_eq!(printer.job_count(), 1);
    }
}
