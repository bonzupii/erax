//! erax - A modern, high-performance text editor
//!
//! This is the main entry point. It parses CLI arguments and delegates
//! to the appropriate mode runner (sed, terminal, or GUI).

mod cli;
mod config;
mod core;
mod run;
mod sed;
mod terminal;
mod user_config;

#[cfg(feature = "gui")]
mod gui;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let cli = cli::Cli::parse()?;

    // Load configuration
    let mut config = config::Config::default();
    user_config::configure(&mut config);

    // Apply CLI overrides
    cli.apply_to_config(&mut config);

    // Determine mode
    let mode = if cli.sed || !cli.expression.is_empty() || cli.script_file.is_some() {
        run::EditorMode::Sed
    } else if cli.ascii {
        run::EditorMode::AsciiTerminal
    } else if cli.ansi {
        run::EditorMode::AnsiTerminal
    } else if cli.utf8 {
        run::EditorMode::Utf8Terminal
    } else if cli.gui {
        #[cfg(feature = "gui")]
        {
            run::run_gui_mode(&cli.files, &config)?;
            return Ok(());
        }
        #[cfg(not(feature = "gui"))]
        {
            eprintln!("GUI feature not available. Compile with --features gui");
            std::process::exit(1);
        }
    } else {
        run::detect_mode()?
    };

    // Run in appropriate mode
    match mode {
        run::EditorMode::Sed => {
            run::run_sed_mode(&cli)?;
        }
        run::EditorMode::AsciiTerminal
        | run::EditorMode::AnsiTerminal
        | run::EditorMode::Utf8Terminal => {
            let display_mode = terminal::capabilities::DisplayMode::detect();
            run::run_terminal_mode(&cli.files, &config, display_mode)?;
        }
        run::EditorMode::Gui => {
            run::run_gui_mode(&cli.files, &config)?;
        }
    }

    Ok(())
}
