//! Typed command-line argument parsing (N-271).
//!
//! Arguments are parsed with clap into a typed [`Args`], then converted by a
//! pure function into an [`Invocation`]. The parser never exits the process
//! (N-001, N-006): help and errors are returned as values and rendered by the
//! caller. Parsing never mutates the compile-time configuration (N-007,
//! N-012): overrides are typed fields on [`Invocation`] consumed where needed.

use clap::Parser;
use std::path::PathBuf;

use crate::terminal::theme::ThemeId;
use crate::config::frontend::{SHELL_MODE, WEB_ALLOW_REMOTE, WEB_BIND};

/// erax command-line arguments, as parsed by clap.
#[derive(Debug, Parser)]
#[command(
    name = "erax",
    about = "A multi-modal text editor",
    version,
    disable_help_flag = true,
    disable_version_flag = true
)]
pub struct Args {
    /// Show this help message
    #[arg(short = 'h', long = "help", action = clap::ArgAction::Help)]
    pub help: (),

    /// Show version
    #[arg(long = "version", action = clap::ArgAction::Version)]
    pub version: (),

    /// Force stream editor (sed) mode
    #[arg(short = 's', long = "sed")]
    pub sed: bool,

    /// Force ASCII terminal mode (no color escapes)
    #[arg(short = 'a', long = "ascii")]
    pub ascii: bool,

    /// Force UTF-8 terminal mode
    #[arg(short = 'u', long = "utf8")]
    pub utf8: bool,

    /// Force GUI mode
    #[arg(short = 'g', long = "gui")]
    pub gui: bool,

    /// Run the WebUI server (VS Code-server style): Core+TUI run here,
    /// the GUI presentation core runs in the browser. Opt-in; binds
    /// 127.0.0.1 by default; prints a per-session token (required).
    #[arg(long = "web-server")]
    pub web_server: bool,

    /// Bind address for --web-server (default: the `WEB_BIND` config const).
    #[arg(long = "web-bind", value_name = "ADDR:PORT", default_value = WEB_BIND)]
    pub web_bind: String,

    /// Acknowledge binding --web-server to a non-loopback address.
    /// Required for any non-127.0.0.1/::1 bind; without it the server
    /// refuses to start. (No unauthenticated mode exists regardless.)
    #[arg(long = "web-allow-remote")]
    pub web_allow_remote: bool,

    /// Suppress automatic printing of pattern space (sed mode)
    #[arg(short = 'n', long = "quiet")]
    pub quiet: bool,

    /// Add a sed script from the command line (may repeat)
    #[arg(short = 'e', long = "expression")]
    pub expression: Vec<String>,

    /// Read sed script from a file
    #[arg(short = 'f', long = "file", value_name = "SCRIPT_FILE")]
    pub script_file: Option<PathBuf>,

    /// Edit files in place (sed mode)
    #[arg(short = 'i', long = "in-place")]
    pub in_place: bool,

    /// Color theme override (typed; invalid names are a parse error)
    #[arg(short = 't', long = "theme", value_name = "NAME")]
    pub theme: Option<String>,

    /// Wrap long lines at the window edge (N-012)
    #[arg(long = "wrap", action = clap::ArgAction::SetTrue)]
    pub wrap: bool,

    /// Do not wrap long lines (N-012)
    #[arg(long = "no-wrap", action = clap::ArgAction::SetTrue)]
    pub no_wrap: bool,

    /// File(s) to open
    pub files: Vec<PathBuf>,
}

/// How the editor should run: the typed result of CLI parsing.
#[derive(Debug)]
pub struct Invocation {
    /// Which mode to run in.
    pub mode: InvocationMode,
    /// Files to open.
    pub files: Vec<PathBuf>,
    /// Theme override from `--theme` (typed; `None` means the config default).
    pub theme: Option<ThemeId>,
    /// Line-wrap override from `--wrap` / `--no-wrap` (`None` = config default).
    pub wrap: Option<bool>,
    /// Sed-mode options.
    pub sed: SedOptions,
    /// Bind address for `--web-server` (WebServer mode only).
    pub web_bind: String,
    /// Acknowledging flag for non-loopback `--web-bind`.
    pub web_allow_remote: bool,
}

