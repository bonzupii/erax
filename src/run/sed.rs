//! Sed (stream editing) mode with interactive diff review.
//!
//! This is *interactive* sed. Every transformation is presented as
//! accept/reject diff hunks before anything is applied; batch-only
//! execution is not a mode of this tool. A script comes only from an
//! explicit `-e`/`-f`; a positional filename is never reinterpreted as a
//! script.

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, IsTerminal, Write};
use std::path::Path;

use crate::cli::Invocation;
use crate::sed::diff::{DiffKind, DiffView, Hunk};
use crate::sed::{SedConfig, SedError};

use crate::core::path::{PathError, validate_file_path};

/// Typed errors for sed-mode invocation and review.
#[derive(Debug)]
pub enum SedRunError {
    /// No script was given via `-e`/`-f`; positionals are never guessed as scripts.
    NoScript,
    /// A script failed to parse.
    ScriptParse(SedError),
    /// Reading a script file failed.
    ScriptFile(PathError),
    /// An input file failed path validation.
    BadInputPath(PathError),
    /// Reading input failed.
    InputIo(std::io::Error),
    /// Writing output failed.
    OutputIo(std::io::Error),
    /// Interactive review needs a terminal for the accept/reject prompts, and
    /// none was available (principle 11: batch-only is not a mode).
    NoReviewTerminal,
    /// The user aborted the review.
    ReviewAborted,
}

impl std::fmt::Display for SedRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SedRunError::NoScript => {
                write!(f, "no sed script provided; use -e or -f (a filename is never a script)")
            }
            SedRunError::ScriptParse(e) => write!(f, "error parsing script: {e}"),
            SedRunError::ScriptFile(e) => write!(f, "cannot read script file: {e}"),
            SedRunError::BadInputPath(e) => write!(f, "bad input path: {e}"),
            SedRunError::InputIo(e) => write!(f, "error reading input: {e}"),
            SedRunError::OutputIo(e) => write!(f, "error writing output: {e}"),
            SedRunError::NoReviewTerminal => write!(
                f,
                "sed mode needs an interactive terminal for diff review; batch-only execution is not supported"
            ),
            SedRunError::ReviewAborted => write!(f, "review aborted; nothing was applied"),
        }
    }
}

/// Run in sed mode: transform, review hunks interactively, apply accepted ones.
pub fn run_sed_mode(inv: &Invocation) -> Result<(), SedRunError> {
    let mut sed_config = SedConfig::new();
    sed_config.quiet = inv.sed.quiet;

    for script in &inv.sed.expressions {
        sed_config
            .add_script(script)
            .map_err(SedRunError::ScriptParse)?;
    }

    if let Some(script_path) = &inv.sed.script_file {
        let validated = validate_file_path(script_path).map_err(SedRunError::ScriptFile)?;
        let file = File::open(&validated).map_err(SedRunError::InputIo)?;
        for line in BufReader::new(file).lines() {
            let line = line.map_err(SedRunError::InputIo)?;
            if !line.trim().is_empty() && !line.starts_with('#') {
                sed_config
                    .add_script(&line)
                    .map_err(SedRunError::ScriptParse)?;
            }
        }
    }

    // A script must come from -e/-f. A positional filename is input,
    // never a script.
    if sed_config.commands.is_empty() {
        return Err(SedRunError::NoScript);
    }

    if inv.files.is_empty() {
        // Stdin → stdout with review.
        let stdin = io::stdin();
        let input: Vec<String> = stdin
            .lock()
            .lines()
            .collect::<Result<_, _>>()
            .map_err(SedRunError::InputIo)?;
        let reviewed = review_transform(&sed_config, &input, "<stdin>")?;
        let stdout = io::stdout();
        let mut out = stdout.lock();
        for line in &reviewed {
            writeln!(out, "{line}").map_err(SedRunError::OutputIo)?;
        }
        out.flush().map_err(SedRunError::OutputIo)?;
    } else {
        for file_path in &inv.files {
            let validated =
                validate_file_path(file_path).map_err(SedRunError::BadInputPath)?;
            let file = File::open(&validated).map_err(SedRunError::InputIo)?;
            let input: Vec<String> = BufReader::new(file)
                .lines()
                .collect::<Result<_, _>>()
                .map_err(SedRunError::InputIo)?;
            let reviewed =
                review_transform(&sed_config, &input, &validated.display().to_string())?;

            if inv.sed.in_place {
                write_in_place(&validated, &reviewed)?;
            } else {
                let stdout = io::stdout();
                let mut out = stdout.lock();
                for line in &reviewed {
                    writeln!(out, "{line}").map_err(SedRunError::OutputIo)?;
                }
                out.flush().map_err(SedRunError::OutputIo)?;
            }
        }
    }

    Ok(())
}

