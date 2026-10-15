//! Structural guard for the bindings-are-data doctrine.
//!
//! Every key, key sequence, and command-line word that invokes a command
//! is data in `src/config`; engines interpret the tables and never
//! hardcode a key-to-command or word-to-command mapping. This test scans
//! every `src/**/*.rs` file for `Command::` references outside test
//! modules and requires each file's count to match an explicit allowlist
//! entry. A new hardcoded mapping changes a count and fails here, forcing
//! the mapping to move into config or to be justified in the allowlist.
//!
//! Files absent from the allowlist must have zero references; files that
//! declare their own `Command` enum (the sed script parser) are skipped.
//! `std::process::Command` is not the core command type and is ignored.

use std::path::{Path, PathBuf};

/// Allowlist: (file, expected `Command::` count, justification).
const ALLOWLIST: &[(&str, usize, &str)] = &[
    (
        "src/core/modal.rs",
        22,
        "motion/operator application dispatching primitives \
         (SetMark, NextLine, SmartHome, ForwardCharacter, EndOfLine, \
         BeginningOfLine, BackwardCharacter, BeginningOfFile); table-action \
         interpretation (g-prefix ShowPosition, visual KillRegion/CopyRegion \
         as operators, the c-on-KillRegion and ForwardSentence adjustments); \
         ex-table interpretation (ConditionalQuit refusal, from_name \
         fallback, ToggleMenu intercept)",
    ),
    (
        "src/terminal/event_handler.rs",
        2,
        "ToggleMenu display intercept on a table-resolved outcome; \
         middle-click paste gesture dispatches Yank (mouse gesture handling, \
         not a key binding)",
    ),
    (
        "src/core/prompt.rs",
        1,
        "the M-x named-command facility resolves names through \
         Command::from_name",
    ),
    (
        "src/core/menu.rs",
        37,
        "menu presentation items map labels to commands; accelerator hints \
         are display strings, not bindings",
    ),
    (
        "src/core/commands/macro_cmd.rs",
        1,
        "macro replay rejects nested ExecuteMacro",
    ),
    (
        "src/core/dispatcher.rs",
        124,
        "the dispatcher: the one place mapping commands to implementations",
    ),
];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Blank strings and comments, preserving newlines so line numbers hold.
fn blank_code(src: &str) -> String {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut out = src.to_string().into_bytes();
    let mut i = 0;
    let blank = |out: &mut [u8], a: usize, b: usize| {
        for k in a..b.min(out.len()) {
            if out[k] != b'\n' {
                out[k] = b' ';
            }
        }
    };
    while i < n {
        let c = bytes[i];
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            let mut j = i;
            while j < n && bytes[j] != b'\n' {
                j += 1;
            }
            blank(&mut out, i, j);
            i = j;
        } else if c == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            let mut j = i + 2;
            let mut depth = 1;
            while j < n && depth > 0 {
                if j + 1 < n && bytes[j] == b'/' && bytes[j + 1] == b'*' {
                    depth += 1;
                    j += 2;
                } else if j + 1 < n && bytes[j] == b'*' && bytes[j + 1] == b'/' {
                    depth -= 1;
                    j += 2;
                } else {
                    j += 1;
                }
            }
            blank(&mut out, i, j);
            i = j;
        } else if c == b'r' && is_raw_string_open(bytes, i) {
            let mut hashes = 0;
            let mut k = i + 1;
            while k < n && bytes[k] == b'#' {
                hashes += 1;
                k += 1;
            }
            let close: Vec<u8> = std::iter::once(b'"')
                .chain(std::iter::repeat_n(b'#', hashes))
                .collect();
            let mut j = k + 1;
            let mut end = n;
            while j + close.len() <= n {
                if &bytes[j..j + close.len()] == close.as_slice() {
                    end = j + close.len();
                    break;
                }
                j += 1;
            }
            blank(&mut out, i, end);
            i = end;
        } else if (c == b'"' || (c == b'b' && i + 1 < n && bytes[i + 1] == b'"'))
            && !is_raw_string_open(bytes, i)
        {
            let mut j = if c == b'b' { i + 2 } else { i + 1 };
            while j < n {
                if bytes[j] == b'\\' {
                    j += 2;
                    continue;
                }
                if bytes[j] == b'"' {
                    j += 1;
                    break;
                }
                j += 1;
            }
            blank(&mut out, i, j);
            i = j;
        } else if c == b'\'' && is_char_literal(bytes, i) {
            let mut j = i + 1;
            if j < n && bytes[j] == b'\\' {
                j += 1;
                if j < n && bytes[j] == b'u' {
                    while j < n && bytes[j] != b'\'' {
                        j += 1;
                    }
                } else {
                    j += 1;
                }
            } else {
                j += 1;
            }
            if j < n && bytes[j] == b'\'' {
                j += 1;
            }
            blank(&mut out, i, j);
            i = j;
        } else {
            i += 1;
        }
    }
    String::from_utf8(out).expect("blanking preserves UTF-8")
}

