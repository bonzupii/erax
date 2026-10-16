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
    /// Environment degradation the resolver applied to the configured
    /// preference, if any. The shell reports it on stderr; it is never
    /// silent and never an error exit.
    pub degradation: Option<Degradation>,
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
    /// Native GUI. The shipped default. Degrades by environment: piped
    /// stdin selects stream-editor mode, and no graphical display selects
    /// the best terminal rung the environment supports. Neither
    /// degradation is silent: the shell reports it on stderr.
    Gui,
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

/// Snapshot of the impure environment, gathered once at the boundary.
///
/// Mode resolution is a pure function over (flags, `SHELL_MODE`, this
/// snapshot): nothing inside the resolver reads process state, so the
/// whole decision table is unit-testable with synthetic snapshots.
/// Reading the live process is the boundary's job
/// ([`Environment::capture`]); tests must never manipulate the real
/// environment to drive the resolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Environment {
    /// A graphical display is reachable: inherently true on Windows and
    /// macOS, `DISPLAY` or `WAYLAND_DISPLAY` set on unix-likes.
    pub graphical_display_present: bool,
    /// Standard input is a terminal (false when piped).
    pub stdin_is_tty: bool,
    /// Terminal color capability, per the existing [`ColorMode`]
    /// detection.
    pub color_mode: ColorMode,
}

impl Environment {
    /// Read the process state once, at the impure CLI boundary.
    pub fn capture() -> Self {
        Environment {
            graphical_display_present: display_present(),
            stdin_is_tty: {
                use std::io::IsTerminal;
                std::io::stdin().is_terminal()
            },
            color_mode: ColorMode::detect(),
        }
    }
}

/// Whether a graphical display is reachable from this process.
fn display_present() -> bool {
    // Windows and macOS always have a display server to talk to.
    if cfg!(target_os = "windows") || cfg!(target_os = "macos") {
        return true;
    }
    std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok()
}

/// How the environment degraded a configured preference.
///
/// Returned by the pure resolver so the (impure) shell can report the
/// degradation on stderr. Degradation is never silent and never an
/// error exit: the editor still starts, in the named fallback mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Degradation {
    /// The preference fell back to the best terminal rung the
    /// environment supports.
    ToTerminal,
    /// The preference fell back to stream-editor mode (piped stdin).
    ToSed,
}

/// The CLI mode flags that participate in mode resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModeFlags {
    /// `--web-server`.
    pub web_server: bool,
    /// `-s`, or a sed script from `-e`/`-f`.
    pub sed: bool,
    /// `-g`.
    pub gui: bool,
    /// `-a`.
    pub ascii: bool,
    /// `-u`.
    pub utf8: bool,
}

/// The resolved invocation mode plus any environment degradation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedMode {
    pub mode: InvocationMode,
    pub degradation: Option<Degradation>,
}

