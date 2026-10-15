//! Sed script engine: parsing, in-memory transformation, and streaming execution.
//!
//! This module implements a documented subset of sed (N-136), aimed at the
//! interactive use case: a script transforms text, and the result is reviewed
//! as accept/reject diff hunks (see [`diff`]) before anything is applied.
//!
//! ## Supported subset
//!
//! **Commands.** `s` (substitute), `d` (delete), `p` (print). Any other
//! command letter is [`SedError::UnknownCommand`].
//!
//! **Addresses.** `[addr][,addr]`, parsed *before* the command letter so that
//! a comma inside a substitution can never be mistaken for a range separator
//! (N-134). Each `addr` is one of:
//!
//! - a 1-based line number (`5`),
//! - `$` — the last line,
//! - `/regex/` — a line whose text matches the regex (unanchored).
//!
//! **Ranges.** GNU sed semantics: a range opens on the first line matching
//! the start address (that line is included) and stays open through the first
//! *later* line matching the end address (that line is included too). A
//! numeric end address less than or equal to the line where the range opened
//! selects only that one line, so `3,1d` deletes just line 3.
//!
//! **Substitute.** `s<delim>pattern<delim>replacement<delim>[flags]`:
//!
//! - the delimiter is any ASCII character except `\`, newline, or an
//!   alphanumeric; a `\` escapes the next character anywhere in the command,
//!   so `s/foo\/bar/baz/` works (N-135);
//! - replacement expansion: `&` is the whole match, `\N` is capture group N
//!   (a single digit; `\0` is the whole match, like GNU sed), `\&` and `\\`
//!   are literal, `\<anything else>` is that character, and `$` is literal
//!   (the regex crate's `$1` syntax is translated to, never exposed);
//! - flags: `g` (global), `p` (print the line), `I` (case-insensitive match).
//!   Any other flag letter is [`SedError::UnknownFlag`].
//!
//! ## Deliberately unsupported (typed errors, never silent)
//!
//! - The `w` flag (write the pattern space to a file): it would write files
//!   as a side effect of [`SedConfig::transform`], bypassing the interactive
//!   diff review that gates every transformation. Rejected with
//!   [`SedError::UnsupportedFlag`].
//! - The `e` flag (execute the pattern space as a shell command): arbitrary
//!   shell execution has no place in this pipeline. Rejected with
//!   [`SedError::UnsupportedFlag`].
//! - Everything else real sed has that this subset omits: empty `s` patterns
//!   (GNU reuses the most recent regex; this engine keeps no such state),
//!   `0,addr` ranges, `+N`/`~N` offsets, `!` negation, `{...}` blocks,
//!   multi-command scripts (one command per script string), and all other
//!   sed commands.

use regex::Regex;
use std::borrow::Cow;
use std::fmt;
use std::io::{self, BufRead};

pub mod diff;

/// Typed errors for sed script parsing (N-133).
///
/// Every failure to parse or apply a script is one of these variants; there
/// are no stringly-typed errors anywhere in this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SedError {
    /// The script was empty or contained only whitespace.
    EmptyScript,
    /// An address or range was malformed. The payload explains what was
    /// expected, e.g. `"expected a line number, \`$\`, or \`/regex/\`"`.
    InvalidAddress(String),
    /// The `s` command was malformed: bad delimiter, unterminated pattern or
    /// replacement, empty pattern, or a group reference that does not exist.
    /// The payload says what went wrong.
    InvalidSubstitution(String),
    /// A regex (in an address or a substitution) failed to compile. The
    /// payload is the regex crate's error message.
    InvalidRegex(String),
    /// A command letter this engine does not implement.
    UnknownCommand(char),
    /// An `s` flag letter that is not a sed flag at all.
    UnknownFlag(char),
    /// A real sed `s` flag this engine deliberately does not support (`w`
    /// writes to a file, `e` executes shell commands). Rejected at parse time
    /// rather than silently ignored.
    UnsupportedFlag(char),
}

