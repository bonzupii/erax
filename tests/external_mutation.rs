//! Adversarial integration tests for external file mutation.
//! open via [`Buffer::open_mapped`].
//!
//! Defined snapshot semantics under test (the authority for expectations):
//!  1. `open_mapped` pins the file: content = the inode + byte length
//!     observed at open (fstat). Later APPENDS are not visible.
//!  2. The open fd pins the INODE: a rename-over does not change reads.
//!  3. Shrinks/truncation: reads of regions that no longer exist surface as
//!     typed `PagerError::Truncated` — never a panic, never OOB, never
//!     silent garbage.
//!  4. `save_as`/`save` write the BUFFER's content to the PATH byte-exact;
//!     they must never corrupt an unrelated file.
//!
//! These tests do NOT fix anything; they probe. A failure is classified in
//! the accompanying report as (i) implementation bug, (ii) wrong test
//! expectation, or (iii) test-authoring bug.

use erax::core::btext::PagerError;
use erax::core::buffer::{Buffer, LoadError};
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Unique temp path per test: `erax-extmut-<pid>-<tag>[-<role>]`.
fn tmp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("erax-extmut-{}-{}", std::process::id(), tag))
}

/// Best-effort cleanup guard. Removes the files on drop (also on panic).
struct TempGuard {
    paths: Vec<PathBuf>,
}

impl TempGuard {
    fn new(paths: Vec<PathBuf>) -> Self {
        Self { paths }
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        for p in &self.paths {
            let _ = fs::remove_file(p);
        }
    }
}

fn write_file(path: &PathBuf, bytes: &[u8]) {
    fs::write(path, bytes).unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
}

/// Deterministic xorshift64* — no new dependencies.
struct XorShift(u64);

impl XorShift {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn fill_bytes(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            let r = self.next().to_le_bytes();
            let n = (buf.len() - i).min(8);
            buf[i..i + n].copy_from_slice(&r[..n]);
            i += n;
        }
    }
}