/// Which frontend erax starts when no CLI mode flag selects one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellMode {
    /// Terminal UI; piped stdin still selects stream-editor mode, as with
    /// no configuration at all.
    Tui,
    /// UTF-8 terminal UI (full color per environment detection).
    Utf8,
    /// WebUI server: Core+TUI run headless here, the presentation core
    /// runs in the browser. Non-loopback binds require the
    /// `--web-allow-remote` acknowledgement.
    WebUi,
}

/// Which top-level mode to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvocationMode {
    /// Stream editor mode.
    Sed,
    /// Terminal UI mode with the given color mode.
    Terminal(ColorMode),
    /// GUI mode.
    Gui,
    /// WebUI server mode (headless Core+TUI + WebSocket protocol).
    WebServer,
}

/// Terminal color capability for TUI output.
///
/// Decided where the mode is chosen (N-002, N-109): `-a` forces `Mono`,
/// `-u` forces full color per detection, otherwise detected from the
/// environment. Never inferred from GUI environment variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// 24-bit truecolor.
    TrueColor,
    /// 16-color ANSI fallback.
    Ansi16,
    /// No color escapes at all (N-110).
    Mono,
}

impl ColorMode {
    /// Detect the color mode from the environment (N-109: fixed detection).
    ///
    /// Truecolor is claimed only from `COLORTERM=truecolor|24bit`, a TERM
    /// containing `truecolor`/`24bit`, or a known truecolor `TERM_PROGRAM`.
    /// The old code also claimed truecolor for `screen`/`tmux` from TERM
    /// alone, which is false without a truecolor-capable outer terminal.
    pub fn detect() -> Self {
        use std::env::var;
        let colorterm = var("COLORTERM").unwrap_or_default().to_lowercase();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            return ColorMode::TrueColor;
        }
        let term = var("TERM").unwrap_or_default().to_lowercase();
        if term.contains("truecolor") || term.contains("24bit") {
            return ColorMode::TrueColor;
        }
        let term_program = var("TERM_PROGRAM").unwrap_or_default();
        match term_program.as_str() {
            "iTerm.app" | "wezterm" | "vscode" | "Hyper" => return ColorMode::TrueColor,
            _ => {}
        }
        // kitty sets KITTY_PID only when running inside kitty, which is truecolor.
        if var("KITTY_PID").is_ok() {
            return ColorMode::TrueColor;
        }
        if term == "dumb" || term.is_empty() {
            return ColorMode::Mono;
        }
        ColorMode::Ansi16
    }
}

/// Sed-mode options, as typed values.
#[derive(Debug, Default)]
pub struct SedOptions {
    pub quiet: bool,
    pub expressions: Vec<String>,
    pub script_file: Option<PathBuf>,
    pub in_place: bool,
}

/// Concrete CLI parse errors (N-001): no boxed strings, no `process::exit`.
#[derive(Debug, PartialEq, Eq)]
pub enum CliError {
    /// `--theme NAME` did not name a known theme.
    UnknownTheme(String),
    /// `--wrap` and `--no-wrap` were both given.
    ConflictingWrapFlags,
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::UnknownTheme(name) => {
                write!(f, "unknown theme '{name}'. Valid themes: ")?;
                let mut first = true;
                for id in ThemeId::all() {
                    if !first {
                        write!(f, ", ")?;
                    }
                    first = false;
                    write!(f, "{}", id.name())?;
                }
                Ok(())
            }
            CliError::ConflictingWrapFlags => {
                write!(f, "--wrap and --no-wrap are mutually exclusive")
            }
        }
    }
}

/// Parse the process arguments into a typed [`Invocation`].
///
/// Returns `Ok` with the invocation, or `Err` with either a [`CliError`] or a
/// clap error (which covers help/version display and clap's own failures).
/// The parser never exits the process (N-006).
pub fn parse() -> Result<Invocation, ParseFailure> {
    parse_from(std::env::args())
}