impl fmt::Display for SedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SedError::EmptyScript => write!(f, "empty sed script"),
            SedError::InvalidAddress(detail) => write!(f, "invalid address: {detail}"),
            SedError::InvalidSubstitution(detail) => {
                write!(f, "invalid substitution: {detail}")
            }
            SedError::InvalidRegex(detail) => write!(f, "invalid regex: {detail}"),
            SedError::UnknownCommand(c) => write!(f, "unknown command: {c:?}"),
            SedError::UnknownFlag(c) => write!(f, "unknown substitution flag: {c:?}"),
            SedError::UnsupportedFlag(c) => {
                write!(f, "unsupported substitution flag: {c:?}")
            }
        }
    }
}

impl std::error::Error for SedError {}

/// Sed command types.
#[derive(Debug, Clone)]
pub enum Command {
    /// Substitute pattern with replacement.
    ///
    /// `replacement` is stored in the regex crate's replacement syntax
    /// (`${0}` = whole match, `${N}` = group N, `$$` = literal `$`),
    /// translated from sed's `&` / `\N` syntax at parse time (N-135).
    Substitute {
        pattern: Regex,
        replacement: String,
        global: bool,
        print: bool,
    },
    /// Delete line.
    Delete,
    /// Print line.
    Print,
}

/// Address specification for commands (N-136).
#[derive(Debug, Clone)]
pub enum Address {
    /// All lines (no address given).
    All,
    /// Specific line number, 1-based.
    Line(usize),
    /// Last line (`$`).
    LastLine,
    /// A line whose text matches the regex (`/pattern/`).
    Regex(Regex),
    /// Inclusive range with GNU sed semantics; see [`address_matches`].
    /// The parser never nests ranges.
    Range(Box<Address>, Box<Address>),
}

/// A sed command with its address.
#[derive(Debug, Clone)]
pub struct SedCommand {
    pub address: Address,
    pub command: Command,
}

/// Sed script executor configuration.
#[derive(Debug)]
pub struct SedConfig {
    /// Quiet mode — suppress automatic printing of each line.
    pub quiet: bool,
    /// Commands to execute, in script order.
    pub commands: Vec<SedCommand>,
}

impl SedConfig {
    pub fn new() -> Self {
        Self {
            quiet: false,
            commands: Vec::new(),
        }
    }

    /// Parse one sed script string (a single command) and append it.
    pub fn add_script(&mut self, script: &str) -> Result<(), SedError> {
        let cmd = parse_sed_command(script)?;
        self.commands.push(cmd);
        Ok(())
    }

    /// Transform lines in memory, returning the output lines (N-137).
    ///
    /// This is the pure path used by the interactive review: no I/O and no
    /// filesystem side effects. The `Result` is infallible once every script
    /// went through [`add_script`](SedConfig::add_script); it stays typed so
    /// the whole pipeline speaks [`SedError`] instead of ad-hoc error kinds.
    pub fn transform(&self, input: &[&str]) -> Result<Vec<String>, SedError> {
        let mut runner = LineRunner::new(self, Some(input.len()));
        let mut out = Vec::with_capacity(input.len());
        for text in input {
            runner.push_line(text, &mut |line| out.push(line.to_owned()));
        }
        Ok(out)
    }

    /// Stream input lines through the commands, writing each output line.
    ///
    /// Lines are processed one at a time with a reused line buffer and no
    /// per-match allocation for non-matching substitutions (N-137). The only
    /// exception: a command addressing `$` (last line) needs the total line
    /// count up front, so the input is buffered once in that case.
    pub fn execute<R: BufRead, W: io::Write>(&self, reader: R, mut writer: W) -> io::Result<()> {
        let mut emit = WriteEmit {
            writer: &mut writer,
            error: None,
        };
        if self.uses_last_line() {
            // `$` needs the total line count: buffer the input once.
            let lines: Vec<String> = reader.lines().collect::<io::Result<_>>()?;
            let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            let mut runner = LineRunner::new(self, Some(refs.len()));
            for text in &refs {
                runner.push_line(text, &mut |line| emit.write_line(line));
                if emit.failed() {
                    break;
                }
            }
        } else {
            // Streaming: one line at a time, no buffering.
            let mut runner = LineRunner::new(self, None);
            for line_result in reader.lines() {
                let line = line_result?;
                runner.push_line(&line, &mut |text| emit.write_line(text));
                if emit.failed() {
                    break;
                }
            }
        }
        emit.finish()
    }

