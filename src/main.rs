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

use cli::{Invocation, InvocationMode, ParseFailure};

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
        InvocationMode::Gui => match run::run_gui_mode(&invocation) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("erax: gui: {e}");
                ExitCode::FAILURE
            }
        },
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