/// Parse an argument iterator (test seam).
pub fn parse_from<I>(args: I) -> Result<Invocation, ParseFailure>
where
    I: IntoIterator,
    I::Item: Into<std::ffi::OsString> + Clone,
{
    let args = Args::try_parse_from(args).map_err(ParseFailure::Clap)?;
    convert(args)
}

/// A parse failure: either a typed CLI error or clap's own error value.
#[derive(Debug)]
pub enum ParseFailure {
    Cli(CliError),
    Clap(clap::Error),
}

/// Pure conversion from parsed args to a typed invocation (N-002).
fn convert(args: Args) -> Result<Invocation, ParseFailure> {
    let theme = match args.theme {
        Some(name) => Some(
            name.parse::<ThemeId>()
                .map_err(|_| ParseFailure::Cli(CliError::UnknownTheme(name)))?,
        ),
        None => None,
    };

    let wrap = match (args.wrap, args.no_wrap) {
        (true, true) => return Err(ParseFailure::Cli(CliError::ConflictingWrapFlags)),
        (true, false) => Some(true),
        (false, true) => Some(false),
        (false, false) => None,
    };

    // Mode selection is a pure function of the typed flags (N-002).
    // Explicit flags win; with none, the SHELL_MODE config const decides.
    let mode = if args.web_server {
        InvocationMode::WebServer
    } else if args.sed || !args.expression.is_empty() || args.script_file.is_some() {
        InvocationMode::Sed
    } else if args.gui {
        InvocationMode::Gui
    } else if args.ascii {
        InvocationMode::Terminal(ColorMode::Mono)
    } else if args.utf8 {
        // UTF-8 mode: full color per environment detection.
        InvocationMode::Terminal(utf8_color_mode())
    } else {
        match SHELL_MODE {
            ShellMode::Tui => {
                if is_stdin_tty() {
                    InvocationMode::Terminal(ColorMode::detect())
                } else {
                    // Piped stdin: stream editor mode (preserves the old
                    // detect_mode behavior).
                    InvocationMode::Sed
                }
            }
            ShellMode::Utf8 => InvocationMode::Terminal(utf8_color_mode()),
            ShellMode::WebUi => InvocationMode::WebServer,
        }
    };

    Ok(Invocation {
        mode,
        files: args.files,
        theme,
        wrap,
        sed: SedOptions {
            quiet: args.quiet,
            expressions: args.expression,
            script_file: args.script_file,
            in_place: args.in_place,
        },
        web_bind: args.web_bind,
        web_allow_remote: args.web_allow_remote || WEB_ALLOW_REMOTE,
    })
}

/// Color mode for UTF-8 terminal mode: full color per detection, never Mono.
fn utf8_color_mode() -> ColorMode {
    match ColorMode::detect() {
        ColorMode::Mono => ColorMode::Ansi16,
        other => other,
    }
}

