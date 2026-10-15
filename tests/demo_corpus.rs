//! Conformance of the generated demo corpus against its canonical expectations.
//!
//! The corpus (demos/UTF-8-demo-lig.txt) and its sidecar
//! (tests/fixtures/demo-expectations.json) are produced by
//! gen_demo_corpus.py from pinned Unicode data files. This test runs the
//! editor's own segmentation and width code over every sample line and
//! asserts the canonical cluster counts and cell widths. Lines whose
//! expectations the editor currently misses live in `expected_failures`
//! as data, never as weakened expectations.

use erax::core::utf8::{grapheme_count, layout_grapheme_width, GraphemeIterator};
use serde_json::Value;
use std::path::PathBuf;

struct Entry {
    line: u64,
    section: String,
    text: String,
    clusters: u64,
    width: u64,
}

fn load_entries() -> Vec<Entry> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read_to_string(root.join("tests/fixtures/demo-expectations.json"))
        .expect("demo-expectations.json is readable");
    let doc: Value = serde_json::from_str(&raw).expect("sidecar is valid JSON");
    doc["entries"]
        .as_array()
        .expect("sidecar has an entries array")
        .iter()
        .map(|e| Entry {
            line: e["line"].as_u64().expect("line is a number"),
            section: e["section"].as_str().expect("section is a string").to_owned(),
            text: e["text"].as_str().expect("text is a string").to_owned(),
            clusters: e["clusters"].as_u64().expect("clusters is a number"),
            width: e["width"].as_u64().expect("width is a number"),
        })
        .collect()
}

/// Lines with known-broken expectations. Data, not weakened assertions:
/// `corpus_expectations` skips these; `expected_failures` documents them.
const EXPECTED_FAILURE_LINES: &[u64] = &[];

fn check_entry(e: &Entry) -> Option<String> {
    let got_clusters = grapheme_count(&e.text) as u64;
    let got_width: u64 = GraphemeIterator::new(&e.text)
        .map(layout_grapheme_width)
        .sum::<usize>() as u64;
    if got_clusters == e.clusters && got_width == e.width {
        None
    } else {
        Some(format!(
            "line {} [{}]: editor gives {} clusters, {} cells; canonical expects {} clusters, {} cells",
            e.line, e.section, got_clusters, got_width, e.clusters, e.width
        ))
    }
}

#[test]
fn corpus_sidecar_matches_corpus_file() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let corpus = std::fs::read_to_string(root.join("demos/UTF-8-demo-lig.txt"))
        .expect("corpus file is readable");
    let lines: Vec<&str> = corpus.lines().collect();
    for e in load_entries() {
        let idx = (e.line - 1) as usize;
        let actual = lines.get(idx).unwrap_or_else(|| panic!("line {} missing", e.line));
        assert_eq!(
            *actual,
            "» ".to_owned() + &e.text,
            "sidecar text diverges from corpus line {}",
            e.line
        );
    }
}

#[test]
fn corpus_expectations() {
    let failures: Vec<String> = load_entries()
        .iter()
        .filter(|e| !EXPECTED_FAILURE_LINES.contains(&e.line))
        .filter_map(check_entry)
        .collect();
    assert!(
        failures.is_empty(),
        "canonical expectation mismatches ({}):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
#[ignore]
fn expected_failures() {
    let entries = load_entries();
    let mut unlisted: Vec<u64> = Vec::new();
    let mut now_passing: Vec<u64> = Vec::new();
    for line in EXPECTED_FAILURE_LINES {
        match entries.iter().find(|e| e.line == *line) {
            None => unlisted.push(*line),
            Some(e) => {
                if check_entry(e).is_none() {
                    now_passing.push(*line);
                }
            }
        }
    }
    assert!(
        unlisted.is_empty() && now_passing.is_empty(),
        "expected-failure list is stale: unlisted lines {:?}, now passing {:?}",
        unlisted,
        now_passing
    );
    assert!(
        !EXPECTED_FAILURE_LINES.is_empty(),
        "expected_failures runs only when EXPECTED_FAILURE_LINES is non-empty"
    );
}