/// Build a deterministic line-oriented payload of ~`target_len` bytes.
/// Every 512th line is a `MARKER-` line for `find_all` probing.
fn line_payload(target_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(target_len + 64);
    let mut i = 0usize;
    while out.len() < target_len {
        if i.is_multiple_of(512) {
            out.extend_from_slice(format!("MARKER-{i:07}\n").as_bytes());
        } else {
            out.extend_from_slice(format!("line {i:07} abcdefghijklmnopqrstuvwxyz\n").as_bytes());
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// (a) Rapid appends to a large file while open mapped
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// Backend parameterization (session 7): every test in this suite runs
// against BOTH the pager (default) and the mmap backend (Unix only).
// The backends share the snapshot contract with NO divergence: a
// same-length content replacement under the open file is refused with
// `FileChanged` by both — the pager revalidates the snapshot on every
// read (cache hit, cache miss, and post-copy), exactly like mmap's
// post-read check. See `extmut_regrow_cached_pages_refused_impl`.
// ---------------------------------------------------------------------------

/// Which storage backend `open_mapped` uses for a test.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Backend {
    Pager,
    #[cfg(unix)]
    Mmap,
}

impl Backend {
    fn open(&self, path: &PathBuf) -> Result<Buffer, LoadError> {
        match self {
            // Note: `Buffer::open_mapped` is the mmap backend on Unix since
            // the session-7 adoption decision, so the pager variant must be
            // explicit.
            Backend::Pager => Buffer::open_mapped_pager(path),
            #[cfg(unix)]
            Backend::Mmap => Buffer::open_mapped_mmap(path),
        }
    }

    fn tag(&self) -> &'static str {
        match self {
            Backend::Pager => "pager",
            #[cfg(unix)]
            Backend::Mmap => "mmap",
        }
    }
}

fn extmut_append_storm_snapshot_pinned_impl(backend: Backend) {
    let path = tmp_path(&format!("{}-append-storm", backend.tag()));
    let _guard = TempGuard::new(vec![path.clone()]);

    let expected = line_payload(5 * 1024 * 1024);
    write_file(&path, &expected);
    let initial_disk_len = expected.len() as u64;

    let buf = backend.open(&path).expect("open_mapped failed");

    // Baseline, captured before any mutation.
    let expected_line_count = buf.line_count();
    let expected_marks = buf.find_all("MARKER-");
    assert_eq!(buf.bytes(), expected, "pre-mutation bytes() mismatch");
    assert!(!expected_marks.is_empty(), "test payload must contain markers");
    assert!(!buf.has_pager_error(), "no pager error expected before mutation");

    let mut appender = OpenOptions::new().append(true).open(&path).expect("open appender");

    // 1000 appends, interleaved with reads. Appends are OUTSIDE the pinned
    // range (semantic 1): they must be invisible and must not error.
    for i in 0..1000u32 {
        let chunk = format!("APPENDED-CHUNK-{i:06}-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n");
        appender
            .write_all(chunk.as_bytes())
            .expect("append write failed");

        // Interleaved reads on every iteration.
        assert_eq!(
            buf.bytes(),
            expected,
            "bytes() changed after {i} appends: snapshot not pinned"
        );
        assert_eq!(
            buf.line_count(),
            expected_line_count,
            "line_count() changed after {i} appends"
        );
        assert_eq!(
            buf.find_all("MARKER-"),
            expected_marks,
            "find_all() changed after {i} appends"
        );
        assert!(
            !buf.has_pager_error(),
            "appends outside the pinned range must not record a pager error (iteration {i})"
        );
    }
    appender.flush().unwrap();
    drop(appender);

    // Sanity: the appends really happened on disk.
    let grown = fs::metadata(&path).unwrap().len();
    assert!(
        grown > initial_disk_len,
        "appends did not grow the file: {grown} <= {initial_disk_len}"
    );
    // And the buffer still sees exactly the pinned snapshot.
    assert_eq!(buf.bytes(), expected);
    assert_eq!(buf.line_count(), expected_line_count);
    assert!(!buf.has_pager_error());
    eprintln!("append storm: 1000 appends, file grew {initial_disk_len} -> {grown} bytes, buffer pinned");
}

// ---------------------------------------------------------------------------
// (b) Rapid shrink mid-file while open
// ---------------------------------------------------------------------------

fn extmut_shrink_midfile_surfaces_truncated_impl(backend: Backend) {
    let path = tmp_path(&format!("{}-shrink-midfile", backend.tag()));
    let _guard = TempGuard::new(vec![path.clone()]);

    // 3 MiB, deterministic 17-byte records ("0123456789abcdef\n").
    let record = b"0123456789abcdef\n";
    let mut expected = Vec::with_capacity(3 * 1024 * 1024);
    while expected.len() < 3 * 1024 * 1024 {
        expected.extend_from_slice(record);
    }
    write_file(&path, &expected);

    let buf = backend.open(&path).expect("open_mapped failed");

    // IMPORTANT: no reads before truncation — pages must be uncached so the
    // reads below actually hit the shrunken file.
    //
    // Truncate to exactly 1 MiB (a 64 KiB page boundary): every 64 KiB page
    // is then either fully present or fully absent.
    const KEEP: u64 = 1024 * 1024;
    OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open for truncate")
        .set_len(KEEP)
        .expect("truncate failed");

    // No panic / OOB; the readable prefix must be intact byte-exact.
    let got = buf.bytes();
    assert!(
        !got.is_empty() && got.len() as u64 <= KEEP,
        "bytes() after shrink: expected a non-empty prefix of at most {KEEP} bytes, got {}",
        got.len()
    );
    assert_eq!(
        &got[..],
        &expected[..got.len()],
        "readable prefix corrupted by shrink: not a prefix of the pinned content"
    );

    // The truncated region must surface as a TYPED Truncated error.
    match buf.take_pager_error() {
        Some(PagerError::Truncated { offset, wanted }) => {
            assert_eq!(
                offset, KEEP,
                "first failing page should be the one at the truncation point"
            );
            assert!(wanted > 0);
            eprintln!("shrink: got expected Truncated {{ offset: {offset}, wanted: {wanted} }}");
        }
        other => panic!("expected PagerError::Truncated after shrink, got {other:?}"),
    }

    // Degraded-but-defined line access: line_count() records the error and
    // returns the documented safe default (0); line(0) in the readable
    // prefix still works; a line in the truncated region is None.
    assert_eq!(buf.line_count(), 0, "line_count must degrade to 0 on pager failure");
    assert!(buf.has_pager_error(), "line_count failure must be recorded");
    let _ = buf.take_pager_error();

    assert_eq!(
        buf.line(0).as_deref(),
        Some("0123456789abcdef"),
        "line(0) in the readable prefix must stay intact"
    );
    assert!(
        buf.line(200_000).is_none(),
        "line() in the truncated region must be None, not garbage/panic"
    );
    assert!(buf.has_pager_error(), "truncated line() must record the error");
    match buf.take_pager_error() {
        Some(PagerError::Truncated { .. }) => {}
        other => panic!("expected Truncated from line() in truncated region, got {other:?}"),
    }

    // Buffer length stays pinned at the open-time length (semantic 1).
    assert_eq!(buf.len(), expected.len(), "buf.len() must stay pinned at open length");
}

// ---------------------------------------------------------------------------
// (c) Truncation to zero, then regrowth with different content
// ---------------------------------------------------------------------------

fn extmut_truncate_zero_then_regrow_impl(backend: Backend) {
    let path = tmp_path(&format!("{}-truncate-regrow", backend.tag()));
    let _guard = TempGuard::new(vec![path.clone()]);

    const LEN: usize = 1024 * 1024; // 1 MiB
    // Same length, every byte different: isolates a pure byte-swap.
    let old_content: Vec<u8> = std::iter::repeat_n(b'O', LEN).collect();
    let new_content: Vec<u8> = std::iter::repeat_n(b'N', LEN).collect();
    write_file(&path, &old_content);

    let buf = backend.open(&path).expect("open_mapped failed");

    // --- Phase 1: truncate to zero (no pre-read: pages uncached). ---
    OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open for truncate")
        .set_len(0)
        .expect("truncate to zero failed");

    let got = buf.bytes();
    assert!(
        got.is_empty(),
        "bytes() on a zero-truncated file must return empty, got {} bytes",
        got.len()
    );
    match buf.take_pager_error() {
        Some(PagerError::Truncated { offset, .. }) => {
            assert_eq!(offset, 0, "first failing read should be at offset 0");
        }
        other => panic!("expected Truncated after zero-truncate, got {other:?}"),
    }
    assert_eq!(buf.line_count(), 0);
    let _ = buf.take_pager_error(); // drain the line_count error

    // --- Phase 2: regrow with DIFFERENT content, same length. ---
    write_file(&path, &new_content);

    // Semantic under test: the buffer is a pinned snapshot of the file AS IT
    // WAS AT OPEN. It must NOT silently serve the regrown bytes as if they
    // were the old content. Acceptable outcomes: still show the old bytes
    // (pinned snapshot), or a typed pager error. Silent substitution of the
    // new bytes with no error is a snapshot violation.
    let after = buf.bytes();
    let err = buf.take_pager_error();
    let silent_substitution = after == new_content && err.is_none();
    assert!(
        !silent_substitution,
        "SNAPSHOT VIOLATION: after truncate+regrow, bytes() silently returned the NEW \
         file content as the pinned snapshot (no pager error). len()={} pinned len={LEN}",
        buf.len()
    );
    eprintln!(
        "regrow: after={} bytes (old={}), pager error: {err:?}",
        after.len(),
        after == old_content
    );

    // Pinned length never moves (semantic 1).
    assert_eq!(buf.len(), LEN, "buf.len() must stay pinned");

    // Explicit re-open sees the new content.
    let fresh = backend.open(&path).expect("re-open failed");
    assert_eq!(fresh.bytes(), new_content, "re-open_mapped must see regrown content");
    assert!(!fresh.has_pager_error());
}

fn extmut_regrow_cached_pages_refused_impl(backend: Backend) {
    // Companion to extmut_truncate_zero_then_regrow: pages that WERE read
    // (resident in the pager cache, or already faulted in the mapping)
    // before the mutation must NOT be served as current after
    // truncate+regrow at the same length. Both backends refuse with
    // `FileChanged` once revalidation observes the replacement; if the
    // replacement is not observed (mtime granularity), the served bytes
    // must still be the pinned old ones. Silent substitution of the new
    // bytes is a violation for either backend.
    let path = tmp_path(&format!("{}-regrow-cached", backend.tag()));
    let _guard = TempGuard::new(vec![path.clone()]);

    const LEN: usize = 256 * 1024;
    let old_content: Vec<u8> = (0..LEN).map(|i| b'a' + (i % 26) as u8).collect();
    let new_content: Vec<u8> = (0..LEN).map(|i| b'Z' - (i % 26) as u8).collect();
    write_file(&path, &old_content);

    let buf = backend.open(&path).expect("open_mapped failed");
    // Warm the cache: read everything once while the file is intact.
    assert_eq!(buf.bytes(), old_content);
    assert!(!buf.has_pager_error());

    // Truncate to zero, then regrow with different content.
    OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open for truncate")
        .set_len(0)
        .expect("truncate failed");
    write_file(&path, &new_content);

    let after = buf.bytes();
    let err = buf.take_pager_error();
    eprintln!(
        "[{}] regrow-cached: {} bytes, old_match={}, err={err:?}",
        backend.tag(),
        after.len(),
        after == old_content,
    );
    assert!(
        after != new_content || err.is_some(),
        "SNAPSHOT VIOLATION [{}]: silently served the NEW bytes as the pinned snapshot",
        backend.tag(),
    );
    match err {
        // Detected replacement: the honest refusal (both backends).
        Some(PagerError::FileChanged { .. }) => {}
        // Not observed (mtime granularity): the already-resident bytes
        // genuinely are the old ones — still the pinned snapshot.
        None => assert_eq!(
            after, old_content,
            "[{}] without a FileChanged error, must serve the pinned old bytes",
            backend.tag(),
        ),
        Some(other) => panic!(
            "[{}] expected FileChanged or pinned bytes, got {other:?}",
            backend.tag()
        ),
    }
    assert_eq!(buf.len(), LEN);
}

// ---------------------------------------------------------------------------
// (d) Rename-over (inode swap) while open
// ---------------------------------------------------------------------------

fn extmut_rename_over_inode_swap_impl(backend: Backend) {
    let path = tmp_path(&format!("{}-rename-over", backend.tag()));
    let new_file = tmp_path(&format!("{}-rename-over-new", backend.tag()));
    let _guard = TempGuard::new(vec![path.clone(), new_file.clone()]);

    let old_content = line_payload(100 * 1024);
    let new_content = b"COMPLETELY DIFFERENT INODE CONTENT\n".repeat(4096);
    assert_ne!(old_content, new_content);

    write_file(&path, &old_content);
    let buf_open = backend.open(&path).expect("open_mapped failed");
    let mut buf = buf_open;
    assert_eq!(buf.bytes(), old_content, "pre-rename baseline");

    // Atomically swap the inode at `path`.
    write_file(&new_file, &new_content);
    fs::rename(&new_file, &path).expect("rename-over failed");
    assert_eq!(
        fs::read(&path).unwrap(),
        new_content,
        "rename-over did not install the new inode"
    );

    // Semantic 2: the open fd pins the OLD inode — reads are unaffected.
    assert_eq!(
        buf.bytes(),
        old_content,
        "buffer must keep reading the OLD inode after rename-over"
    );
    assert!(!buf.has_pager_error(), "no pager error expected after rename-over");

    // save_as writes the BUFFER's content to the PATH, byte-exact.
    // Documented behavior: the file now at `path` (the NEW inode installed
    // by the rename) is atomically REPLACED by the buffer's content via a
    // temp-file + rename. No other file is touched; the old inode (still
    // referenced by the buffer's fd) is unaffected.
    buf.save_as(&path).expect("save_as after rename-over failed");
    assert_eq!(
        fs::read(&path).unwrap(),
        old_content,
        "save_as must write the buffer content byte-exact to the path"
    );
    assert!(!buf.has_pager_error());

    // A fresh open now sees the saved (old) content at the path.
    let fresh = backend.open(&path).expect("re-open failed");
    assert_eq!(fresh.bytes(), old_content);
    eprintln!("rename-over: buffer read old inode throughout; save_as replaced path content with buffer bytes");
}

// ---------------------------------------------------------------------------
// (e) Write bursts interleaved with editor ops (threaded storm)
// ---------------------------------------------------------------------------

fn extmut_write_burst_storm_with_editor_ops_impl(backend: Backend) {
    let path = tmp_path(&format!("{}-burst-storm", backend.tag()));
    let dst_mid = tmp_path(&format!("{}-burst-storm-mid", backend.tag()));
    let dst_final = tmp_path(&format!("{}-burst-storm-final", backend.tag()));
    let _guard = TempGuard::new(vec![path.clone(), dst_mid.clone(), dst_final.clone()]);

    let initial = line_payload(4 * 1024 * 1024);
    let initial_len = initial.len() as u64;
    write_file(&path, &initial);

    let mut buf = backend.open(&path).expect("open_mapped failed");
    assert_eq!(buf.bytes(), initial);

    // Writer thread: seeded rounds (4 ops per round) of random overwrites
    // / appends / truncates directly on the file while the main thread
    // edits+reads. The writer runs until the main thread signals completion
    // — overlap is structural, not timed: under parallel-test load the main
    // thread's 200 iterations can take far longer than a fixed round count,
    // and a writer that finishes early leaves the later cold probes racing
    // a static file (the observed flake: zero truncation errors observed).
    // Truncations shrink by at most 64 KiB (one page) and appends add
    // 64 KiB, so the file size random-walks stably around 4 MiB instead of
    // collapsing — the cold-cache probes' 64-page reads then reliably race
    // a truncation mid-read. A deterministic severe truncate every 64 rounds
    // guarantees the storm produces truncations regardless of RNG luck
    // (regrowth is exercised too).
    let writer_done = Arc::new(AtomicBool::new(false));
    let writer_done_w = writer_done.clone();
    let main_done = Arc::new(AtomicBool::new(false));
    let main_done_w = main_done.clone();
    let wpath = path.clone();
    let writer = thread::spawn(move || {
        let mut rng = XorShift::new(0x1234_5678_9abc_def0);
        let mut cur_len = initial_len;
        let mut f = OpenOptions::new().write(true).open(&wpath).expect("writer open");
        let mut rounds = 0u32;
        while !main_done_w.load(Ordering::SeqCst) && rounds < 20_000 {
            for _ in 0..4 {
                if cur_len == 0 {
                    // Only appends are possible on an empty file.
                    let mut data = vec![0u8; 65536];
                    rng.fill_bytes(&mut data);
                    f.seek(SeekFrom::End(0)).unwrap();
                    f.write_all(&data).unwrap();
                    cur_len += 65536;
                } else {
                    match rng.next() % 4 {
                        0 => {
                            // Random overwrite inside the current file.
                            let off = rng.next() % cur_len;
                            let max = (cur_len - off).min(4096);
                            let n = (rng.next() % max + 1) as usize;
                            let mut data = vec![0u8; n];
                            rng.fill_bytes(&mut data);
                            f.seek(SeekFrom::Start(off)).unwrap();
                            f.write_all(&data).unwrap();
                        }
                        1 => {
                            // Append 64 KiB (balances the truncations below).
                            let mut data = vec![0u8; 65536];
                            rng.fill_bytes(&mut data);
                            f.seek(SeekFrom::End(0)).unwrap();
                            f.write_all(&data).unwrap();
                            cur_len += 65536;
                        }
                        _ => {
                            // Truncate shorter: usually by at most one page,
                            // occasionally severe (regrowth path).
                            let n = if rng.next().is_multiple_of(20) {
                                rng.next() % (cur_len + 1)
                            } else {
                                let cut = rng.next() % 65536;
                                cur_len.saturating_sub(cut).max(65536)
                            };
                            f.set_len(n).unwrap();
                            cur_len = n;
                        }
                    }
                }
            }
            if rounds % 64 == 63 {
                // Deterministic severe truncate: the storm always contains
                // truncations, not only when the RNG picks them.
                let n = rng.next() % (cur_len + 1);
                f.set_len(n).unwrap();
                cur_len = n;
            }
            rounds += 1;
            thread::sleep(std::time::Duration::from_millis(1));
        }
        f.sync_all().unwrap();
        writer_done_w.store(true, Ordering::SeqCst);
    });

    // Main thread: reads + edits + undos + save_as to a SEPARATE path, plus
    // a cold-cache probe: a FRESH open_mapped every few iterations. The
    // editor buffer's pager cache (16 MiB) would otherwise absorb the whole
    // 2 MiB file on the first read and never observe the writer's
    // truncations; the probe's cold pager hits the live disk every time, so
    // the truncation paths are genuinely exercised.
    let mut rng = XorShift::new(0xdead_beef_cafe_1234);
    let mut pager_errors = 0u32;
    let mut probe_errors = 0u32;
    let mut probes = 0u32;
    for i in 0..200 {
        // Reads on the editor buffer.
        let _ = buf.bytes();
        let _ = buf.line_count();
        let _ = buf.find_all("MARKER-");
        let lc = buf.line_count();
        if lc > 0 {
            let _ = buf.line((rng.next() as usize) % lc);
        }
        let _ = buf.line_to_byte((rng.next() as usize) % lc.max(1));

        // Cold-cache probe: fresh open, full read, then drop. Must never
        // panic; any anomaly must be a typed PagerError. Done every
        // iteration: with ~600 writer ops racing 200 probes, mid-read
        // truncations are reliably observed (not a timing fluke).
        {
            probes += 1;
            match backend.open(&path) {
                Ok(probe) => {
                    let pinned = probe.len();
                    let pbytes = probe.bytes();
                    assert!(
                        pbytes.len() <= pinned,
                        "probe bytes() exceeded pinned len: {} > {pinned}",
                        pbytes.len()
                    );
                    while let Some(e) = probe.take_pager_error() {
                        probe_errors += 1;
                        match e {
                            PagerError::Truncated { .. }
                            | PagerError::Io { .. }
                            | PagerError::OutOfRange { .. }
                            | PagerError::FileChanged { .. } => {}
                        }
                    }
                }
                Err(e) => panic!("probe open_mapped failed mid-storm: {e:?}"),
            }
        }

        // Edits at (valid, clamped) positions.
        let blen = buf.len();
        let ipos = (rng.next() as usize) % (blen + 1);
        buf.insert(ipos, "STORM-EDIT");
        let blen2 = buf.len();
        if blen2 > 0 {
            let dpos = (rng.next() as usize) % blen2;
            let dlen = (rng.next() % 64 + 1) as usize;
            buf.delete(dpos, dlen);
        }
        if i % 7 == 0 {
            let _ = buf.undo();
        }
        // save_as to a separate path mid-storm: may abort with a typed
        // error if the pager hits a truncated region; must never panic
        // and never leave a corrupt/partial file behind on failure.
        if i % 20 == 0 {
            match buf.save_as(&dst_mid) {
                Ok(()) => assert!(dst_mid.exists(), "save_as Ok but file missing"),
                Err(e) => {
                    eprintln!("mid-storm save_as aborted (typed): {e}");
                }
            }
        }

        // Drain the typed error channel. Every anomaly must arrive as a
        // PagerError variant — the type system enforces this, but counting
        // proves the storm actually exercised the truncation paths.
        while let Some(e) = buf.take_pager_error() {
            pager_errors += 1;
            match e {
                PagerError::Truncated { .. }
                | PagerError::Io { .. }
                | PagerError::OutOfRange { .. }
                | PagerError::FileChanged { .. } => {}
            }
        }
    }

    // Signal the writer to stop, then join: the storm covered the whole
    // main-thread run by construction.
    main_done.store(true, Ordering::SeqCst);
    writer.join().expect("writer thread panicked");
    assert!(writer_done.load(Ordering::SeqCst));
    eprintln!(
        "burst storm: 200 writer iterations vs 200 editor iterations ({probes} cold probes), \
         {pager_errors} editor-buffer + {probe_errors} probe typed pager errors, no panics"
    );
    assert!(
        probe_errors > 0,
        "storm did not exercise any truncation path: {probes} cold probes saw zero pager errors"
    );

    // Quiesce: the file is static now. Drain any pending error, then the
    // final save invariant: save_as either writes buf.bytes() byte-exact,
    // or aborts with the typed pager error and leaves NO partial file.
    // (save() streams the tree through the pager into a temp file and only
    // renames on success — a pager failure must never produce a partial.)
    while buf.take_pager_error().is_some() {}
    let mem = buf.bytes();
    let bytes_err = buf.take_pager_error();
    let dst_fresh = dst_final.clone();
    assert!(!dst_fresh.exists(), "final dst must not exist before final save");
    match buf.save_as(&dst_fresh) {
        Ok(()) => {
            assert!(
                bytes_err.is_none(),
                "save succeeded but bytes() recorded a pager error — inconsistent: {bytes_err:?}"
            );
            let on_disk = fs::read(&dst_fresh).expect("save_as Ok but file missing");
            assert_eq!(
                on_disk, mem,
                "final save_as output must equal buf.bytes() byte-exact"
            );
            eprintln!("final save_as: Ok, {} bytes, byte-exact match", on_disk.len());
        }
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("failed to write buffer content"),
                "save failure must be the typed pager abort, got: {msg}"
            );
            assert!(
                !dst_fresh.exists(),
                "aborted save must not leave a partial file at {}",
                dst_fresh.display()
            );
            eprintln!("final save_as: typed abort ({msg}); no partial file left");
        }
    }
}

