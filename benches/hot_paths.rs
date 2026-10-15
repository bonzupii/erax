//! Benchmark harness for erax hot paths.
//!
//! Choice of harness: `criterion` (dev-dependency only — it never ships in the
//! editor binary). It gives statistically rigorous timing with warmup and
//! outlier rejection, which a hand-rolled `Instant` loop cannot. This is the
//! standard Rust benchmarking tool; writing our own statistics would duplicate
//! it for no gain.
//!
//! Run: `cargo bench` (or `cargo bench -- --save-baseline before`).
//! Methodology notes are kept in ~/workspace/erax-work/BENCHMARKS.md.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;
use erax::core::buffer::Buffer;
use erax::core::completion::WordCompleter;
use erax::core::spell::SpellChecker;
use erax::sed::diff::DiffView;
use erax::sed::SedConfig;
use erax::terminal::color::Color;
use std::io::{BufReader, Cursor};

/// Build a synthetic source file with `lines` lines of Rust-like text.
fn large_text(lines: usize) -> String {
    let mut text = String::new();
    for i in 0..lines {
        text.push_str(&format!(
            "fn function_{i}(arg: usize) -> usize {{ let result = arg * {i} + compute(i); result }}\n"
        ));
    }
    text
}

fn large_buffer(lines: usize) -> Buffer {
    Buffer::from_string(large_text(lines))
}

fn bench_config_startup(c: &mut Criterion) {
    // Config is now compile-time consts (N-270); "startup" is accessing them.
    c.bench_function("startup/configure", |b| {
        b.iter(|| {
            let theme = erax::config::theme::THEME.theme();
            let bindings = erax::config::keys::KEYBINDINGS.len();
            black_box((theme, bindings));
        });
    });
}

fn bench_line_access(c: &mut Criterion) {
    let buf = large_buffer(100_000);
    let n = buf.line_count();
    c.bench_function("buffer/line_access_100k", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for i in (0..n).step_by(7) {
                if let Some(line) = buf.line(black_box(i)) {
                    total += line.len();
                }
            }
            black_box(total);
        });
    });
}