/// Pure mode resolution over (flags, config const, environment snapshot).
///
/// Precedence: explicit CLI mode flags, then the `SHELL_MODE` const,
/// then environment degradation of that preference. The `WebUi`
/// preference is never degraded (the server is headless by design); an
/// explicit `-g` is never degraded either (a dead display surfaces as an
/// event-loop error, which the dispatch boundary degrades to terminal
/// mode with the same stderr notice).
pub fn resolve_mode(flags: ModeFlags, shell: ShellMode, env: &Environment) -> ResolvedMode {
    let (mode, degradation) = if flags.web_server {
        (InvocationMode::WebServer, None)
    } else if flags.sed {
        (InvocationMode::Sed, None)
    } else if flags.gui {
        (InvocationMode::Gui, None)
    } else if flags.ascii {
        (InvocationMode::Terminal(ColorMode::Mono), None)
    } else if flags.utf8 {
        (
            InvocationMode::Terminal(utf8_color_mode(env.color_mode)),
            None,
        )
    } else {
        match shell {
            ShellMode::Tui => {
                if env.stdin_is_tty {
                    (InvocationMode::Terminal(env.color_mode), None)
                } else {
                    // Piped stdin: stream editor mode (the unconfigured
                    // fallback; not a degradation of a GUI preference).
                    (InvocationMode::Sed, None)
                }
            }
            ShellMode::Gui => {
                if !env.stdin_is_tty {
                    (InvocationMode::Sed, Some(Degradation::ToSed))
                } else if env.graphical_display_present {
                    (InvocationMode::Gui, None)
                } else {
                    // No display: the best terminal rung the capability
                    // inputs support — the same rung Tui would take.
                    (
                        InvocationMode::Terminal(env.color_mode),
                        Some(Degradation::ToTerminal),
                    )
                }
            }
            ShellMode::Utf8 => (
                InvocationMode::Terminal(utf8_color_mode(env.color_mode)),
                None,
            ),
            ShellMode::WebUi => (InvocationMode::WebServer, None),
        }
    };
    ResolvedMode { mode, degradation }
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

    // Mode selection is a pure function of the typed flags, the
    // SHELL_MODE const, and the environment snapshot (N-002). Explicit
    // flags win; with none, the const decides, degraded by the
    // environment. The snapshot is captured once here, at the impure
    // boundary; the resolver itself reads no process state.
    let flags = ModeFlags {
        web_server: args.web_server,
        sed: args.sed || !args.expression.is_empty() || args.script_file.is_some(),
        gui: args.gui,
        ascii: args.ascii,
        utf8: args.utf8,
    };
    let env = Environment::capture();
    let resolved = resolve_mode(flags, SHELL_MODE, &env);

    Ok(Invocation {
        mode: resolved.mode,
        degradation: resolved.degradation,
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
fn utf8_color_mode(detected: ColorMode) -> ColorMode {
    match detected {
        ColorMode::Mono => ColorMode::Ansi16,
        other => other,
    }
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

    /// Synthetic environment snapshots: the resolver never reads process
    /// state, so tests drive it with constructed inputs only. Touching
    /// the real environment here is forbidden.
    fn snapshot(display: bool, tty: bool, color: ColorMode) -> Environment {
        Environment {
            graphical_display_present: display,
            stdin_is_tty: tty,
            color_mode: color,
        }
    }

    /// Fully capable environment: display present, tty stdin.
    fn capable() -> Environment {
        snapshot(true, true, ColorMode::TrueColor)
    }

    fn flags_with(mk: fn(&mut ModeFlags)) -> ModeFlags {
        let mut flags = ModeFlags::default();
        mk(&mut flags);
        flags
    }

    #[test]
    fn gui_const_with_display_selects_gui() {
        let r = resolve_mode(ModeFlags::default(), ShellMode::Gui, &capable());
        assert_eq!(r.mode, InvocationMode::Gui);
        assert_eq!(r.degradation, None);
    }

    #[test]
    fn gui_const_without_display_degrades_to_terminal() {
        let env = snapshot(false, true, ColorMode::Ansi16);
        let r = resolve_mode(ModeFlags::default(), ShellMode::Gui, &env);
        assert_eq!(r.mode, InvocationMode::Terminal(ColorMode::Ansi16));
        assert_eq!(r.degradation, Some(Degradation::ToTerminal));
    }

    #[test]
    fn gui_const_degraded_terminal_uses_detected_rung() {
        // The fallback rung is the detected capability, not a second
        // detector: every ColorMode round-trips through unchanged.
        for color in [ColorMode::TrueColor, ColorMode::Ansi16, ColorMode::Mono] {
            let env = snapshot(false, true, color);
            let r = resolve_mode(ModeFlags::default(), ShellMode::Gui, &env);
            assert_eq!(r.mode, InvocationMode::Terminal(color));
            assert_eq!(r.degradation, Some(Degradation::ToTerminal));
        }
    }

    #[test]
    fn gui_const_with_piped_stdin_degrades_to_sed() {
        // Piped stdin is checked before the display: the oracle order.
        for display in [true, false] {
            let env = snapshot(display, false, ColorMode::TrueColor);
            let r = resolve_mode(ModeFlags::default(), ShellMode::Gui, &env);
            assert_eq!(r.mode, InvocationMode::Sed);
            assert_eq!(r.degradation, Some(Degradation::ToSed));
        }
    }

    #[test]
    fn explicit_gui_flag_is_not_degraded() {
        // An explicit -g is never degraded: a dead display surfaces as an
        // event-loop error, which the dispatch boundary degrades instead.
        let flags = flags_with(|f| f.gui = true);
        let env = snapshot(false, true, ColorMode::Mono);
        let r = resolve_mode(flags, ShellMode::Tui, &env);
        assert_eq!(r.mode, InvocationMode::Gui);
        assert_eq!(r.degradation, None);
    }

    #[test]
    fn explicit_flags_beat_the_gui_const() {
        let env = capable();
        let cases = [
            (
                flags_with(|f| f.web_server = true),
                InvocationMode::WebServer,
            ),
            (flags_with(|f| f.sed = true), InvocationMode::Sed),
            (flags_with(|f| f.gui = true), InvocationMode::Gui),
            (
                flags_with(|f| f.ascii = true),
                InvocationMode::Terminal(ColorMode::Mono),
            ),
            (
                flags_with(|f| f.utf8 = true),
                InvocationMode::Terminal(ColorMode::TrueColor),
            ),
        ];
        for (flags, expected) in cases {
            let r = resolve_mode(flags, ShellMode::Gui, &env);
            assert_eq!(r.mode, expected);
            assert_eq!(r.degradation, None);
        }
    }

    #[test]
    fn flag_precedence_is_web_server_sed_gui_ascii_utf8() {
        let env = capable();
        let both = |a: fn(&mut ModeFlags), b: fn(&mut ModeFlags)| {
            let mut flags = ModeFlags::default();
            a(&mut flags);
            b(&mut flags);
            flags
        };
        assert_eq!(
            resolve_mode(both(|f| f.web_server = true, |f| f.sed = true), ShellMode::Tui, &env).mode,
            InvocationMode::WebServer
        );
        assert_eq!(
            resolve_mode(both(|f| f.sed = true, |f| f.gui = true), ShellMode::Tui, &env).mode,
            InvocationMode::Sed
        );
        assert_eq!(
            resolve_mode(both(|f| f.gui = true, |f| f.ascii = true), ShellMode::Tui, &env).mode,
            InvocationMode::Gui
        );
        assert_eq!(
            resolve_mode(both(|f| f.ascii = true, |f| f.utf8 = true), ShellMode::Tui, &env).mode,
            InvocationMode::Terminal(ColorMode::Mono)
        );
    }

    #[test]
    fn webui_const_ignores_missing_display() {
        // The server is headless by design: no local display is needed.
        let env = snapshot(false, true, ColorMode::TrueColor);
        let r = resolve_mode(ModeFlags::default(), ShellMode::WebUi, &env);
        assert_eq!(r.mode, InvocationMode::WebServer);
        assert_eq!(r.degradation, None);
    }

    #[test]
    fn tui_const_paths_unchanged() {
        let env = snapshot(false, true, ColorMode::Ansi16);
        let r = resolve_mode(ModeFlags::default(), ShellMode::Tui, &env);
        assert_eq!(r.mode, InvocationMode::Terminal(ColorMode::Ansi16));
        assert_eq!(r.degradation, None);
        // Piped stdin keeps the unconfigured fallback, without a notice.
        let piped = snapshot(false, false, ColorMode::Ansi16);
        let r = resolve_mode(ModeFlags::default(), ShellMode::Tui, &piped);
        assert_eq!(r.mode, InvocationMode::Sed);
        assert_eq!(r.degradation, None);
    }

    #[test]
    fn utf8_const_unchanged() {
        // Utf8 means full color per detection: never Mono.
        let env = snapshot(true, true, ColorMode::Mono);
        let r = resolve_mode(ModeFlags::default(), ShellMode::Utf8, &env);
        assert_eq!(r.mode, InvocationMode::Terminal(ColorMode::Ansi16));
        assert_eq!(r.degradation, None);
        let env = snapshot(true, true, ColorMode::TrueColor);
        let r = resolve_mode(ModeFlags::default(), ShellMode::Utf8, &env);
        assert_eq!(r.mode, InvocationMode::Terminal(ColorMode::TrueColor));
        assert_eq!(r.degradation, None);
    }

    #[test]
    fn default_frontend_follows_shell_mode_const() {
        // No CLI mode flag: the SHELL_MODE config const decides, against a
        // fully capable environment so no degradation interferes.
        let r = resolve_mode(ModeFlags::default(), SHELL_MODE, &capable());
        assert_eq!(r.degradation, None);
        match SHELL_MODE {
            ShellMode::Tui => assert!(matches!(r.mode, InvocationMode::Terminal(_))),
            ShellMode::Gui => assert_eq!(r.mode, InvocationMode::Gui),
            ShellMode::Utf8 => assert!(matches!(r.mode, InvocationMode::Terminal(_))),
            ShellMode::WebUi => assert_eq!(r.mode, InvocationMode::WebServer),
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
