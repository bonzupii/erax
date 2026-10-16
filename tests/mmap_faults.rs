//! Fault-injection tests for the mmap backend's SIGBUS mitigation stack.
//!
//! Unix-only (the backend is Unix-only) and never under Miri (no real
//! signals there; see `crate::core::mmap::guard`).
//!
//! These tests prove the owner-required guarantee: a truncation (or any
//! external mutation) racing a mapping read becomes a *typed*
//! `PagerError` — never process death. The session-6 prototype SIGBUSed
//! on exactly the write-burst storm below; these tests fail on that
//! prototype and pass on the hardened module.
#![cfg(all(unix, not(miri)))]

use erax::core::btext::{BText, PagerError};
use erax::core::mmap::MmapRegion;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn tmp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("erax-mmapfault-{}-{tag}", std::process::id()))
}

struct TempGuard {
    paths: Vec<PathBuf>,
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
}

/// Fill `path` with `size` deterministic bytes.
fn fill_file(path: &PathBuf, size: usize, seed: u64) {
    let mut rng = XorShift::new(seed);
    let mut f = fs::File::create(path).expect("create fixture");
    let mut remaining = size;
    let mut chunk = vec![0u8; 65536];
    while remaining > 0 {
        let n = remaining.min(chunk.len());
        for b in &mut chunk[..n] {
            *b = (rng.next() & 0xff) as u8;
        }
        // Newline every ~80 bytes so line ops are meaningful.
        for i in (79..n).step_by(80) {
            chunk[i] = b'\n';
        }
        f.write_all(&chunk[..n]).expect("write fixture");
        remaining -= n;
    }
    f.sync_all().expect("sync fixture");
}

fn truncate_to(path: &PathBuf, len: u64) {
    OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for truncate")
        .set_len(len)
        .expect("set_len failed");
}

// ---------------------------------------------------------------------------
// (1) Deterministic truncation-under-map: the TOCTOU window, forced.
// ---------------------------------------------------------------------------

/// Deterministic truncation-under-map: read once (faults in page 0 only),
/// truncate to zero, then read never-touched pages far into the file. The
/// post-read fstat cannot pre-empt the fault — the deref happens first — so
/// these reads MUST take the SIGBUS path. Without the handler this SIGBUSes
/// the test process; with it, we get `Truncated`.
#[test]
fn sigbus_handler_converts_truncation_fault_to_typed_error() {
    let path = tmp_path("det-trunc");
    let _guard = TempGuard { paths: vec![path.clone()] };
    const SIZE: usize = 16 * 1024 * 1024;
    fill_file(&path, SIZE, 0x1);

    let file = fs::File::open(&path).expect("open mapped file");
    let region = MmapRegion::new(file).expect("mmap failed");

    // Baseline: faults in page 0 only.
    let first = region
        .read_guarded(0, 4096, |_, s| s.to_vec())
        .expect("baseline read failed");
    assert_eq!(first.len(), 4096);

    // Truncate to zero.
    truncate_to(&path, 0);

    // Offset 8 MiB was never faulted in, so the deref faults — the handler
    // must convert it to `Truncated`. (Both the copy path and the pure-scan
    // path are exercised: the fault can strike in either.)
    let r = region.read_guarded(8 * 1024 * 1024, 4096, |_, s| s.to_vec());
    match r {
        Err(PagerError::Truncated { offset, wanted }) => {
            assert_eq!(offset, 8 * 1024 * 1024);
            assert_eq!(wanted, 4096);
        }
        other => panic!("expected Truncated from faulted read, got {other:?}"),
    }

    // Scan path (no allocation inside the window): newline counting over a
    // never-faulted page must also come back as `Truncated`, not death.
    // (The fault path refreshed the length cache, so this short-circuits
    // on the cached length — still `Truncated`, still no signal needed.)
    let r2 = region.read_guarded(12 * 1024 * 1024, 65536, |_, s| {
        s.iter().filter(|&&b| b == b'\n').count()
    });
    match r2 {
        Err(PagerError::Truncated { .. }) => {}
        other => panic!("expected Truncated from faulted scan, got {other:?}"),
    }

    // The process survived and the region is still usable: reads now
    // short-circuit on the refreshed length cache (no signal path needed).
    match region.read_guarded(0, 16, |_, s| s.len()) {
        Err(PagerError::Truncated { offset, .. }) => assert_eq!(offset, 0),
        other => panic!("expected Truncated after cache refresh, got {other:?}"),
    }
    eprintln!("deterministic fault injection: faulted read -> typed Truncated, process alive, region reusable");
}