fn is_stdin_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Invocation, ParseFailure> {
        parse_from(args.iter())
    }

    #[test]
    fn sed_mode_from_flag() {
        let inv = parse_args(&["erax", "-s", "file.txt"]).unwrap();
        assert_eq!(inv.mode, InvocationMode::Sed);
        assert_eq!(inv.files, vec![PathBuf::from("file.txt")]);
    }

    #[test]
    fn sed_mode_from_expression() {
        let inv = parse_args(&["erax", "-e", "s/a/b/", "file.txt"]).unwrap();
        assert_eq!(inv.mode, InvocationMode::Sed);
        assert_eq!(inv.sed.expressions, vec!["s/a/b/".to_string()]);
    }

    #[test]
    fn ascii_flag_forces_mono() {
        let inv = parse_args(&["erax", "-a"]).unwrap();
        assert_eq!(inv.mode, InvocationMode::Terminal(ColorMode::Mono));
    }

    #[test]
    fn gui_flag_selects_gui() {
        let inv = parse_args(&["erax", "-g"]).unwrap();
        assert_eq!(inv.mode, InvocationMode::Gui);
    }

    #[test]
    fn theme_parses_to_typed_id() {
        let inv = parse_args(&["erax", "-t", "dracula"]).unwrap();
        assert_eq!(inv.theme, Some(ThemeId::Dracula));
    }

    #[test]
    fn unknown_theme_is_typed_error() {
        let err = parse_args(&["erax", "-t", "not-a-theme"]).unwrap_err();
        match err {
            ParseFailure::Cli(CliError::UnknownTheme(name)) => {
                assert_eq!(name, "not-a-theme");
            }
            other => panic!("expected UnknownTheme, got {other:?}"),
        }
    }

    #[test]
    fn wrap_flags_produce_typed_override() {
        let inv = parse_args(&["erax", "--wrap"]).unwrap();
        assert_eq!(inv.wrap, Some(true));
        let inv = parse_args(&["erax", "--no-wrap"]).unwrap();
        assert_eq!(inv.wrap, Some(false));
        let inv = parse_args(&["erax"]).unwrap();
        assert_eq!(inv.wrap, None);
    }

    #[test]
    fn conflicting_wrap_flags_are_an_error() {
        let err = parse_args(&["erax", "--wrap", "--no-wrap"]).unwrap_err();
        assert!(matches!(
            err,
            ParseFailure::Cli(CliError::ConflictingWrapFlags)
        ));
    }

    #[test]
    fn help_is_a_clap_value_not_an_exit() {
        let err = parse_args(&["erax", "--help"]).unwrap_err();
        match err {
            ParseFailure::Clap(e) => {
                assert_eq!(e.kind(), clap::error::ErrorKind::DisplayHelp)
            }
            other => panic!("expected DisplayHelp, got {other:?}"),
        }
    }

    #[test]
    fn unknown_flag_is_a_clap_error() {
        let err = parse_args(&["erax", "--bogus"]).unwrap_err();
        assert!(matches!(err, ParseFailure::Clap(_)));
    }

    #[test]
    fn default_frontend_follows_shell_mode_const() {
        // No CLI mode flag: the SHELL_MODE config const decides. (Test
        // stdin is never a tty, so Tui degrades to Sed here exactly as
        // the unconfigured fallback would.)
        let inv = parse_args(&["erax"]).unwrap();
        match SHELL_MODE {
            ShellMode::Tui => assert_eq!(inv.mode, InvocationMode::Sed),
            ShellMode::Utf8 => {
                assert!(matches!(inv.mode, InvocationMode::Terminal(_)))
            }
            ShellMode::WebUi => assert_eq!(inv.mode, InvocationMode::WebServer),
        }
    }

    #[test]
    fn explicit_flags_override_shell_mode() {
        // Every explicit mode flag wins over SHELL_MODE, whatever it is.
        assert_eq!(
            parse_args(&["erax", "-g"]).unwrap().mode,
            InvocationMode::Gui
        );
        assert_eq!(
            parse_args(&["erax", "--web-server"]).unwrap().mode,
            InvocationMode::WebServer
        );
        assert!(matches!(
            parse_args(&["erax", "-u"]).unwrap().mode,
            InvocationMode::Terminal(_)
        ));
        assert_eq!(
            parse_args(&["erax", "-a"]).unwrap().mode,
            InvocationMode::Terminal(ColorMode::Mono)
        );
        assert_eq!(
            parse_args(&["erax", "-s"]).unwrap().mode,
            InvocationMode::Sed
        );
    }

    #[test]
    fn web_bind_defaults_to_const() {
        let inv = parse_args(&["erax"]).unwrap();
        assert_eq!(inv.web_bind, WEB_BIND);
    }

    #[test]
    fn explicit_web_bind_overrides_const() {
        let inv = parse_args(&["erax", "--web-bind", "0.0.0.0:9090"]).unwrap();
        assert_eq!(inv.web_bind, "0.0.0.0:9090");
    }

    #[test]
    fn web_allow_remote_flag_or_const() {
        let inv = parse_args(&["erax", "--web-allow-remote"]).unwrap();
        assert!(inv.web_allow_remote);
        let inv = parse_args(&["erax"]).unwrap();
        assert_eq!(inv.web_allow_remote, WEB_ALLOW_REMOTE);
    }
}
