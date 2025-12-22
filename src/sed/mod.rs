use regex::Regex;
use std::io::{self, BufRead};

pub mod diff;

/// Sed command types
#[derive(Debug, Clone)]
pub enum Command {
    /// Substitute pattern with replacement
    Substitute {
        pattern: Regex,
        replacement: String,
        global: bool,
        print: bool,
    },
    /// Delete line
    Delete,
    /// Print line
    Print,
}

/// Address specification for commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Address {
    /// Specific line number (1-indexed)
    Line(usize),
    /// Range of lines (start, end) inclusive, 1-indexed
    Range(usize, usize),
    /// All lines
    All,
}

/// A sed command with its address
#[derive(Debug, Clone)]
pub struct SedCommand {
    pub address: Address,
    pub command: Command,
}

/// Sed script executor configuration
#[derive(Debug)]
pub struct SedConfig {
    /// Quiet mode - suppress automatic printing
    pub quiet: bool,
    /// Commands to execute
    pub commands: Vec<SedCommand>,
}

impl SedConfig {
    pub fn new() -> Self {
        Self {
            quiet: false,
            commands: Vec::new(),
        }
    }

    /// Parse a sed script string and add it to commands
    pub fn add_script(&mut self, script: &str) -> Result<(), String> {
        let cmd = parse_sed_command(script)?;
        self.commands.push(cmd);
        Ok(())
    }

    /// Execute sed commands on input
    pub fn execute<R: BufRead, W: io::Write>(&self, reader: R, mut writer: W) -> io::Result<()> {
        let mut line_num = 0;

        for line_result in reader.lines() {
            let mut line = line_result?;
            line_num += 1;
            let mut print_line = !self.quiet;
            let mut deleted = false;

            for sed_cmd in &self.commands {
                // Check if command applies to this line
                if !matches_address(&sed_cmd.address, line_num) {
                    continue;
                }

                match &sed_cmd.command {
                    Command::Substitute {
                        pattern,
                        replacement,
                        global,
                        print: print_flag,
                    } => {
                        if *global {
                            line = pattern.replace_all(&line, replacement).to_string();
                        } else {
                            line = if pattern.is_match(&line) {
                                pattern.replacen(&line, 1, replacement).to_string()
                            } else {
                                line
                            };
                        }
                        if *print_flag {
                            writeln!(writer, "{}", line)?;
                        }
                    }
                    Command::Delete => {
                        deleted = true;
                        print_line = false;
                        break; // Stop processing this line
                    }
                    Command::Print => {
                        writeln!(writer, "{}", line)?;
                    }
                }
            }

            // Print line if not deleted and auto-print is on
            if !deleted && print_line {
                writeln!(writer, "{}", line)?;
            }
        }

        Ok(())
    }
}

/// Check if an address matches the current line number
fn matches_address(address: &Address, line_num: usize) -> bool {
    match address {
        Address::All => true,
        Address::Line(n) => line_num == *n,
        Address::Range(start, end) => line_num >= *start && line_num <= *end,
    }
}

/// Parse a sed command string
pub fn parse_sed_command(script: &str) -> Result<SedCommand, String> {
    let script = script.trim();

    // Try to parse address prefix
    let (address, cmd_part) = parse_address(script)?;

    // Parse the command
    let command = if cmd_part.starts_with('s') {
        parse_substitute(cmd_part)?
    } else if cmd_part == "d" {
        Command::Delete
    } else if cmd_part == "p" {
        Command::Print
    } else {
        return Err(format!("Unknown command: {}", cmd_part));
    };

    Ok(SedCommand { address, command })
}

/// Parse address portion of a sed command
fn parse_address(script: &str) -> Result<(Address, &str), String> {
    // Check for range: "1,10s/foo/bar/"
    if let Some(comma_pos) = script.find(',') {
        let start_str = &script[..comma_pos];
        let rest = &script[comma_pos + 1..];

        // Find where the command starts (first non-digit character)
        let end_pos = rest
            .chars()
            .position(|c| !c.is_ascii_digit())
            .ok_or("Invalid range format")?;

        let end_str = &rest[..end_pos];
        let cmd_part = &rest[end_pos..];

        let start: usize = start_str.parse().map_err(|_| "Invalid start line number")?;
        let end: usize = end_str.parse().map_err(|_| "Invalid end line number")?;

        if start == 0 || end == 0 {
            return Err("Line numbers must be >= 1".to_string());
        }

        return Ok((Address::Range(start, end), cmd_part));
    }

    // Check for single line: "2d"
    let first_non_digit = script.chars().position(|c| !c.is_ascii_digit());

    if let Some(pos) = first_non_digit {
        if pos > 0 {
            let line_str = &script[..pos];
            let cmd_part = &script[pos..];
            let line_num: usize = line_str.parse().map_err(|_| "Invalid line number")?;

            if line_num == 0 {
                return Err("Line numbers must be >= 1".to_string());
            }

            return Ok((Address::Line(line_num), cmd_part));
        }
    }

    // No address specified - applies to all lines
    Ok((Address::All, script))
}

