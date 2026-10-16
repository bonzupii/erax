//! The erax editor binary.
//!
//! The entry point parses CLI arguments into a typed [`cli::Invocation`] and
//! routes to the single dispatch match below (N-004: one router). Failures are
//! concrete typed errors matched exhaustively here (N-001); nothing exits the
//! process from inside a parser or a run function — `main` returns an
//! [`ExitCode`].

use erax::cli;
use erax::run;

use std::process::ExitCode;

use cli::{ColorMode, Degradation, Invocation, InvocationMode, ParseFailure};

fn main() -> ExitCode {
    match cli::parse() {
        Ok(invocation) => run(invocation),
        Err(ParseFailure::Clap(e)) => {
            // clap's help/version are values, not exits (N-006).
            match e.kind() {
                clap::error::ErrorKind::DisplayHelp
                | clap::error::ErrorKind::DisplayVersion => {
                    print!("{e}");
                    ExitCode::SUCCESS
                }
                _ => {
                    eprint!("{e}");
                    ExitCode::FAILURE
                }
            }
        }
        Err(ParseFailure::Cli(e)) => {
            eprintln!("erax: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The single dispatch router (N-004): one match over the invocation mode.
fn run(invocation: Invocation) -> ExitCode {
    // Environment degradation of the configured preference is reported
    // here, at the impure shell: never silent, never an error exit.
    if let Some(degradation) = invocation.degradation {
        eprintln!("erax: {}", degradation_notice(degradation));
    }
    match invocation.mode {
        InvocationMode::Sed => match run::run_sed_mode(&invocation) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("erax: sed: {e}");
                ExitCode::FAILURE
            }
        },
        InvocationMode::Terminal(color_mode) => {
            match run::run_terminal_mode(&invocation, color_mode) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("erax: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        InvocationMode::Gui => {
            #[cfg(target_os = "windows")]
            println!("erax: Windows detected. The editor works fine. The operating system remains the problem.");
            match run::run_gui_mode(&invocation) {
                Ok(()) => ExitCode::SUCCESS,
                Err(run::GuiError::EventLoop(_)) => {
                    // The display claimed present but produced no event
                    // loop: degrade in the shell exactly as the resolver
                    // would have, instead of failing the invocation.
                    let (fallback, degradation) =
                        event_loop_fallback(&invocation.environment);
                    eprintln!("erax: {}", degradation_notice(degradation));
                    let result = match fallback {
                        EventLoopFallback::Terminal(color_mode) => {
                            run::run_terminal_mode(&invocation, color_mode)
                                .map_err(|e| format!("{e}"))
                        }
                        EventLoopFallback::Sed => run::run_sed_mode(&invocation)
                            .map_err(|e| format!("sed: {e}")),
                    };
                    match result {
                        Ok(()) => ExitCode::SUCCESS,
                        Err(e) => {
                            eprintln!("erax: {e}");
                            ExitCode::FAILURE
                        }
                    }
                }
                Err(e) => {
                    eprintln!("erax: gui: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        InvocationMode::WebServer => {
            match erax::web::server::run_web_server(
                &invocation,
                &invocation.web_bind,
                invocation.web_allow_remote,
            ) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("erax: web-server: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

/// The one-line stderr notice naming a degradation outcome.
fn degradation_notice(degradation: Degradation) -> &'static str {
    match degradation {
        Degradation::ToTerminal => "no graphical environment; running in terminal mode",
        Degradation::ToSed => "piped standard input; running in stream-editor mode",
    }
}

/// The shell's fallback when the GUI event loop is unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventLoopFallback {
    /// Best terminal rung from the captured snapshot.
    Terminal(ColorMode),
    /// Stream-editor mode (piped stdin).
    Sed,
}

/// Which fallback the shell takes when the GUI event loop is
/// unavailable, mirroring the resolver's Gui branch: piped stdin
/// degrades to stream-editor mode, otherwise to the best terminal rung
/// from the captured snapshot (never re-read from the environment).
fn event_loop_fallback(env: &cli::Environment) -> (EventLoopFallback, Degradation) {
    if env.stdin_is_tty {
        (
            EventLoopFallback::Terminal(env.color_mode),
            Degradation::ToTerminal,
        )
    } else {
        (EventLoopFallback::Sed, Degradation::ToSed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(tty: bool, color: ColorMode) -> cli::Environment {
        cli::Environment {
            graphical_display_present: true,
            stdin_is_tty: tty,
            color_mode: color,
        }
    }

    #[test]
    fn event_loop_fallback_mirrors_resolver_gui_branch() {
        // tty: the best terminal rung from the snapshot, ToTerminal notice.
        let (fallback, degradation) = event_loop_fallback(&snapshot(true, ColorMode::Ansi16));
        assert_eq!(
            fallback,
            EventLoopFallback::Terminal(ColorMode::Ansi16)
        );
        assert_eq!(degradation, Degradation::ToTerminal);
        // Piped stdin is checked first, as in the resolver: stream-editor
        // mode with the ToSed notice, not a terminal on a dead stdin.
        let (fallback, degradation) = event_loop_fallback(&snapshot(false, ColorMode::TrueColor));
        assert_eq!(fallback, EventLoopFallback::Sed);
        assert_eq!(degradation, Degradation::ToSed);
    }
}