// ---------------------------------------------------------------------------
// (f) bytes() on a file truncated to zero mid-open
// ---------------------------------------------------------------------------

fn extmut_bytes_on_zero_truncated_file_impl(backend: Backend) {
    let path = tmp_path(&format!("{}-bytes-zero-trunc", backend.tag()));
    let _guard = TempGuard::new(vec![path.clone()]);

    let content = line_payload(100 * 1024);
    write_file(&path, &content);

    let buf = backend.open(&path).expect("open_mapped failed");

    // No reads before truncation: pages uncached, so bytes() hits the
    // zero-length file.
    OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open for truncate")
        .set_len(0)
        .expect("truncate to zero failed");

    // Must return empty — not a panic, not garbage.
    let got = buf.bytes();
    assert!(
        got.is_empty(),
        "bytes() on zero-truncated file must be empty, got {} bytes",
        got.len()
    );

    // ... and must record the typed error.
    match buf.take_pager_error() {
        Some(PagerError::Truncated { offset, wanted }) => {
            assert_eq!(offset, 0);
            assert!(wanted > 0);
            eprintln!("zero-truncate: bytes() empty + Truncated {{ offset: 0, wanted: {wanted} }}");
        }
        other => panic!("expected PagerError::Truncated, got {other:?}"),
    }

    // Degraded line access stays defined too.
    assert_eq!(buf.line_count(), 0);
    assert!(buf.has_pager_error());
    assert!(buf.line(0).is_none());
    let _ = buf.take_pager_error();
}

