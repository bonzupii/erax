//! Command-line argument parsing for erax.
//!
//! This module provides the `Cli` struct which encapsulates all command-line
//! options and methods for parsing them.

use crate::config::{Config, ConfigValue};
use std::path::PathBuf;

/// Command-line interface configuration.
#[derive(Debug, Default)]
pub struct Cli {
    /// File(s) to open
    pub files: Vec<PathBuf>,

    /// Force sed-like mode
    pub sed: bool,

    /// Force ASCII terminal mode
    pub ascii: bool,

    /// Force ANSI terminal mode
    pub ansi: bool,

    /// Force UTF-8 terminal mode
    pub utf8: bool,

    /// Force GUI mode
    pub gui: bool,

    // Sed-specific options for POSIX compliance
    /// Suppress automatic printing of pattern space (sed mode)
    pub quiet: bool,

    /// Scripts to execute (sed mode, -e flag)
    pub expression: Vec<String>,

    /// Script file path (sed mode, -f flag)
    pub script_file: Option<PathBuf>,

    /// Edit files in-place (sed mode)
    pub in_place: bool,

    /// Color theme override
    pub theme: Option<String>,
}

impl Cli {
    /// Parse command-line arguments.
    ///
    /// Returns a `Cli` struct populated with parsed arguments.
    /// Returns an error if required values are missing.
    pub fn parse() -> Result<Self, Box<dyn std::error::Error>> {
        let mut cli = Self::default();
        let mut args = std::env::args().skip(1).peekable();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-s" | "--sed" => cli.sed = true,
                "-a" | "--ascii" => cli.ascii = true,
                "-u" | "--utf8" => cli.utf8 = true,
                "-g" | "--gui" => cli.gui = true,
                "-n" | "--quiet" => cli.quiet = true,
                "-i" | "--in-place" => cli.in_place = true,
                "-e" | "--expression" => {
                    if let Some(expr) = args.next() {
                        cli.expression.push(expr);
                    } else {
                        return Err("--expression requires a value".into());
                    }
                }
                "-f" | "--file" => {
                    if let Some(path) = args.next() {
                        cli.script_file = Some(PathBuf::from(path));
                    } else {
                        return Err("--file requires a value".into());
                    }
                }
                "-t" | "--theme" => {
                    if let Some(t) = args.next() {
                        cli.theme = Some(t);
                    } else {
                        return Err("--theme requires a value".into());
                    }
                }
                "-h" | "--help" => {
                    println!("erax - A multi-modal text editor");
                    println!();
                    println!("Usage: erax [OPTIONS] [FILES...]");
                    println!();
                    println!("Options:");
                    println!("  -h, --help        Show this help message");
                    println!("  -g, --gui         Force GUI mode");
                    println!("  -u, --utf8        Force UTF-8 terminal mode");
                    println!("  -a, --ascii       Force ASCII terminal mode");
                    println!("  -t, --theme NAME  Set color theme");
                    println!();
                    println!("Sed mode options:");
                    println!("  -s, --sed         Force stream editor mode");
                    println!("  -n, --quiet       Suppress automatic printing");
                    println!("  -e, --expression  Add script to commands");
                    println!("  -f, --file        Add script file");
                    println!("  -i, --in-place    Edit files in place");
                    std::process::exit(0);
                }
                arg if arg.starts_with('-') => {
                    return Err(format!("Unknown flag: {}. Use --help for usage.", arg).into());
                }
                _ => {
                    // Positional arguments are files
                    cli.files.push(PathBuf::from(arg));
                }
            }
        }

        Ok(cli)
    }

    /// Apply CLI overrides to a configuration object.
    pub fn apply_to_config(&self, config: &mut Config) {
        if let Some(theme) = &self.theme {
            config.set("theme", ConfigValue::String(theme.clone()));
        }
    }
}