fn is_raw_string_open(bytes: &[u8], i: usize) -> bool {
    if bytes[i] != b'r' {
        return false;
    }
    let mut k = i + 1;
    while k < bytes.len() && bytes[k] == b'#' {
        k += 1;
    }
    k < bytes.len() && bytes[k] == b'"'
}

/// A `'` starts a char literal when it matches `'x'`, `'\''`, `'\n'`,
/// `'\u{...}'`; otherwise it is a lifetime and only the quote is skipped.
fn is_char_literal(bytes: &[u8], i: usize) -> bool {
    let mut j = i + 1;
    if j >= bytes.len() {
        return false;
    }
    if bytes[j] == b'\\' {
        j += 1;
        if j < bytes.len() && bytes[j] == b'u' {
            while j < bytes.len() && bytes[j] != b'\'' {
                j += 1;
            }
        } else {
            j += 1;
        }
    } else if bytes[j] == b'\'' {
        return false;
    } else {
        j += 1;
    }
    j < bytes.len() && bytes[j] == b'\''
}

/// Blank every `#[cfg(test)] mod <name> { ... }` region, keeping newlines.
fn strip_test_mods(src: &str, code: &str) -> String {
    let mut out = src.to_string().into_bytes();
    let code_bytes = code.as_bytes();
    let n = code_bytes.len();
    let mut i = 0;
    while i < n {
        let rel = match find_sub(&code_bytes[i..], b"#[cfg(test)]") {
            Some(r) => r,
            None => break,
        };
        let a = i + rel;
        let mut k = a + "#[cfg(test)]".len();
        while k < n && (code_bytes[k] == b' ' || code_bytes[k] == b'\t' || code_bytes[k] == b'\n')
        {
            k += 1;
        }
        if code_bytes[k..].starts_with(b"mod ") {
            let mut m = k + 4;
            while m < n && (code_bytes[m].is_ascii_alphanumeric() || code_bytes[m] == b'_') {
                m += 1;
            }
            let mut q = m;
            while q < n
                && (code_bytes[q] == b' '
                    || code_bytes[q] == b'\t'
                    || code_bytes[q] == b'\n')
            {
                q += 1;
            }
            if q < n && code_bytes[q] == b'{' {
                let mut p = q;
                let mut depth = 0;
                while p < n {
                    if code_bytes[p] == b'{' {
                        depth += 1;
                    } else if code_bytes[p] == b'}' {
                        depth -= 1;
                        if depth == 0 {
                            p += 1;
                            break;
                        }
                    }
                    p += 1;
                }
                for b in out.iter_mut().take(p).skip(a) {
                    if *b != b'\n' {
                        *b = b' ';
                    }
                }
                i = p;
                continue;
            }
        }
        i = a + 1;
    }
    String::from_utf8(out).expect("blanking preserves UTF-8")
}

fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