// ---------------------------------------------------------------------------
// Backend wrappers (session 7): each test runs against the pager (default)
// and the mmap backend (Unix only).
// ---------------------------------------------------------------------------

macro_rules! backend_tests {
    ($impl_fn:ident, $pager_name:ident, $mmap_name:ident) => {
        #[test]
        fn $pager_name() {
            $impl_fn(Backend::Pager);
        }

        #[cfg(unix)]
        #[test]
        fn $mmap_name() {
            $impl_fn(Backend::Mmap);
        }
    };
}

backend_tests!(
    extmut_append_storm_snapshot_pinned_impl,
    extmut_append_storm_snapshot_pinned_pager,
    extmut_append_storm_snapshot_pinned_mmap
);
backend_tests!(
    extmut_shrink_midfile_surfaces_truncated_impl,
    extmut_shrink_midfile_surfaces_truncated_pager,
    extmut_shrink_midfile_surfaces_truncated_mmap
);
backend_tests!(
    extmut_truncate_zero_then_regrow_impl,
    extmut_truncate_zero_then_regrow_pager,
    extmut_truncate_zero_then_regrow_mmap
);
backend_tests!(
    extmut_regrow_cached_pages_refused_impl,
    extmut_regrow_cached_pages_refused_pager,
    extmut_regrow_cached_pages_refused_mmap
);
backend_tests!(
    extmut_rename_over_inode_swap_impl,
    extmut_rename_over_inode_swap_pager,
    extmut_rename_over_inode_swap_mmap
);
backend_tests!(
    extmut_write_burst_storm_with_editor_ops_impl,
    extmut_write_burst_storm_with_editor_ops_pager,
    extmut_write_burst_storm_with_editor_ops_mmap
);
backend_tests!(
    extmut_bytes_on_zero_truncated_file_impl,
    extmut_bytes_on_zero_truncated_file_pager,
    extmut_bytes_on_zero_truncated_file_mmap
);