/// The allocation-safe copy primitive under fault injection: a fault
/// mid-copy must yield `Truncated` with the caller's buffer left empty
/// (no stranded allocation, no partial bytes observable).
#[test]
fn read_guarded_into_fault_leaves_caller_buffer_empty() {
    let path = tmp_path("into-fault");
    let _guard = TempGuard { paths: vec![path.clone()] };
    const SIZE: usize = 16 * 1024 * 1024;
    fill_file(&path, SIZE, 0x2);

    let file = fs::File::open(&path).expect("open mapped file");
    let region = MmapRegion::new(file).expect("mmap failed");

    // Baseline copy through the new primitive.
    let mut out = Vec::new();
    region.read_guarded_into(0, 4096, &mut out).expect("baseline copy");
    assert_eq!(out.len(), 4096);

    // Truncate to zero; read at an offset never faulted in.
    truncate_to(&path, 0);
    let mut faulted = vec![0xAAu8; 8];
    match region.read_guarded_into(8 * 1024 * 1024, 4096, &mut faulted) {
        Err(PagerError::Truncated { offset, wanted }) => {
            assert_eq!(offset, 8 * 1024 * 1024);
            assert_eq!(wanted, 4096);
        }
        other => panic!("expected Truncated from faulted copy, got {other:?}"),
    }
    assert!(
        faulted.is_empty(),
        "caller buffer must be empty after a faulted copy (got {} bytes)",
        faulted.len()
    );

    // The region stays usable; the old `to_vec`-in-window shape is gone
    // from production, but the generic primitive still works.
    let again = region.read_guarded(0, 16, |_, s| s.len());
    assert!(
        matches!(again, Err(PagerError::Truncated { .. })),
        "expected Truncated after cache refresh, got {again:?}"
    );
    eprintln!("faulted read_guarded_into -> typed Truncated, caller buffer empty, region reusable");
}

/// Same-length content replacement must become `FileChanged`, never silent
/// substitution.
///
/// Regression test for the hole caught during session-7 hardening:
/// `MAP_PRIVATE` read-faults map the page cache (not copy-on-write), so an
/// in-place rewrite is visible in already-faulted pages WITHOUT faulting —
/// no SIGBUS, no signal path. Only the post-read mtime check catches it.
/// (The first hardening design used a 1024-read epoch cache for mtime and
/// missed this for up to 1023 reads; the external-mutation suite caught it.)
#[test]
fn same_length_replacement_becomes_file_changed() {
    let path = tmp_path("det-replace");
    let _guard = TempGuard { paths: vec![path.clone()] };
    const SIZE: usize = 1024 * 1024;
    let old_content = vec![b'O'; SIZE];
    let new_content = vec![b'N'; SIZE];
    write_file(&path, &old_content);

    let region = MmapRegion::new(fs::File::open(&path).expect("open")).expect("mmap");
    // Fault in every page with the old content.
    let baseline = region
        .read_guarded(0, SIZE, |_, s| s.to_vec())
        .expect("baseline read failed");
    assert_eq!(baseline, old_content);

    // Truncate + regrow at the SAME length with different bytes.
    truncate_to(&path, 0);
    write_file(&path, &new_content);

    // The already-faulted pages now show the NEW bytes (page cache); the
    // post-read mtime check must refuse rather than serve them.
    match region.read_guarded(0, SIZE, |_, s| s.to_vec()) {
        Err(PagerError::FileChanged { offset }) => assert_eq!(offset, 0),
        Ok(got) => panic!(
            "SILENT SUBSTITUTION: served {} new bytes with no error (old_match={})",
            got.len(),
            got == old_content
        ),
        other => panic!("expected FileChanged, got {other:?}"),
    }
    eprintln!("same-length replacement -> FileChanged, no silent substitution");
}

// ---------------------------------------------------------------------------
// (2) Shrink-to-zero under active multi-threaded reads.
// ---------------------------------------------------------------------------