    /// Whether any command's address can match the last line, i.e. whether
    /// the total line count must be known before streaming can start.
    fn uses_last_line(&self) -> bool {
        self.commands
            .iter()
            .any(|cmd| address_uses_last_line(&cmd.address))
    }
}

impl Default for SedConfig {
    fn default() -> Self {
        Self::new()
    }
}

fn address_uses_last_line(addr: &Address) -> bool {
    match addr {
        Address::LastLine => true,
        Address::Range(start, end) => {
            address_uses_last_line(start) || address_uses_last_line(end)
        }
        Address::All | Address::Line(_) | Address::Regex(_) => false,
    }
}

/// Incremental per-line executor shared by [`SedConfig::transform`] and
/// [`SedConfig::execute`].
///
/// Line numbers, per-command range state, and the reusable line buffer live
/// here so neither path rebuilds them per line (N-137).
struct LineRunner<'a> {
    commands: &'a [SedCommand],
    range_open: Vec<bool>,
    line_num: usize,
    total_lines: Option<usize>,
    quiet: bool,
    line_buf: String,
}

impl<'a> LineRunner<'a> {
    fn new(config: &'a SedConfig, total_lines: Option<usize>) -> Self {
        Self {
            commands: &config.commands,
            range_open: vec![false; config.commands.len()],
            line_num: 0,
            total_lines,
            quiet: config.quiet,
            line_buf: String::new(),
        }
    }

    /// Process one input line, calling `emit` for each output line in order.
    fn push_line(&mut self, text: &str, emit: &mut dyn FnMut(&str)) {
        self.line_num += 1;
        let line_num = self.line_num;
        let total_lines = self.total_lines;
        let quiet = self.quiet;
        let commands = self.commands;
        let range_open = &mut self.range_open;
        let line_buf = &mut self.line_buf;

        line_buf.clear();
        line_buf.push_str(text);

        let mut deleted = false;
        for (cmd, open) in commands.iter().zip(range_open.iter_mut()) {
            if !address_matches(&cmd.address, line_buf, line_num, total_lines, open) {
                continue;
            }
            match &cmd.command {
                Command::Substitute {
                    pattern,
                    replacement,
                    global,
                    print,
                } => {
                    // `replace_all`/`replacen` return `Cow::Borrowed` when
                    // nothing matches, so a non-matching line costs no
                    // allocation here (N-137).
                    let replaced: Cow<'_, str> = if *global {
                        pattern.replace_all(line_buf.as_str(), replacement.as_str())
                    } else {
                        pattern.replacen(line_buf.as_str(), 1, replacement.as_str())
                    };
                    if let Cow::Owned(new) = replaced {
                        *line_buf = new;
                    }
                    // The `p` flag prints the pattern space whether or not
                    // the substitution matched (verified against GNU sed).
                    if *print {
                        emit(line_buf.as_str());
                    }
                }
                Command::Delete => {
                    deleted = true;
                    break;
                }
                Command::Print => emit(line_buf.as_str()),
            }
        }

        if !deleted && !quiet {
            emit(line_buf.as_str());
        }
    }
}

/// Captures the first I/O error from a `FnMut(&str)` emit closure so
/// [`SedConfig::execute`] can report write failures instead of ignoring them.
struct WriteEmit<'w, W: io::Write> {
    writer: &'w mut W,
    error: Option<io::Error>,
}

