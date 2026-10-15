//! Print and message-line commands.
use crate::core::app::EditorApp;
use crate::core::dispatcher::DispatchResult;
use crate::core::print::{print_active_buffer, PrintersBackend};

/// Print the active buffer on the default printer (`print` command).
///
/// Uses the single cross-platform print path in [`crate::core::print`];
/// success is reported only after the spooler accepts the job.
pub fn print(app: &mut EditorApp, _count: usize) -> DispatchResult {
    let backend = PrintersBackend::new();
    print_active_buffer(app, |buffer| buffer.display_name(), &backend)
}
