//! Large-file DS probe for the owner-directed large-file program.
//!
//! Usage: `cargo run --release --example large_probe -- <fixture> [<fixture>...]`
//! Prints one `RESULT` line per fixture with timings in milliseconds and
//! RSS (VmRSS KiB) after each phase. Run 5x via a script for medians+spread.
//!
//! Measurements per fixture:
//!   open_ms        — `Buffer::open_mapped` (fstat-only O(1) expected)
//!   rss_open_kib   — VmRSS right after open (page-cache budget: 16 MiB)
//!   randline_ms    — 2000 random `line()` accesses (deep in the file)
//!   seqline_ms     — 100k sequential `line()` accesses from the middle
//!   rss_lines_kib  — VmRSS after line access
//!   edit_ms        — insert+delete of 100 bytes at start/middle/end (6 ops)
//!   findall_ms     — whole-file `find_all` for a common pattern
//!   hits           — number of find_all hits (sanity)
//!   save_ms        — `write_to` to a temp file + byte-exactness check
//!   save_ok        — 1 if the round-trip was byte-exact
//!   rss_end_kib    — VmRSS at the end
//!
//! Fixtures: see benches/gen_large_fixtures.py (seed 245661).

use erax::core::buffer::Buffer;
use std::fs;
use std::io::Read;
use std::time::Instant;

fn rss_kib() -> u64 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0)
}

// Deterministic xorshift (same family as the in-tree fuzzers).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn fnv1a64(path: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut f = fs::File::open(path).expect("open for hash");
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).expect("read for hash");
        if n == 0 {
            break;
        }
        for &b in &buf[..n] {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

fn probe(path: &str) {
    let mut rng = Rng(0x51ab3f7d);
    let t = Instant::now();
    let mut buf = Buffer::open_mapped(path).expect("open_mapped");
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;
    let rss_open_kib = rss_kib();

    let n_lines = buf.line_count();
    let len = buf.len();

    // Random line access, deep in the file.
    let t = Instant::now();
    let mut sink = 0usize;
    for _ in 0..2000 {
        let li = rng.below(n_lines);
        if let Some(line) = buf.line(li) {
            sink += line.len();
        }
    }
    let randline_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Sequential line access from the middle.
    let t = Instant::now();
    let start_line = n_lines / 2;
    let seq_n = 100_000.min(n_lines.saturating_sub(start_line));
    for i in 0..seq_n {
        if let Some(line) = buf.line(start_line + i) {
            sink += line.len();
        }
    }
    let seqline_ms = t.elapsed().as_secs_f64() * 1000.0;
    let rss_lines_kib = rss_kib();

    // Edits at start / middle / end (insert 100 bytes, then delete them).
    let t = Instant::now();
    let chunk = "x".repeat(100);
    for &pos in &[0usize, len / 2, len] {
        buf.insert(pos, &chunk);
        let at = pos.min(buf.len().saturating_sub(chunk.len()));
        buf.delete(at, chunk.len().min(buf.len().saturating_sub(at)));
    }
    let edit_ms = t.elapsed().as_secs_f64() * 1000.0;
    // Edits must net to zero: content identical to the pristine mapped file.
    let pristine_hash = fnv1a64(path);
    // (hash of buffer content via bytes())
    let mut h: u64 = 0xcbf29ce484222325;
    for b in buf.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    let edits_net_zero = h == pristine_hash;

    // Whole-file find_all.
    let t = Instant::now();
    let hits = buf.find_all("function_").len();
    let findall_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Save round-trip byte-exactness.
    let t = Instant::now();
    let save_path = format!("{path}.probe_save_tmp");
    buf.save_as(&save_path).expect("save_as");
    let save_ms = t.elapsed().as_secs_f64() * 1000.0;
    let save_ok = fnv1a64(&save_path) == pristine_hash;
    let _ = fs::remove_file(&save_path);
    let rss_end_kib = rss_kib();

    // Keep the optimizer honest.
    std::hint::black_box(sink);

    println!(
        "RESULT fixture={path} lines={n_lines} bytes={len} \
         open_ms={open_ms:.2} rss_open_kib={rss_open_kib} \
         randline_ms={randline_ms:.2} seqline_ms={seqline_ms:.2} seq_n={seq_n} \
         rss_lines_kib={rss_lines_kib} edit_ms={edit_ms:.2} edits_net_zero={edits_net_zero} \
         findall_ms={findall_ms:.2} hits={hits} \
         save_ms={save_ms:.2} save_ok={save_ok} rss_end_kib={rss_end_kib}"
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: large_probe <fixture> [<fixture>...]");
        std::process::exit(2);
    }
    for a in &args {
        probe(a);
    }
}