fn bench_edit_ops(c: &mut Criterion) {
    let text = large_text(100_000);
    let probe = Buffer::from_string(text.clone());
    let mid = probe.len() / 2;
    c.bench_function("buffer/edit_insert_delete_100k", |b| {
        b.iter_batched(
            || Buffer::from_string(text.clone()),
            |mut b2| {
                for i in 0..1000 {
                    b2.insert(black_box(mid + i), "x");
                }
                for _ in 0..1000 {
                    b2.delete(black_box(mid), 1);
                }
                black_box(b2.len());
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_search(c: &mut Criterion) {
    let buf = large_buffer(100_000);
    c.bench_function("buffer/find_all_100k", |b| {
        b.iter(|| {
            let hits = buf.find_all(black_box("function_"));
            black_box(hits.len());
        });
    });
}

fn bench_completion(c: &mut Criterion) {
    let buf = large_buffer(10_000);
    let mut completer = WordCompleter::new();
    let id = erax::core::id::BufferId(0);
    let pos = buf.len() / 2;
    c.bench_function("completion/complete_at_cursor_10k", |b| {
        b.iter(|| {
            completer.invalidate_cache();
            let words = completer.complete_at_cursor(&buf, id, black_box(pos));
            black_box(words.len());
        });
    });
}

fn bench_spell(c: &mut Criterion) {
    let checker = SpellChecker::new();
    let tokens = ["function", "teh", "recieve", "variable", "seperate"];
    c.bench_function("spell/check_words", |b| {
        b.iter(|| {
            let mut bad = 0;
            for w in tokens {
                if !checker.check(black_box(w)) {
                    bad += 1;
                }
            }
            black_box(bad);
        });
    });
}

fn bench_sed(c: &mut Criterion) {
    let input: String = (0..10_000)
        .map(|i| format!("line {i} foo bar\n"))
        .collect();
    c.bench_function("sed/transform_10k_lines", |b| {
        b.iter(|| {
            let mut cfg = SedConfig::new();
            cfg.add_script("s/foo/baz/g").unwrap();
            let reader = BufReader::new(Cursor::new(input.clone()));
            let mut out = Vec::new();
            cfg.execute(reader, &mut out).unwrap();
            black_box(out.len());
        });
    });
}

fn bench_diff_hunks(c: &mut Criterion) {
    let original: String = (0..10_000).map(|i| format!("line {i}\n")).collect();
    let modified: String = (0..10_000)
        .map(|i| {
            if i % 100 == 0 {
                format!("CHANGED {i}\n")
            } else {
                format!("line {i}\n")
            }
        })
        .collect();
    c.bench_function("sed/diff_hunks_10k", |b| {
        b.iter(|| {
            let view = DiffView::new(original.clone(), modified.clone());
            let hunks = view.compute_hunks();
            black_box(hunks.len());
        });
    });
}

fn bench_color_ansi(c: &mut Criterion) {
    // Simulates the per-cell ANSI conversion cost in the render path (N-104/N-106).
    let colors = [
        Color::Red,
        Color::Green,
        Color::Rgb { r: 10, g: 20, b: 30 },
        Color::Rgb { r: 200, g: 100, b: 50 },
    ];
    let mut group = c.benchmark_group("render/ansi_color_codes");
    group.throughput(Throughput::Elements(10_000));
    group.bench_function("per_cell_fg_bg_10k", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for _ in 0..2_500 {
                for col in colors {
                    // The converted value must be *consumed* observably or
                    // opt-levels 2/3 dead-code-eliminate the conversion (the
                    // earlier bench measured 531ps of loop overhead — vacuous).
                    // The real render path likewise consumes the converted
                    // color rather than discarding it (N-106).
                    let converted = col.to_crossterm();
                    total = total.wrapping_add(color_checksum(black_box(converted)) as usize);
                }
            }
            black_box(total);
        });
    });
    group.finish();
}

/// Observable consumer for the converted crossterm color in `bench_color_ansi`.
/// Cheap (one match, like the render path's value comparison), but keeps the
/// conversion alive under every opt level.
fn color_checksum(c: crossterm::style::Color) -> u64 {
    use crossterm::style::Color as C;
    match c {
        C::Reset => 0,
        C::Black => 1,
        C::DarkRed => 2,
        C::Red => 3,
        C::DarkGreen => 4,
        C::Green => 5,
        C::DarkYellow => 6,
        C::Yellow => 7,
        C::DarkBlue => 8,
        C::Blue => 9,
        C::DarkMagenta => 10,
        C::Magenta => 11,
        C::DarkCyan => 12,
        C::Cyan => 13,
        C::Grey => 14,
        C::DarkGrey => 15,
        C::White => 16,
        C::Rgb { r, g, b } => (r as u64) | ((g as u64) << 8) | ((b as u64) << 16) | (1 << 24),
        C::AnsiValue(v) => (v as u64) | (1 << 25),
    }
}

fn bench_key_sequence(c: &mut Criterion) {
    // Keybinding lookup cost per keypress (typed trie).
    let _config = ();
    use erax::core::input::{Key, KeyPress};
    use erax::terminal::keybinds::KeyBindingManager;
    let mut manager = KeyBindingManager::new();
    manager.load(erax::config::keys::KEYBINDINGS);
    let keys = [
        KeyPress::ctrl(Key::Char('f')),
        KeyPress::ctrl(Key::Char('b')),
        KeyPress::ctrl(Key::Char('n')),
        KeyPress::ctrl(Key::Char('x')),
        KeyPress::ctrl(Key::Char('s')),
        KeyPress::ctrl(Key::Char('r')),
        KeyPress::plain(Key::Esc),
        KeyPress::plain(Key::Char('x')),
    ];
    c.bench_function("input/keybind_lookup", |b| {
        b.iter(|| {
            for k in &keys {
                black_box(manager.process_key(black_box(*k)));
            }
        });
    });
}

criterion_group!(
    benches,
    bench_config_startup,
    bench_line_access,
    bench_edit_ops,
    bench_search,
    bench_completion,
    bench_spell,
    bench_sed,
    bench_diff_hunks,
    bench_color_ansi,
    bench_key_sequence,
);
criterion_main!(benches);