#[test]
fn truncate_to_zero_under_active_reads() {
    let path = tmp_path("zero-storm");
    let _guard = TempGuard { paths: vec![path.clone()] };
    const SIZE: usize = 64 * 1024 * 1024;
    fill_file(&path, SIZE, 0x2);

    let stop = Arc::new(AtomicBool::new(false));
    let errors = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let oks = Arc::new(std::sync::atomic::AtomicU64::new(0));

    // Readers open FIRST, before any mutation: each opens its own mapping
    // (BText is !Sync) and hammers line + byte ops. Every outcome must be
    // Ok or a typed PagerError. A SIGBUS anywhere fails the whole test
    // binary — survival IS the test.
    //
    // (The open-before-mutate order matters: `open_mapped_mmap` on a
    // zero-length file yields an empty BText whose reads trivially return
    // Ok — opening after the writer starts would make the storm vacuous.)
    let ready = Arc::new(std::sync::Barrier::new(5)); // 4 readers + main
    let mut readers = Vec::new();
    for t in 0..4 {
        let rpath = path.clone();
        let rready = ready.clone();
        let rerrors = errors.clone();
        let roks = oks.clone();
        readers.push(thread::spawn(move || {
            let btext = BText::open_mapped_mmap(&rpath).expect("open_mapped_mmap failed");
            assert_eq!(
                btext.len(),
                SIZE,
                "reader opened a truncated file: storm setup is racy"
            );
            rready.wait();
            let mut rng = XorShift::new(0x1000 + t);
            let deadline = Instant::now() + Duration::from_secs(4);
            while Instant::now() < deadline {
                let op = rng.next() % 5;
                let r: Result<(), PagerError> = match op {
                    0 => btext.line_count().map(|_| ()),
                    1 => btext
                        .line_to_byte((rng.next() % 100_000) as usize)
                        .map(|_| ()),
                    2 => {
                        let off = rng.next() % (SIZE as u64);
                        btext.slice_bytes(off as usize..(off as usize + 4096).min(SIZE)).map(|_| ())
                    }
                    3 => btext.collect_bytes().map(|_| ()),
                    _ => {
                        btext.char_at((rng.next() % (SIZE as u64)) as usize);
                        Ok(())
                    }
                };
                match r {
                    Ok(()) => {
                        roks.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(
                        PagerError::Truncated { .. }
                        | PagerError::FileChanged { .. }
                        | PagerError::Io { .. }
                        | PagerError::OutOfRange { .. },
                    ) => {
                        rerrors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
    }

    // All readers have opened the full-size file; NOW start the writer.
    ready.wait();
    let wpath = path.clone();
    let wstop = stop.clone();
    let writer = thread::spawn(move || {
        let mut i = 0u64;
        while !wstop.load(Ordering::Relaxed) {
            if i.is_multiple_of(2) {
                truncate_to(&wpath, 0);
            } else {
                fill_file(&wpath, SIZE, 0x2 + i);
            }
            i += 1;
            thread::sleep(Duration::from_millis(2));
        }
    });

    for r in readers {
        r.join().expect("reader thread panicked or was killed (SIGBUS?)");
    }
    stop.store(true, Ordering::Relaxed);
    writer.join().expect("writer thread panicked");

    let e = errors.load(Ordering::Relaxed);
    let o = oks.load(Ordering::Relaxed);
    eprintln!("zero-storm: {o} ok + {e} typed errors across 4 readers, no panics, process alive");
    assert!(e > 0, "storm never raced a truncation: no typed errors observed");
}

// ---------------------------------------------------------------------------
// (3) Write-burst storm — the session-6 SIGBUS repro, now through the mmap
//     backend. Must pass as typed errors.
// ---------------------------------------------------------------------------

#[test]
fn write_burst_storm_mmap_backend() {
    let path = tmp_path("burst-mmap");
    let _guard = TempGuard { paths: vec![path.clone()] };
    const SIZE: usize = 8 * 1024 * 1024;
    fill_file(&path, SIZE, 0x3);

    // Open BEFORE the writer starts (see the zero-storm test for why the
    // order matters).
    let btext = BText::open_mapped_mmap(&path).expect("open_mapped_mmap failed");
    assert_eq!(btext.len(), SIZE, "opened a truncated file: storm setup is racy");

    let stop = Arc::new(AtomicBool::new(false));
    let wpath = path.clone();
    let wstop = stop.clone();
    // Writer: random overwrites / appends / truncates, paced.
    let writer = thread::spawn(move || {
        let mut rng = XorShift::new(0x9abc_def0_1234);
        let mut cur_len = SIZE as u64;
        let mut f = OpenOptions::new().write(true).open(&wpath).expect("writer open");
        for _ in 0..150 {
            for _ in 0..4 {
                use std::io::{Seek, SeekFrom};
                match rng.next() % 4 {
                    0 => {
                        // In-place overwrite.
                        let off = rng.next() % cur_len.max(1);
                        let n = ((rng.next() % 4096) + 1).min(cur_len - off).max(1) as usize;
                        let data = vec![0xAAu8; n];
                        f.seek(SeekFrom::Start(off)).unwrap();
                        f.write_all(&data).unwrap();
                    }
                    1 => {
                        // Append 64 KiB.
                        let data = vec![0xBBu8; 65536];
                        f.seek(SeekFrom::End(0)).unwrap();
                        f.write_all(&data).unwrap();
                        cur_len += 65536;
                    }
                    _ => {
                        // Truncate (usually small, occasionally severe).
                        let n = if rng.next().is_multiple_of(20) {
                            rng.next() % (cur_len + 1)
                        } else {
                            cur_len.saturating_sub(rng.next() % 65536).max(65536)
                        };
                        f.set_len(n).unwrap();
                        cur_len = n;
                    }
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        f.sync_all().unwrap();
        wstop.store(true, Ordering::SeqCst);
    });

    // Main thread hammers the already-open mmap backend.
    let mut rng = XorShift::new(0xcafe_1234);
    let mut typed_errors = 0u64;
    let mut ok_ops = 0u64;
    for _ in 0..150 {
        let ops: [Result<(), PagerError>; 4] = [
            btext.line_count().map(|_| ()),
            btext
                .line_to_byte((rng.next() % 50_000) as usize)
                .map(|_| ()),
            btext
                .slice_bytes({
                    let s = (rng.next() % (SIZE as u64)) as usize;
                    s..(s + 8192).min(SIZE)
                })
                .map(|_| ()),
            {
                btext.char_at((rng.next() % (SIZE as u64)) as usize);
                Ok::<(), PagerError>(())
            },
        ];
        for op in ops {
            match op {
                Ok(()) => ok_ops += 1,
                Err(
                    PagerError::Truncated { .. }
                    | PagerError::FileChanged { .. }
                    | PagerError::Io { .. }
                    | PagerError::OutOfRange { .. },
                ) => typed_errors += 1,
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    writer.join().expect("writer panicked");
    assert!(stop.load(Ordering::SeqCst));
    eprintln!("mmap write-burst storm: {ok_ops} ok + {typed_errors} typed errors, no SIGBUS");
    assert!(typed_errors > 0, "storm never raced: no typed errors observed");
}

// ---------------------------------------------------------------------------
// (4) Rename-over under an active mapping.
// ---------------------------------------------------------------------------

#[test]
fn rename_over_under_mmap() {
    let path = tmp_path("rename-mmap");
    let new_file = tmp_path("rename-mmap-new");
    let _guard = TempGuard {
        paths: vec![path.clone(), new_file.clone()],
    };
    let old_content = vec![b'O'; 1024 * 1024];
    let new_content = vec![b'N'; 2 * 1024 * 1024];
    write_file(&path, &old_content);

    let region = MmapRegion::new(fs::File::open(&path).expect("open")).expect("mmap");
    let before = region
        .read_guarded(0, 1024, |_, s| s.to_vec())
        .expect("baseline read");
    assert_eq!(before, vec![b'O'; 1024]);

    // Atomically swap the inode at `path`.
    write_file(&new_file, &new_content);
    fs::rename(&new_file, &path).expect("rename failed");

    // The mapping pins the OLD inode via the open fd: reads are unaffected,
    // no error, no crash. (The fd-based revalidation sees the old inode.)
    let after = region
        .read_guarded(0, 1024, |_, s| s.to_vec())
        .expect("read after rename-over failed");
    assert_eq!(after, vec![b'O'; 1024], "mapping must keep serving the pinned inode");
    eprintln!("rename-over: mapping kept serving the old inode, no error, no crash");
}

// ---------------------------------------------------------------------------
// (5) Shrink under a cached line index.
// ---------------------------------------------------------------------------

#[test]
fn shrink_under_cached_line_index() {
    let path = tmp_path("shrink-lineidx");
    let _guard = TempGuard { paths: vec![path.clone()] };
    // Line-oriented content: 200k lines.
    let mut content = Vec::new();
    for i in 0..200_000u32 {
        content.extend_from_slice(format!("line {i:06}\n").as_bytes());
    }
    write_file(&path, &content);

    let btext = BText::open_mapped_mmap(&path).expect("open_mapped_mmap failed");
    // Warm the cached LineIndex on mapped leaves.
    let b0 = btext.line_to_byte(1000).expect("warmup line_to_byte").expect("line 1000");
    assert!(b0 > 0);
    let lc = btext.line_count().expect("warmup line_count");
    assert_eq!(lc, 200_001, "line_count = newlines + 1");

    // Shrink to half.
    truncate_to(&path, (content.len() / 2) as u64);

    // Lines in the readable prefix still resolve; lines past the shrink
    // surface as typed errors — never a panic, never garbage.
    assert!(btext.line_to_byte(100).is_ok(), "prefix line must still resolve");
    let mut saw_typed = false;
    for probe in [199_999usize, 150_000, 199_000] {
        match btext.line_to_byte(probe) {
            Ok(_) => {}
            Err(
                PagerError::Truncated { .. }
                | PagerError::FileChanged { .. }
                | PagerError::Io { .. }
                | PagerError::OutOfRange { .. },
            ) => saw_typed = true,
        }
    }
    assert!(saw_typed, "expected typed errors for lines past the shrink");
    eprintln!("shrink under cached line index: prefix ok, past-shrink -> typed errors");
}

#[test]
fn shrink_regrow_clears_sticky_truncated() {
    // Regression: the `cached_len` fast path in `check_range` returned
    // `Truncated` without revalidating. After shrink→Truncated→regrow, reads
    // at offsets past the shrunken length stayed `Truncated` forever, even
    // though the file was whole again. The pager (fresh fstat in
    // `revalidate`) correctly reported `FileChanged`; the mmap backend must
    // match.
    let path = tmp_path("shrink-regrow");
    let _guard = TempGuard { paths: vec![path.clone()] };
    const SIZE: usize = 8192;
    fill_file(&path, SIZE, 0x5);

    let file = std::fs::File::open(&path).expect("open");
    let region = MmapRegion::new(file).expect("MmapRegion::new");

    // Shrink to 100 bytes. A read past the shrink observes `Truncated` and
    // poisons `cached_len`.
    truncate_to(&path, 100);
    match region.read_guarded(7000, 100, |_, s| s.to_vec()) {
        Err(PagerError::Truncated { .. }) => {}
        other => panic!("expected Truncated after shrink, got {:?}", other.is_ok()),
    }

    // Regrow to the open length with different content (mtime moves).
    fill_file(&path, SIZE, 0x6);

    // Must NOT be sticky `Truncated`: the file is 8192 bytes again, so this
    // is a same-length content replacement → `FileChanged`.
    match region.read_guarded(7000, 100, |_, s| s.to_vec()) {
        Err(PagerError::FileChanged { .. }) => {}
        Err(PagerError::Truncated { .. }) => {
            panic!("sticky Truncated after regrow: expected FileChanged")
        }
        other => panic!("expected FileChanged after regrow, got {:?}", other.is_ok()),
    }
    eprintln!("shrink→regrow: Truncated cleared, FileChanged reported");
}

#[test]
fn fault_does_not_double_drop_closure_captures() {
    // Regression: `guard::guarded`'s fault path emitted drop glue for the
    // closure `f` after the `FnOnce` call had already consumed it —
    // double-dropping owned captures. The `ManuallyDrop` wrapper ensures
    // captures drop exactly once.
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct DropCounter(std::sync::Arc<AtomicUsize>);
    impl Drop for DropCounter {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    let path = tmp_path("double-drop");
    let _guard = TempGuard { paths: vec![path.clone()] };
    const SIZE: usize = 65536;
    fill_file(&path, SIZE, 0x7);

    let file = std::fs::File::open(&path).expect("open");
    let region = MmapRegion::new(file).expect("MmapRegion::new");

    // Truncate the file so the read faults.
    truncate_to(&path, 100);

    let drops = std::sync::Arc::new(AtomicUsize::new(0));
    let counter = DropCounter(drops.clone());
    // The closure captures `counter` (owned). The read faults (file
    // truncated), so we take the fault path. `counter` must drop exactly
    // once — via the `FnOnce` call — not twice.
    let result: Result<Vec<u8>, PagerError> =
        region.read_guarded(0, SIZE, |_, s| {
            let _ = &counter;
            s.to_vec()
        });
    assert!(matches!(result, Err(PagerError::Truncated { .. })), "expected Truncated, got {:?}", result.is_ok());
    // The fault path must NOT double-drop: without the `ManuallyDrop` fix,
    // the compiler emits drop glue for `f` in the recovery branch after the
    // `FnOnce` call already consumed it (2 drops). With the fix, the
    // closure is leaked on the fault path (0 drops) — safe, not a double
    // free. Either 0 or 1 is correct; 2 is the bug.
    let n = drops.load(Ordering::SeqCst);
    assert!(n <= 1, "closure captures double-dropped on fault path: {}", n);
    eprintln!("fault path: closure captures dropped {} time(s), no double-drop", n);
}
