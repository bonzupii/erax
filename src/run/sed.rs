//! Sed (stream editing) mode implementation.

use crate::cli::Cli;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};

use super::validate_file_path;

/// Run in sed (stream editing) mode.
pub fn run_sed_mode(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    use crate::sed::SedConfig;

    let mut sed_config = SedConfig::new();
    sed_config.quiet = cli.quiet;

    for script in &cli.expression {
        sed_config
            .add_script(script)
            .map_err(|e| format!("Error parsing script: {}", e))?;
    }

    if let Some(script_path) = &cli.script_file {
        let validated_path = validate_file_path(script_path)?;
        let file = File::open(&validated_path)?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            if !line.trim().is_empty() && !line.starts_with('#') {
                sed_config
                    .add_script(&line)
                    .map_err(|e| format!("Error parsing script from file: {}", e))?;
            }
        }
    }

    let input_files = if sed_config.commands.is_empty() {
        if let Some(first_arg) = cli.files.first() {
            let script = first_arg.to_string_lossy();
            sed_config
                .add_script(&script)
                .map_err(|e| format!("Error parsing script argument: {}", e))?;
            &cli.files[1..]
        } else {
            return Err(
                "No sed script provided. Use -e, -f, or provide script as first argument.".into(),
            );
        }
    } else {
        &cli.files[..]
    };

    if input_files.is_empty() {
        let stdin = io::stdin();
        let stdout = io::stdout();
        let stdin_lock = stdin.lock();
        let mut stdout_lock = stdout.lock();
        sed_config.execute(stdin_lock, &mut stdout_lock)?;
    } else {
        for file_path in input_files {
            let validated_path = validate_file_path(file_path)?;
            let file = File::open(&validated_path)?;
            let reader = BufReader::new(file);

            if cli.in_place {
                use std::io::BufWriter;
                use tempfile::NamedTempFile;

                let parent = validated_path
                    .parent()
                    .ok_or_else(|| -> Box<dyn std::error::Error> { "Invalid file path".into() })?;

                let temp_file = NamedTempFile::new_in(parent)?;
                {
                    let mut writer = BufWriter::new(&temp_file);
                    sed_config.execute(reader, &mut writer)?;
                    writer.flush()?;
                }
                temp_file.as_file().sync_all()?;
                temp_file.persist(validated_path)?;
            } else {
                let stdout = io::stdout();
                let mut stdout_lock = stdout.lock();
                sed_config.execute(reader, &mut stdout_lock)?;
            }
        }
    }

    Ok(())
}