impl<W: io::Write> WriteEmit<'_, W> {
    fn write_line(&mut self, line: &str) {
        if self.error.is_none()
            && let Err(e) = writeln!(self.writer, "{line}") {
                self.error = Some(e);
            }
    }

    fn failed(&self) -> bool {
        self.error.is_some()
    }

    fn finish(self) -> io::Result<()> {
        match self.error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// Match a non-range address against one line.
fn single_address_matches(
    addr: &Address,
    line: &str,
    line_num: usize,
    total_lines: Option<usize>,
) -> bool {
    match addr {
        Address::All => true,
        Address::Line(n) => line_num == *n,
        Address::LastLine => Some(line_num) == total_lines,
        Address::Regex(re) => re.is_match(line),
        // The parser never nests ranges; a nested range constructed by hand
        // is treated as not matching rather than recursing with fresh state.
        Address::Range(_, _) => false,
    }
}

/// Match a (possibly range) address against one line, updating `range_open`.
///
/// Range semantics follow GNU sed (verified empirically): the range opens on
/// the first line matching the start address — that line is included — and
/// stays open through the first *later* line matching the end address, which
/// is included too. A numeric end address less than or equal to the line
/// where the range opened selects only that one line.
fn address_matches(
    addr: &Address,
    line: &str,
    line_num: usize,
    total_lines: Option<usize>,
    range_open: &mut bool,
) -> bool {
    let Address::Range(start, end) = addr else {
        return single_address_matches(addr, line, line_num, total_lines);
    };
    if *range_open {
        if single_address_matches(end, line, line_num, total_lines) {
            *range_open = false;
        }
        return true;
    }
    if !single_address_matches(start, line, line_num, total_lines) {
        return false;
    }
    // The range opened on this line. A numeric end at or before this line
    // selects just this line (GNU); anything else stays open so the end is
    // tested from the following line on.
    *range_open = !matches!(end.as_ref(), Address::Line(n) if *n <= line_num);
    true
}

/// Parse one sed script string (a single command with an optional address).
pub fn parse_sed_command(script: &str) -> Result<SedCommand, SedError> {
    let script = script.trim();
    if script.is_empty() {
        return Err(SedError::EmptyScript);
    }

    // N-134: parse the address grammar FIRST, before the command letter, so a
    // comma inside a substitution (s/a,b/c/) is never read as a range.
    let (address, rest) = parse_address(script)?;
    let rest = rest.trim_start();
    let cmd = rest.chars().next().ok_or_else(|| {
        SedError::InvalidAddress(format!("expected a command after address in {script:?}"))
    })?;

    let command = match cmd {
        's' => parse_substitute(rest)?,
        'd' | 'p' => {
            let tail = rest[cmd.len_utf8()..].trim_start();
            match tail.chars().next() {
                None => {
                    if cmd == 'd' {
                        Command::Delete
                    } else {
                        Command::Print
                    }
                }
                Some(c) => return Err(SedError::UnknownCommand(c)),
            }
        }
        other => return Err(SedError::UnknownCommand(other)),
    };

    Ok(SedCommand { address, command })
}

/// Parse the `[addr][,addr]` address prefix, returning the address and the
/// remaining command text.
fn parse_address(script: &str) -> Result<(Address, &str), SedError> {
    let (first, rest) = parse_single_address(script)?;
    let Some(start) = first else {
        return Ok((Address::All, script));
    };
    let after_comma = match rest.trim_start().strip_prefix(',') {
        Some(after) => after,
        None => return Ok((start, rest)),
    };
    let (second, rest) = parse_single_address(after_comma.trim_start())?;
    match second {
        Some(end) => Ok((Address::Range(Box::new(start), Box::new(end)), rest)),
        None => Err(SedError::InvalidAddress(
            "expected a line number, `$`, or `/regex/` after ','".to_string(),
        )),
    }
}

/// Parse one address atom: a line number, `$`, or `/regex/`.
/// Returns `Ok((None, script))` when the text starts with no address.
fn parse_single_address(script: &str) -> Result<(Option<Address>, &str), SedError> {
    match script.chars().next() {
        Some(c) if c.is_ascii_digit() => {
            let end = script
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(script.len());
            let num: usize = script[..end].parse().map_err(|_| {
                SedError::InvalidAddress(format!("invalid line number {:?}", &script[..end]))
            })?;
            if num == 0 {
                return Err(SedError::InvalidAddress(
                    "line numbers start at 1 (got 0)".to_string(),
                ));
            }
            Ok((Some(Address::Line(num)), &script[end..]))
        }
        Some('$') => Ok((Some(Address::LastLine), &script['$'.len_utf8()..])),
        Some('/') => {
            let (pattern, rest) = split_escaped(&script[1..], '/').map_err(|()| {
                SedError::InvalidAddress("unterminated `/regex/` address".to_string())
            })?;
            if pattern.is_empty() {
                // GNU reuses the most recent regex for `//`; this engine
                // keeps no such state, so an empty address regex is an error.
                return Err(SedError::InvalidAddress(
                    "empty `/regex/` address".to_string(),
                ));
            }
            let regex =
                Regex::new(pattern).map_err(|e| SedError::InvalidRegex(e.to_string()))?;
            Ok((Some(Address::Regex(regex)), rest))
        }
        _ => Ok((None, script)),
    }
}

/// Parse a substitute command: `s<delim>pattern<delim>replacement<delim>[flags]`.
/// `cmd` must start with `'s'`.
fn parse_substitute(cmd: &str) -> Result<Command, SedError> {
    debug_assert!(cmd.starts_with('s'));
    let after_s = &cmd['s'.len_utf8()..];
    let delim = after_s.chars().next().ok_or_else(|| {
        SedError::InvalidSubstitution("expected a delimiter after 's'".to_string())
    })?;
    if !delim.is_ascii() || delim == '\\' || delim == '\n' || delim.is_alphanumeric() {
        return Err(SedError::InvalidSubstitution(format!(
            "invalid substitution delimiter {delim:?}"
        )));
    }

    let body = &after_s[delim.len_utf8()..];
    let (pattern_src, rest) = split_escaped(body, delim)
        .map_err(|()| SedError::InvalidSubstitution("unterminated pattern".to_string()))?;
    // GNU requires the closing delimiter after the replacement as well
    // (`s/a/b` is "unterminated `s' command"), so it is not optional here.
    let (replacement_src, flags_src) = split_escaped(rest, delim)
        .map_err(|()| SedError::InvalidSubstitution("unterminated replacement".to_string()))?;
    if pattern_src.is_empty() {
        // GNU would reuse the most recent regex; there is none in this
        // engine, so an empty pattern is rejected instead of guessed.
        return Err(SedError::InvalidSubstitution(
            "pattern must not be empty".to_string(),
        ));
    }

    let mut global = false;
    let mut print = false;
    let mut case_insensitive = false;
    for flag in flags_src.chars() {
        match flag {
            'g' => global = true,
            'p' => print = true,
            'I' => case_insensitive = true,
            'w' | 'e' => return Err(SedError::UnsupportedFlag(flag)),
            other => return Err(SedError::UnknownFlag(other)),
        }
    }

    let full_pattern = if case_insensitive {
        let mut p = String::with_capacity(pattern_src.len() + 4);
        p.push_str("(?i)");
        p.push_str(pattern_src);
        p
    } else {
        pattern_src.to_string()
    };
    let pattern =
        Regex::new(&full_pattern).map_err(|e| SedError::InvalidRegex(e.to_string()))?;
    let replacement = translate_replacement(replacement_src, &pattern)?;

    Ok(Command::Substitute {
        pattern,
        replacement,
        global,
        print,
    })
}

/// Translate a sed replacement string into the regex crate's replacement
/// syntax (`${0}` = whole match, `${N}` = group N, `$$` = literal `$`).
///
/// Mapping, verified against GNU sed (N-135):
///
/// - `&` → whole match; `\&` → literal `&`;
/// - `\N` for N in 1..=9 → capture group N (a single digit, like GNU, so
///   `\12` is group 1 followed by a literal `2`); `\0` → whole match;
/// - `\\` → literal backslash; `\<anything else>` → that character;
/// - `$` → literal `$` (it is not special in sed replacements).
///
/// Group references use the braced `${N}` form: the regex crate parses an
/// unbraced `$ref` with the longest `[0-9A-Za-z_]` match, so `$0B` would mean
/// the (nonexistent) group named `0B`, not group 0 followed by `B`.
///
/// A `\N` referring to a group the pattern does not have is
/// [`SedError::InvalidSubstitution`], checked here at parse time.
fn translate_replacement(sed: &str, pattern: &Regex) -> Result<String, SedError> {
    // `captures_len` counts group 0 (the whole match), so valid N < groups.
    let groups = pattern.captures_len();
    let mut out = String::with_capacity(sed.len());
    let mut chars = sed.chars();
    while let Some(c) = chars.next() {
        match c {
            '&' => out.push_str("${0}"),
            '$' => out.push_str("$$"),
            '\\' => match chars.next() {
                // A trailing backslash has nothing to escape: keep it literal.
                None => out.push('\\'),
                Some('&') => out.push('&'),
                Some('\\') => out.push('\\'),
                Some(d) if d.is_ascii_digit() => {
                    let n = (d as u8 - b'0') as usize;
                    if n >= groups {
                        return Err(SedError::InvalidSubstitution(format!(
                            "\\{d} refers to group {n}, but the pattern has only {} group(s)",
                            groups - 1
                        )));
                    }
                    out.push_str("${");
                    out.push(d);
                    out.push('}');
                }
                // GNU: `\<ordinary character>` is the character itself.
                Some(other) => out.push(other),
            },
            other => out.push(other),
        }
    }
    Ok(out)
}

/// Split `s` at the first unescaped `delim`, returning the text before it and
/// the text after it. A `\` escapes the next character (the backslash is kept
/// in the head, so the regex crate still sees `\/` and friends).
/// `Err(())` when the delimiter never appears unescaped.
///
/// Byte-wise scanning is sound: both `\` and an ASCII `delim` are single
/// bytes that can never occur inside a multi-byte UTF-8 sequence, and splits
/// only happen at the delimiter byte itself.
fn split_escaped(s: &str, delim: char) -> Result<(&str, &str), ()> {
    debug_assert!(delim.is_ascii());
    let bytes = s.as_bytes();
    let d = delim as u8;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' {
            i += 2; // skip the escaped character, whatever it is
            continue;
        }
        if b == d {
            return Ok((&s[..i], &s[i + 1..]));
        }
        i += 1;
    }
    Err(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(script: &str) -> SedCommand {
        parse_sed_command(script).expect("test script must parse")
    }

    fn run(script: &str, input: &[&str]) -> Vec<String> {
        let mut config = SedConfig::new();
        config.add_script(script).expect("test script must parse");
        config.transform(input).expect("transform is infallible")
    }

    #[test]
    fn parse_substitute_basic() {
        let cmd = parse("s/foo/bar/");
        assert!(matches!(cmd.address, Address::All));
        match cmd.command {
            Command::Substitute {
                pattern,
                replacement,
                global,
                print,
            } => {
                assert_eq!(pattern.as_str(), "foo");
                assert_eq!(replacement, "bar");
                assert!(!global);
                assert!(!print);
            }
            _ => panic!("expected Substitute"),
        }
    }

    #[test]
    fn parse_substitute_global_and_print_flags() {
        let cmd = parse("s/foo/bar/gp");
        match cmd.command {
            Command::Substitute { global, print, .. } => {
                assert!(global);
                assert!(print);
            }
            _ => panic!("expected Substitute"),
        }
    }

    #[test]
    fn parse_delete() {
        let cmd = parse("d");
        assert!(matches!(cmd.command, Command::Delete));
    }

    #[test]
    fn parse_print() {
        let cmd = parse("p");
        assert!(matches!(cmd.command, Command::Print));
    }

    #[test]
    fn parse_line_address() {
        let cmd = parse("2d");
        assert!(matches!(cmd.address, Address::Line(2)));
    }

    #[test]
    fn parse_range_address() {
        let cmd = parse("1,3d");
        match cmd.address {
            Address::Range(start, end) => {
                assert!(matches!(*start, Address::Line(1)));
                assert!(matches!(*end, Address::Line(3)));
            }
            _ => panic!("expected Range"),
        }
    }

    #[test]
    fn parse_last_line_address() {
        let cmd = parse("$d");
        assert!(matches!(cmd.address, Address::LastLine));
    }

    #[test]
    fn parse_regex_address() {
        let cmd = parse("/foo/d");
        match cmd.address {
            Address::Regex(re) => assert_eq!(re.as_str(), "foo"),
            _ => panic!("expected Regex address"),
        }
    }

    #[test]
    fn parse_regex_range() {
        let cmd = parse("/a/,/b/d");
        match cmd.address {
            Address::Range(start, end) => {
                assert!(matches!(*start, Address::Regex(_)));
                assert!(matches!(*end, Address::Regex(_)));
            }
            _ => panic!("expected Range"),
        }
    }

    #[test]
    fn comma_in_substitution_is_not_a_range() {
        // N-134: the comma lives inside the substitution pattern, so the
        // address must be All, not a range.
        let cmd = parse("s/a,b/c/");
        assert!(matches!(cmd.address, Address::All));
        assert_eq!(run("s/a,b/c/", &["a,b", "x"]), ["c", "x"]);

        // ...while a real range still parses as one.
        let cmd = parse("1,2s/a,b/c/");
        assert!(matches!(cmd.address, Address::Range(_, _)));
        assert_eq!(run("1,2s/a,b/c/", &["a,b", "a,b", "a,b"]), ["c", "c", "a,b"]);
    }

    #[test]
    fn escaped_delimiter_in_pattern() {
        // N-135: `\/` must not end the pattern.
        assert_eq!(run("s/foo\\/bar/baz/", &["foo/bar"]), ["baz"]);
        // An alternate delimiter works too.
        assert_eq!(run("s|a|b|", &["a"]), ["b"]);
    }

    #[test]
    fn replacement_expands_whole_match_and_groups() {
        // N-135: `&` is the whole match, `\N` is group N.
        assert_eq!(run("s/(a)(b)/\\2\\1[&]/", &["ab"]), ["ba[ab]"]);
        // `\0` is the whole match, like GNU sed.
        assert_eq!(run("s/y/A\\0B/", &["xy"]), ["xAyB"]);
        // `$` is literal in sed replacements.
        assert_eq!(run("s/a/$1/", &["a"]), ["$1"]);
        // Braced references: `&` followed by a digit is the whole match
        // plus the digit, not a longer group reference.
        assert_eq!(run("s/a/&1/", &["a"]), ["a1"]);
        // `\12` is group 1 followed by a literal `2`, like GNU sed.
        assert_eq!(run("s/(a)(b)/\\12/", &["ab"]), ["a2"]);
    }

    #[test]
    fn replacement_backslash_escapes() {
        // `\&` is a literal `&`, `\\` a literal backslash, `\q` is `q`.
        assert_eq!(run("s/a/\\&\\\\/", &["a"]), ["&\\"]);
        assert_eq!(run("s/a/\\q/", &["a"]), ["q"]);
    }

    #[test]
    fn unknown_substitution_flag_is_an_error() {
        // N-135: unknown flags are typed errors, never silently ignored.
        let err = parse_sed_command("s/a/b/z").expect_err("unknown flag must fail");
        assert_eq!(err, SedError::UnknownFlag('z'));
    }

    #[test]
    fn unsupported_w_and_e_flags_are_errors() {
        // N-135: `w` would bypass the diff review, `e` runs shell commands.
        let err = parse_sed_command("s/a/b/w /tmp/out").expect_err("w must fail");
        assert_eq!(err, SedError::UnsupportedFlag('w'));
        let err = parse_sed_command("s/a/b/e").expect_err("e must fail");
        assert_eq!(err, SedError::UnsupportedFlag('e'));
    }

    #[test]
    fn case_insensitive_flag() {
        assert_eq!(run("s/foo/bar/I", &["FOO", "foo", "FoO"]), ["bar", "bar", "bar"]);
    }

    #[test]
    fn global_replaces_all_first_only_by_default() {
        assert_eq!(run("s/a/X/g", &["aaa"]), ["XXX"]);
        assert_eq!(run("s/a/X/", &["aaa"]), ["Xaa"]);
    }

    #[test]
    fn invalid_regex_is_a_typed_error() {
        let err = parse_sed_command("s/(/x/").expect_err("bad regex must fail");
        assert!(matches!(err, SedError::InvalidRegex(_)));
        let err = parse_sed_command("/(/d").expect_err("bad address regex must fail");
        assert!(matches!(err, SedError::InvalidRegex(_)));
    }

    #[test]
    fn empty_and_unknown_commands_are_errors() {
        assert_eq!(
            parse_sed_command("").expect_err("empty must fail"),
            SedError::EmptyScript
        );
        assert_eq!(
            parse_sed_command("   ").expect_err("blank must fail"),
            SedError::EmptyScript
        );
        assert_eq!(
            parse_sed_command("2q").expect_err("q must fail"),
            SedError::UnknownCommand('q')
        );
        assert!(parse_sed_command("1,2").is_err());
        assert!(parse_sed_command("0d").is_err());
    }

    #[test]
    fn bad_group_reference_is_an_error() {
        let err = parse_sed_command("s/(a)/\\2/").expect_err("bad group must fail");
        assert!(matches!(err, SedError::InvalidSubstitution(_)));
    }

    #[test]
    fn empty_pattern_and_unterminated_are_errors() {
        assert!(matches!(
            parse_sed_command("s//x/").expect_err("empty pattern must fail"),
            SedError::InvalidSubstitution(_)
        ));
        // GNU rejects `s/a/b` as unterminated; so do we.
        assert!(matches!(
            parse_sed_command("s/a/b").expect_err("unterminated must fail"),
            SedError::InvalidSubstitution(_)
        ));
        assert!(parse_sed_command("s/a").is_err());
    }

    #[test]
    fn sed_error_display_mentions_the_problem() {
        assert!(format!("{}", SedError::UnknownFlag('z')).contains('z'));
        assert!(!format!("{}", SedError::EmptyScript).is_empty());
    }

    #[test]
    fn transform_last_line_address() {
        // N-136: `$` is the last line.
        assert_eq!(run("$d", &["a", "b", "c"]), ["a", "b"]);
        assert_eq!(run("2,$s/b/B/", &["a", "b", "c"]), ["a", "B", "c"]);
        assert_eq!(run("1,$d", &["a", "b"]), Vec::<String>::new());
        // `$` on empty input matches nothing.
        assert_eq!(run("$d", &[]), Vec::<String>::new());
    }

    #[test]
    fn transform_regex_address() {
        // N-136: `/pattern/` addresses.
        assert_eq!(run("/b/d", &["a", "b", "c"]), ["a", "c"]);
        assert_eq!(run("/b/,/c/d", &["a", "b", "c", "d"]), ["a", "d"]);
    }

    #[test]
    fn numeric_range_end_before_start_selects_one_line() {
        // GNU: a numeric end <= the line where the range opened selects only
        // that line.
        assert_eq!(run("3,1d", &["a", "b", "c", "d"]), ["a", "b", "d"]);
        assert_eq!(run("2,2s/b/B/", &["a", "b", "c"]), ["a", "B", "c"]);
    }

    #[test]
    fn regex_range_end_is_tested_from_the_next_line() {
        // GNU: `/b/,/b/` on b,c,b,d deletes b,c,b (end tested after start).
        assert_eq!(run("/b/,/b/d", &["a", "b", "c", "b", "d"]), ["a", "d"]);
    }

    #[test]
    fn substitute_print_flag_prints_regardless_of_match() {
        // Verified against GNU sed: `p` prints the pattern space even when
        // the substitution did not match.
        assert_eq!(run("s/a/b/p", &["xa"]), ["xb", "xb"]);
        assert_eq!(run("s/a/b/p", &["xx"]), ["xx", "xx"]);
    }

    #[test]
    fn execute_basic_substitute() {
        let mut config = SedConfig::new();
        config.add_script("s/hello/world/").expect("script must parse");

        let input = b"hello there\nhello world\n";
        let mut output = Vec::new();
        config.execute(&input[..], &mut output).expect("execute must succeed");

        let result = String::from_utf8(output).expect("valid UTF-8");
        assert_eq!(result, "world there\nworld world\n");
    }

    #[test]
    fn execute_delete() {
        let mut config = SedConfig::new();
        config.add_script("2d").expect("script must parse");

        let input = b"line1\nline2\nline3\n";
        let mut output = Vec::new();
        config.execute(&input[..], &mut output).expect("execute must succeed");

        let result = String::from_utf8(output).expect("valid UTF-8");
        assert_eq!(result, "line1\nline3\n");
    }

    #[test]
    fn execute_quiet_print() {
        let mut config = SedConfig::new();
        config.quiet = true;
        config.add_script("1p").expect("script must parse");

        let input = b"line1\nline2\nline3\n";
        let mut output = Vec::new();
        config.execute(&input[..], &mut output).expect("execute must succeed");

        let result = String::from_utf8(output).expect("valid UTF-8");
        assert_eq!(result, "line1\n");
    }

    #[test]
    fn execute_last_line_address_buffers_once() {
        // `$` needs the total line count: execute takes the buffered path.
        let mut config = SedConfig::new();
        config.add_script("$d").expect("script must parse");

        let input = b"a\nb\nc\n";
        let mut output = Vec::new();
        config.execute(&input[..], &mut output).expect("execute must succeed");

        let result = String::from_utf8(output).expect("valid UTF-8");
        assert_eq!(result, "a\nb\n");
    }
}