/// Count core `Command::` references in production code of one file.
/// Returns `None` for files declaring their own `Command` enum.
fn count_file(path: &Path) -> Option<Vec<(usize, String)>> {
    let src = std::fs::read_to_string(path).expect("source file readable");
    if src.contains("pub enum Command") {
        return None;
    }
    let code = blank_code(&src);
    let prod = strip_test_mods(&src, &code);
    let pcode = blank_code(&prod).replace("std::process::Command", "                      ");
    let mut occurrences = Vec::new();
    let mut search_from = 0;
    while let Some(rel) = find_sub(&pcode.as_bytes()[search_from..], b"Command::") {
        let abs = search_from + rel;
        let line_no = pcode[..abs].bytes().filter(|&b| b == b'\n').count() + 1;
        let line = prod
            .lines()
            .nth(line_no - 1)
            .unwrap_or("")
            .trim()
            .to_string();
        occurrences.push((line_no, line));
        search_from = abs + 1;
    }
    Some(occurrences)
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("src dir readable");
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn engine_files_reference_commands_only_through_the_allowlist() {
    let root = manifest_dir();
    let src = root.join("src");
    let mut files = Vec::new();
    collect_rs_files(&src, &mut files);
    let mut failures = Vec::new();
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        // The type's own definition and the binding tables are exempt:
        // they are the doctrine's endpoints, not engines.
        if rel == "src/core/command.rs" || rel.starts_with("src/config/") {
            continue;
        }
        let occurrences = match count_file(path) {
            Some(o) => o,
            None => continue,
        };
        let expected = ALLOWLIST.iter().find(|(f, _, _)| *f == rel).map(|(_, n, _)| *n);
        match expected {
            Some(n) if n == occurrences.len() => {}
            Some(n) => failures.push(format!(
                "{rel}: expected {n} `Command::` references, found {}:\n{}",
                occurrences.len(),
                occurrences
                    .iter()
                    .map(|(ln, text)| format!("  {rel}:{ln}: {text}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )),
            None if occurrences.is_empty() => {}
            None => failures.push(format!(
                "{rel}: {n} `Command::` references but no allowlist entry; \
                 move the mapping into src/config or justify the entry",
                n = occurrences.len()
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "bindings-are-data violations:\n{}",
        failures.join("\n")
    );
}

#[test]
fn allowlist_covers_every_config_table_consumer() {
    // The scan's file list is derived from the tree, so a new engine file
    // with a hardcoded mapping fails the count test above. This test pins
    // the known table consumers: the engines that must keep reading config.
    let root = manifest_dir();
    for consumer in ["src/core/modal.rs", "src/terminal/event_handler.rs"] {
        let src = std::fs::read_to_string(root.join(consumer)).expect("readable");
        assert!(
            src.contains("config::keys"),
            "{consumer} must read its mappings from the config tables"
        );
    }
    // The M-x facility's mapping is the typed command name table itself.
    let prompt = std::fs::read_to_string(root.join("src/core/prompt.rs")).expect("readable");
    assert!(
        prompt.contains("Command::from_name"),
        "src/core/prompt.rs must resolve names through the typed command table"
    );
}

/// Variants of the `Command` enum exempt from the config inventory, each
/// with the reason recorded. Keep this empty by default: a command is a
/// promise of behavior, and the promise belongs in config.
const INVENTORY_EXCEPTIONS: &[(&str, &str)] = &[];

/// Every `Command` variant is named in `src/config/keys.rs` — bound in a
/// model table, recorded as a named-facility binding, or listed by name
/// in `INVENTORY_EXCEPTIONS`. This is structural: it asserts presence of
/// the variant's mention, never any binding value. A new command that is
/// not recorded in the config module fails here.
#[test]
fn config_module_is_the_complete_binding_inventory() {
    let root = manifest_dir();
    let command_src =
        std::fs::read_to_string(root.join("src/core/command.rs")).expect("readable");
    let keys_src =
        std::fs::read_to_string(root.join("src/config/keys.rs")).expect("readable");
    let variants = command_variants(&command_src);
    assert!(
        variants.len() > 100,
        "expected the full command enum, parsed {}",
        variants.len()
    );
    let mut failures = Vec::new();
    for variant in &variants {
        if INVENTORY_EXCEPTIONS.iter().any(|(name, _)| name == variant) {
            continue;
        }
        let needle = format!("Command::{variant}");
        if !references_command(&keys_src, &needle) {
            failures.push(variant.clone());
        }
    }
    assert!(
        failures.is_empty(),
        "commands absent from src/config/keys.rs: {} — bind them in a \
         model table, record them in FACILITY_BINDINGS, or add them to \
         INVENTORY_EXCEPTIONS with a reason",
        failures.join(", ")
    );
}

/// Variant names of `pub enum Command`, in declaration order.
fn command_variants(src: &str) -> Vec<String> {
    let start = src
        .find("pub enum Command {")
        .expect("the command enum is defined");
    let body = &src[start..];
    let mut depth = 0usize;
    let mut end = body.len();
    for (i, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    let mut variants = Vec::new();
    for line in body[..end].lines() {
        let trimmed = line.trim();
        let name: String = trimmed
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty()
            && name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && trimmed[name.len()..]
                .chars()
                .next()
                .is_none_or(|c| c == ',' || c == '(' || c == '{' || c == '=')
        {
            variants.push(name);
        }
    }
    variants
}

/// True when `needle` (`Command::Name`) occurs in `src` followed by a
/// non-identifier byte, so `Command::Exit` does not match `Command::ExitErax`.
fn references_command(src: &str, needle: &str) -> bool {
    let bytes = src.as_bytes();
    let nbytes = needle.as_bytes();
    bytes
        .windows(nbytes.len())
        .enumerate()
        .any(|(i, w)| {
            w == nbytes
                && src.as_bytes()[i + nbytes.len()..]
                    .first()
                    .is_none_or(|b| !(b.is_ascii_alphanumeric() || *b == b'_'))
        })
}