/// Transform the input, present the diff hunks for accept/reject review, and
/// return the reviewed output lines.
fn review_transform(
    sed_config: &SedConfig,
    input: &[String],
    label: &str,
) -> Result<Vec<String>, SedRunError> {
    let input_refs: Vec<&str> = input.iter().map(String::as_str).collect();
    let output = sed_config
        .transform(&input_refs)
        .map_err(SedRunError::ScriptParse)?;

    let original = input.join("\n");
    let modified = output.join("\n");
    if original == modified {
        return Ok(output);
    }

    let view = DiffView::new(original, modified);
    let hunks = view.compute_hunks();

    let mut review = Review::new(label)?;
    let mut accepted: Vec<bool> = Vec::with_capacity(hunks.len());
    for (i, hunk) in hunks.iter().enumerate() {
        accepted.push(review.prompt_hunk(i, hunks.len(), hunk)?);
    }

    // Rebuild output: accepted hunks take new lines, rejected keep old.
    Ok(apply_review(input, &output, &hunks, &accepted))
}

/// Rebuild the output lines from the review decisions.
///
/// Each hunk covers `old_span` in the input and `new_span` in the transformed
/// output; accepted hunks splice the new span in, rejected hunks keep the old.
fn apply_review(
    input: &[String],
    output: &[String],
    hunks: &[Hunk<'_>],
    accepted: &[bool],
) -> Vec<String> {
    let mut result = Vec::with_capacity(output.len().max(input.len()));
    let mut old_pos = 0; // 0-based line index into input
    for (hunk, &accept) in hunks.iter().zip(accepted.iter()) {
        let (old_start, old_end) = hunk.old_span; // 1-based, end exclusive
        // Copy unchanged lines before this hunk.
        while old_pos < old_start.saturating_sub(1) && old_pos < input.len() {
            result.push(input[old_pos].clone());
            old_pos += 1;
        }
        if accept {
            let (new_start, new_end) = hunk.new_span;
            for line in output
                .iter()
                .take(new_end.saturating_sub(1))
                .skip(new_start.saturating_sub(1))
            {
                result.push(line.clone());
            }
        } else {
            for line in input
                .iter()
                .take(old_end.saturating_sub(1))
                .skip(old_start.saturating_sub(1))
            {
                result.push(line.clone());
            }
        }
        old_pos = old_end.saturating_sub(1).max(old_pos);
    }
    while old_pos < input.len() {
        result.push(input[old_pos].clone());
        old_pos += 1;
    }
    result
}

/// Interactive hunk review prompter.
struct Review {
    label: String,
    tty: Box<dyn BufRead>,
    accept_all: bool,
}

impl Review {
    fn new(label: &str) -> Result<Self, SedRunError> {
        // Prompts go to the terminal: stdin if it is a TTY, else /dev/tty.
        // Without any terminal, batch-only execution is refused (principle 11).
        let tty: Box<dyn BufRead> = if io::stdin().is_terminal() {
            Box::new(BufReader::new(io::stdin()))
        } else {
            match File::open("/dev/tty") {
                Ok(f) => Box::new(BufReader::new(f)),
                Err(_) => return Err(SedRunError::NoReviewTerminal),
            }
        };
        Ok(Self {
            label: label.to_string(),
            tty,
            accept_all: false,
        })
    }

    /// Present one hunk and return true if accepted.
    fn prompt_hunk(
        &mut self,
        index: usize,
        total: usize,
        hunk: &Hunk<'_>,
    ) -> Result<bool, SedRunError> {
        if self.accept_all {
            return Ok(true);
        }
        let stderr = io::stderr();
        let mut err = stderr.lock();
        writeln!(
            err,
            "--- {}: hunk {}/{} (lines {}-{}) ---",
            self.label,
            index + 1,
            total,
            hunk.old_span.0,
            hunk.old_span.1.saturating_sub(1).max(hunk.old_span.0),
        )
        .map_err(SedRunError::OutputIo)?;
        for line in &hunk.lines {
            let marker = match line.kind {
                DiffKind::Context => ' ',
                DiffKind::Removed => '-',
                DiffKind::Added => '+',
            };
            writeln!(err, "{marker} {}", line.text).map_err(SedRunError::OutputIo)?;
        }
        write!(err, "Accept this hunk? [y]es / [n]o / [a]ll / [q]uit: ").map_err(SedRunError::OutputIo)?;
        err.flush().map_err(SedRunError::OutputIo)?;

        loop {
            let mut answer = String::new();
            self.tty
                .read_line(&mut answer)
                .map_err(SedRunError::InputIo)?;
            match answer.trim().to_lowercase().as_str() {
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                "a" | "all" => {
                    self.accept_all = true;
                    return Ok(true);
                }
                "q" | "quit" => return Err(SedRunError::ReviewAborted),
                _ => {
                    write!(err, "Please answer y, n, a, or q: ").map_err(SedRunError::OutputIo)?;
                    err.flush().map_err(SedRunError::OutputIo)?;
                }
            }
        }
    }
}

/// Atomically write reviewed lines back to the file (temp file + rename).
fn write_in_place(path: &Path, lines: &[String]) -> Result<(), SedRunError> {
    use tempfile::NamedTempFile;

    let parent = path
        .parent()
        .ok_or(SedRunError::BadInputPath(PathError::MissingFileName))?;
    let temp = NamedTempFile::new_in(parent).map_err(SedRunError::OutputIo)?;
    {
        let mut writer = BufWriter::new(&temp);
        for line in lines {
            writeln!(writer, "{line}").map_err(SedRunError::OutputIo)?;
        }
        writer.flush().map_err(SedRunError::OutputIo)?;
    }
    temp.as_file()
        .sync_all()
        .map_err(SedRunError::OutputIo)?;
    temp.persist(path).map_err(|e| {
        // PersistError carries the temp path and the io error; keep
        // both in the message instead of discarding them.
        SedRunError::OutputIo(io::Error::other(format!(
            "failed to replace {}: {} (temp file: {})",
            path.display(),
            e.error,
            e.file.path().display(),
        )))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_script_is_a_typed_error_not_a_guessed_filename() {
        // Without -e/-f, the first positional is input, never a script.
        let inv = Invocation {
            mode: crate::cli::InvocationMode::Sed,
            degradation: None,
            files: vec![Path::new("somefile.txt").to_path_buf()],
            theme: None,
            wrap: None,
            sed: crate::cli::SedOptions::default(),
            web_bind: "127.0.0.1:8080".to_string(),
            web_allow_remote: false,
        };
        let result = run_sed_mode(&inv);
        assert!(matches!(result, Err(SedRunError::NoScript)));
    }

    #[test]
    fn apply_review_accepts_and_rejects_per_hunk() {
        let input = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let output = vec!["a".to_string(), "B".to_string(), "c".to_string()];
        let view = DiffView::new(input.join("\n"), output.join("\n"));
        let hunks = view.compute_hunks();
        assert!(!hunks.is_empty());

        // Accept: output wins.
        let accepted = apply_review(&input, &output, &hunks, &vec![true; hunks.len()]);
        assert_eq!(accepted, output);
        // Reject: input wins.
        let rejected = apply_review(&input, &output, &hunks, &vec![false; hunks.len()]);
        assert_eq!(rejected, input);
    }

    #[test]
    fn apply_review_rejects_all_hunks() {
        let input = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let output = vec!["a".to_string(), "B".to_string(), "C".to_string()];
        let view = DiffView::new(input.join("\n"), output.join("\n"));
        let hunks = view.compute_hunks();
        assert!(!hunks.is_empty());

        // Reject everything: input wins verbatim.
        let rejected = apply_review(&input, &output, &hunks, &vec![false; hunks.len()]);
        assert_eq!(rejected, input);
    }
}