/// Parse substitute command: s/pattern/replacement/[flags]
fn parse_substitute(cmd: &str) -> Result<Command, String> {
    if !cmd.starts_with('s') {
        return Err("Not a substitute command".to_string());
    }

    // Find delimiter (typically '/')
    let delimiter = cmd
        .chars()
        .nth(1)
        .ok_or_else(|| "Invalid substitute format: expected s/pattern/replacement/".to_string())?;

    let parts: Vec<&str> = cmd[2..].split(delimiter).collect();

    if parts.len() < 2 {
        return Err("Invalid substitute format: expected s/pattern/replacement/".to_string());
    }

    let pattern_str = parts[0];
    let replacement = parts[1];
    let flags = if parts.len() > 2 { parts[2] } else { "" };

    // Parse flags
    let global = flags.contains('g');
    let print = flags.contains('p');

    // Validate pattern string (basic validation)
    if pattern_str.is_empty() {
        return Err("Pattern cannot be empty".to_string());
    }

    // Compile regex pattern
    let regex = Regex::new(pattern_str).map_err(|e| format!("Invalid regex pattern: {}", e))?;

    Ok(Command::Substitute {
        pattern: regex,
        replacement: replacement.to_string(),
        global,
        print,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_substitute_basic() {
        if let Ok(cmd) = parse_sed_command("s/foo/bar/") {
            assert!(matches!(cmd.address, Address::All));
            assert!(matches!(
                cmd.command,
                Command::Substitute {
                    global: false,
                    print: false,
                    ..
                }
            ));
        } else {
            panic!("Expected valid parse");
        }
    }

    #[test]
    fn test_parse_substitute_global() {
        if let Ok(cmd) = parse_sed_command("s/foo/bar/g") {
            if let Command::Substitute { global, .. } = cmd.command {
                assert!(global);
            } else {
                panic!("Expected Substitute command");
            }
        } else {
            panic!("Expected valid parse");
        }
    }

    #[test]
    fn test_parse_delete() {
        if let Ok(cmd) = parse_sed_command("d") {
            assert!(matches!(cmd.command, Command::Delete));
        } else {
            panic!("Expected valid parse");
        }
    }

    #[test]
    fn test_parse_print() {
        if let Ok(cmd) = parse_sed_command("p") {
            assert!(matches!(cmd.command, Command::Print));
        } else {
            panic!("Expected valid parse");
        }
    }

    #[test]
    fn test_parse_line_address() {
        if let Ok(cmd) = parse_sed_command("2d") {
            assert_eq!(cmd.address, Address::Line(2));
        } else {
            panic!("Expected valid parse");
        }
    }

    #[test]
    fn test_parse_range_address() {
        if let Ok(cmd) = parse_sed_command("1,3s/a/b/") {
            assert_eq!(cmd.address, Address::Range(1, 3));
        } else {
            panic!("Expected valid parse");
        }
    }

    #[test]
    fn test_execute_basic_substitute() {
        let mut config = SedConfig::new();
        if config.add_script("s/hello/world/").is_err() {
            panic!("Failed to add script");
        }

        let input = b"hello there\nhello world\n";
        let mut output = Vec::new();

        if config.execute(&input[..], &mut output).is_err() {
            panic!("Failed to execute");
        }

        if let Ok(result) = String::from_utf8(output) {
            assert_eq!(result, "world there\nworld world\n");
        } else {
            panic!("Invalid UTF-8");
        }
    }

    #[test]
    fn test_execute_delete() {
        let mut config = SedConfig::new();
        if config.add_script("2d").is_err() {
            panic!("Failed to add script");
        }

        let input = b"line1\nline2\nline3\n";
        let mut output = Vec::new();

        if config.execute(&input[..], &mut output).is_err() {
            panic!("Failed to execute");
        }

        if let Ok(result) = String::from_utf8(output) {
            assert_eq!(result, "line1\nline3\n");
        } else {
            panic!("Invalid UTF-8");
        }
    }

    #[test]
    fn test_execute_quiet_print() {
        let mut config = SedConfig::new();
        config.quiet = true;
        if config.add_script("1p").is_err() {
            panic!("Failed to add script");
        }

        let input = b"line1\nline2\nline3\n";
        let mut output = Vec::new();

        if config.execute(&input[..], &mut output).is_err() {
            panic!("Failed to execute");
        }

        if let Ok(result) = String::from_utf8(output) {
            assert_eq!(result, "line1\n");
        } else {
            panic!("Invalid UTF-8");
        }
    }
}
