use crossterm;
use std::sync::atomic::{AtomicBool, Ordering}; // explicit dependency check

// static ORIGINAL_TERMIOS: std::sync::OnceLock<termios> = std::sync::OnceLock::new();

static TERMINAL_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// RAII wrapper for raw mode and alternate screen.
/// Enables raw mode and enters alternate screen on creation.
/// Restores terminal state on drop.
pub struct RawMode {
    original_hook: Option<Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Sync + Send + 'static>>,
    _initialized: bool,
}

impl RawMode {
    /// Enter raw mode and alternate screen
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Enable raw mode
        crossterm::terminal::enable_raw_mode()?;

        let mut stdout = std::io::stdout();
        // Enter alternate screen and hide cursor
        crossterm::execute!(
            stdout,
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture,
            crossterm::cursor::Hide,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        )?;

        TERMINAL_INITIALIZED.store(true, Ordering::SeqCst);

        // Set up panic hook to restore terminal before printing panic
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|info| {
            // Restore terminal settings
            let _ = restore_terminal();
            eprintln!("{}", info);
        }));

        Ok(Self {
            original_hook: Some(original_hook),
            _initialized: true,
        })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // Restore original terminal settings
        let _ = restore_terminal();

        // Restore original panic hook if we set one
        if let Some(hook) = self.original_hook.take() {
            std::panic::set_hook(hook);
        }
    }
}

// Helper function to restore terminal
fn restore_terminal() -> Result<(), Box<dyn std::error::Error>> {
    if TERMINAL_INITIALIZED.load(Ordering::SeqCst) {
        let mut stdout = std::io::stdout();

        // For VT100 and terminals without proper alternate screen support,
        // clear the screen and reset cursor before leaving alternate screen
        // to avoid leaving artifacts. Also show cursor.
        let _ = crossterm::execute!(
            stdout,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
            crossterm::cursor::MoveTo(0, 0),
            crossterm::cursor::Show,
            crossterm::event::DisableMouseCapture,
            crossterm::terminal::LeaveAlternateScreen
        );

        let _ = crossterm::terminal::disable_raw_mode();

        TERMINAL_INITIALIZED.store(false, Ordering::SeqCst);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // We can't easily test actual raw mode in unit tests without messing up the test runner's terminal.
    // But we can test that the struct can be created and dropped (mocking would be better but crossterm is hard to mock).
    // For now, we'll skip aggressive unit testing of the actual terminal syscalls and rely on manual verification
    // as stated in the plan.

    #[test]
    fn test_raw_mode_structure() {
        // Just verify the struct exists and compiles.
        // Actual behavior verified manually.
        let _ = RawMode {
            original_hook: None,
            _initialized: false,
        };
    }
}
