//! In-house byte B-tree (`BText`) backing every buffer.
//!
//! This module replaces `ropey`. Design record: `~/workspace/erax-work/DATA-STRUCTURE.md`.
//!
//! # Shape
//! A persistent (path-copying) B-tree over byte chunks, piece-table style.
//! Leaves are either **owned** byte chunks (`Rc<[u8]>`, shared between tree
//! versions at zero copy) or **mapped** regions of the original file
//! (`(pager, offset, len)`), materialized on demand through the [`Pager`].
//! Every node carries its total **byte length** eagerly; **newline counts**
//! resolve lazily and are cached (mapped regions are never scanned at open).
//!
//! # Persistence
//! Edits never mutate in place: the path from the root to the edited leaf is
//! rebuilt, unchanged subtrees are shared via `Rc`. A `BText` clone is an
//! O(1) snapshot. Undo goes through the editor's [`UndoGroup`] machinery
//! (byte-position [`Edit`] pairs — exact inverses, proven by V-02), not through
//! retained roots; the tree's persistence additionally gives cheap consistent
//! snapshots for rendering.
//!
//! [`UndoGroup`]: super::undo_group::UndoGroup
//! [`Edit`]: super::buffer::Edit
//!
//! # `unsafe` budget
//! This module is the owner's single authorized `unsafe` scope, and the base
//! implementation below uses **no `unsafe` at all**: the chunk store is
//! `Rc<[u8]>` sharing (safe), the pager is safe `pread`-style I/O. The only
//! `unsafe` that may ever appear here is the memory-mapping fast path, which
//! is pre-approved **iff** benchmarks show it beating the pager beyond noise,
//! and then only in a dedicated `mmap` submodule with `SAFETY` proofs,
//! fault-injection tests, and an entry in the vendored-`unsafe` inventory.
//! `unsafe_code = deny` stays in force for the rest of the crate.
//!
//! # Invariants (see VERIFICATION.md Tier 0)
//! - I1: every internal node has between 2 and `BTREE_MAX_CHILDREN`
//!   children; the root may additionally be a single leaf. Deletions restore
//!   the classic B-tree minimum fan-out only best-effort (see
//!   `collapse_and_merge`); the arity-2 lower bound is what keeps depth
//!   logarithmic — depth <= log2(bytes) + 1 — and it holds by construction
//!   because single-child internal nodes are always collapsed.
//! - I2: `bytes` on every node equals the sum of its children's `bytes`.
//! - I3: a cached newline count, once set, never changes (nodes are immutable).
//! - I4: owned leaves are at most `MAX_LEAF_BYTES` bytes, except transiently
//!   during construction (split immediately after).
//! - I5: mapped leaves are immutable originals. Inserts split the region
//!   *descriptor* around new owned bytes (no pager read); deletes
//!   materialize the affected span to return the deleted bytes. Either way
//!   the pager never serves a byte for an edited region.
//! - I6: page metrics, once computed, are never recomputed (cached per page).

use std::borrow::Cow;
use std::cell::{Ref, RefCell};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::rc::Rc;

use crate::config::editor::{
    BTREE_MAX_CHILDREN, MAP_REGION_BYTES, MAX_LEAF_BYTES, PAGE_CACHE_MAX_PAGES, PAGE_SIZE_BYTES,
};

/// Heuristic "underfull" threshold for the best-effort deletion merges in
/// `collapse_and_merge`. This is NOT the hard invariant (I1's hard lower
/// bound is 2); it is the target the merge pass tries to restore when cheap.
/// Kept as a function of the max so the two cannot drift apart.
const BTREE_MIN_CHILDREN: usize = BTREE_MAX_CHILDREN / 2;

// ---------------------------------------------------------------------------
// Pager
// ---------------------------------------------------------------------------

/// Typed error for pager I/O. Truncation and I/O failures are values, never
/// panics and never signals: there is no `mmap` in the baseline, so there is
/// no `SIGBUS` to survive.
#[derive(Debug)]
pub enum PagerError {
    /// The underlying file could not be read at `offset`.
    Io { offset: u64, source: io::Error },
    /// The file shrank (or was replaced by a shorter file) under the open
    /// buffer: a read at `offset` reached end-of-file first.
    Truncated { offset: u64, wanted: u64 },
    /// `offset + len` exceeds the file length recorded at open.
    OutOfRange { offset: u64, len: u64, file_len: u64 },
    /// The file's content was replaced under the open buffer (same length,
    /// different mtime — truncate+regrow or in-place rewrite). The pinned
    /// snapshot no longer exists on disk; the read is refused rather than
    /// silently serving new bytes.
    FileChanged { offset: u64 },
}

impl std::fmt::Display for PagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PagerError::Io { offset, source } => {
                write!(f, "pager I/O error at file offset {offset}: {source}")
            }
            PagerError::Truncated { offset, wanted } => write!(
                f,
                "file truncated under open buffer: wanted {wanted} bytes at offset {offset}"
            ),
            PagerError::OutOfRange {
                offset,
                len,
                file_len,
            } => write!(
                f,
                "pager read [{offset}, {offset}+{len}) beyond file length {file_len}"
            ),
            PagerError::FileChanged { offset } => {
                write!(f, "file changed under open buffer at offset {offset}")
            }
        }
    }
}

impl std::error::Error for PagerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PagerError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Per-page newline count, computed once at fetch and never evicted.
/// (~16 bytes per 64 KiB page — `Option<PageMetrics>` has no niche to
/// exploit — so ~256 KiB per gigabyte of mapped file: bounded, tiny, and
/// the price of a whole-file line index that never scans at open.)
#[derive(Debug, Clone, Copy, Default)]
struct PageMetrics {
    newlines: usize,
}

/// Count newlines in a byte slice. `memchr`-style: the standard library's
/// slice iterator is what the optimizer vectorizes here; the assembly is
/// checked in the micro-opt pass, not assumed.
fn count_newlines(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| b == b'\n').count()
}

/// Editor-owned pager: on-demand, bounded, LRU-cached file reads.
///
/// The original file is never fully read. Pages are fetched with positional
/// reads (`pread` semantics: no shared file offset is mutated, so concurrent
/// logical reads are safe) into a bounded LRU cache. A page fetch that hits
/// end-of-file becomes `PagerError::Truncated` — an ordinary value the caller
/// turns into UI, not a crash.
#[derive(Debug)]
struct Pager {
    file: File,
    /// File length at open. Reads are bounds-checked against this; a file
    /// that grows afterwards is read only up to the recorded length (the
    /// buffer is a snapshot of the open moment, like every other editor).
    len: u64,
    /// mtime at open. Used to detect content replacement (truncate+regrow
    /// with the same length): if the mtime changed, the pinned snapshot no
    /// longer exists on disk.
    open_mtime: Option<std::time::SystemTime>,
    page_size: usize,
    max_pages: usize,
    /// Resident pages: page index -> bytes. Bounded by `max_pages`.
    pages: HashMap<u64, Vec<u8>>,
    /// LRU order: front = least recently used. `pages` and `lru` agree on keys.
    lru: VecDeque<u64>,
    /// Per-page metrics, indexed by page number. `None` = not yet fetched.
    /// Never evicted: this is the whole-file line index, built lazily.
    metrics: Vec<Option<PageMetrics>>,
}

/// `pread` that tolerates a short read at end-of-file, reporting how many
/// bytes were actually obtained. A short read mid-file is an I/O error.
#[cfg(unix)]
fn pread_partial(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    let mut total = 0;
    while total < buf.len() {
        match file.read_at(&mut buf[total..], offset + total as u64) {
            Ok(0) => break, // EOF
            Ok(n) => total += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

#[cfg(windows)]
fn pread_partial(file: &mut File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(offset))?;
    let mut total = 0;
    while total < buf.len() {
        match file.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

impl Pager {
    /// Open a pager over `path`. Performs `fstat` only — no content is read.
    /// Opening is O(1) in file size by construction.
    fn open(path: &Path) -> Result<Self, PagerError> {
        let file = File::open(path).map_err(|source| PagerError::Io { offset: 0, source })?;
        let meta = file
            .metadata()
            .map_err(|source| PagerError::Io { offset: 0, source })?;
        let len = meta.len();
        let open_mtime = meta.modified().ok();
        let page_count = len.div_ceil(PAGE_SIZE_BYTES as u64) as usize;
        Ok(Pager {
            file,
            len,
            open_mtime,
            page_size: PAGE_SIZE_BYTES,
            max_pages: PAGE_CACHE_MAX_PAGES,
            pages: HashMap::new(),
            lru: VecDeque::new(),
            metrics: vec![None; page_count],
        })
    }

    /// Revalidate the snapshot against the live file (one fstat). A read
    /// range extending beyond the current length means the pinned snapshot
    /// is gone there — `Truncated` (a pure shrink still serves its intact
    /// prefix byte-exact; only the gone region refuses). Same length +
    /// different mtime means the content was replaced (truncate+regrow or
    /// in-place rewrite) — `FileChanged`. Anything else (tolerated append,
    /// intact prefix of a shrink) proceeds; reads are clamped to the pinned
    /// length. This is the shared external-mutation rule for both backends
    /// — the mmap backend's `post_read_check` implements the same arms.
    fn revalidate(&self, offset: u64, wanted: u64) -> Result<(), PagerError> {
        if let Some(open_mtime) = self.open_mtime
            && let Ok(meta) = self.file.metadata()
        {
            let cur_len = meta.len();
            if offset.saturating_add(wanted) > cur_len {
                return Err(PagerError::Truncated { offset, wanted });
            }
            if cur_len == self.len
                && let Ok(mtime) = meta.modified()
                && mtime != open_mtime
            {
                return Err(PagerError::FileChanged { offset });
            }
        }
        Ok(())
    }

    /// Fetch a page into the cache (no-op if resident). Computes and stores
    /// the page's metrics on first fetch.
    fn ensure_page(&mut self, page_idx: u64) -> Result<(), PagerError> {
        if let Some(page) = self.pages.get(&page_idx) {
            // Cache hit: a resident page is not a license to serve stale
            // bytes as current — revalidate every hit, like the miss path
            // and like the mmap backend's post-read check. The range is the
            // cached page's actual length: a partial last page must not
            // trip the truncation arm when the file is intact.
            let offset = page_idx * self.page_size as u64;
            let page_len = page.len() as u64;
            self.revalidate(offset, page_len)?;
            self.touch(page_idx);
            return Ok(());
        }
        let offset = page_idx * self.page_size as u64;
        if offset >= self.len {
            return Err(PagerError::OutOfRange {
                offset,
                len: self.page_size as u64,
                file_len: self.len,
            });
        }
        // Snapshot-integrity revalidation on a cache miss: the old bytes
        // are gone, so refuse the fetch rather than silently substituting
        // the replacement's bytes. The range checked is the actual fetch
        // (`want`), not the full page: a partial last page must not trip
        // the truncation arm when the file is intact.
        let want = (self.len - offset).min(self.page_size as u64) as usize;
        self.revalidate(offset, want as u64)?;
        let mut buf = vec![0u8; want];
        let got = pread_partial(&self.file, &mut buf, offset)
            .map_err(|source| PagerError::Io { offset, source })?;
        if got < want {
            // The file shrank under us between open and this fetch.
            return Err(PagerError::Truncated {
                offset,
                wanted: want as u64,
            });
        }
        let metrics = PageMetrics {
            newlines: count_newlines(&buf),
        };
        if let Some(slot) = self.metrics.get_mut(page_idx as usize) {
            *slot = Some(metrics);
        }
        // Evict least-recently-used pages beyond budget. Metrics stay.
        while self.pages.len() >= self.max_pages {
            if let Some(old) = self.lru.pop_front() {
                self.pages.remove(&old);
            } else {
                break;
            }
        }
        self.lru.push_back(page_idx);
        self.pages.insert(page_idx, buf);
        Ok(())
    }

    /// Mark a resident page most-recently-used. `pages`/`lru` key agreement
    /// is maintained here: only resident pages are ever touched or pushed.
    fn touch(&mut self, page_idx: u64) {
        if let Some(pos) = self.lru.iter().position(|&p| p == page_idx) {
            self.lru.remove(pos);
        }
        self.lru.push_back(page_idx);
    }

    /// Read `len` bytes at `offset`, through the page cache.
    pub fn read(&mut self, offset: u64, len: usize) -> Result<Vec<u8>, PagerError> {
        let end = offset.checked_add(len as u64).ok_or(PagerError::OutOfRange {
            offset,
            len: len as u64,
            file_len: self.len,
        })?;
        if end > self.len {
            return Err(PagerError::OutOfRange {
                offset,
                len: len as u64,
                file_len: self.len,
            });
        }
        let mut out = Vec::with_capacity(len);
        let mut remaining = len;
        let mut cur = offset;
        while remaining > 0 {
            let page_idx = cur / self.page_size as u64;
            let page_off = (cur % self.page_size as u64) as usize;
            self.ensure_page(page_idx)?;
            // `ensure_page` guarantees residency; the borrow ends here.
            let page = self.pages.get(&page_idx).expect("page resident after ensure");
            let take = remaining.min(page.len() - page_off);
            out.extend_from_slice(&page[page_off..page_off + take]);
            self.touch(page_idx);
            cur += take as u64;
            remaining -= take;
        }
        // Post-read revalidation: a replacement racing the fetch must not
        // silently substitute. Checking after the copy is precise where a
        // pre-read check has a TOCTOU window — the same reason the mmap
        // backend checks after the guarded deref (`post_read_check`).
        self.revalidate(offset, len as u64)?;
        Ok(out)
    }

    /// Metrics for one page, fetching it if needed. Returned by copy: small.
    pub fn page_metrics(&mut self, page_idx: u64) -> Result<PageMetrics, PagerError> {
        self.ensure_page(page_idx)?;
        self.touch(page_idx);
        Ok(self.metrics[page_idx as usize].expect("metrics set by ensure_page"))
    }

    pub fn len(&self) -> u64 {
        self.len
    }
}

// ---------------------------------------------------------------------------
/// Shared ownership of one file's pager across tree snapshots.
/// Storage backend for mapped leaves: the syscall pager or a memory mapping.
/// (The mmap backend's mitigation set — fstat-validated length,
/// revalidation epochs, a SIGBUS handler with scoped recovery, immutable
/// PROT_READ mapping, fault-injection tests — lives in `crate::core::mmap`.)
/// The mmap backend is Unix-only; on other platforms only `Pager` exists.
enum Backend {
    Pager(Pager),
    #[cfg(any(unix, windows))]
    Mmap(crate::core::mmap::MmapRegion),
}

#[derive(Clone)]
struct SharedPager(Rc<RefCell<Backend>>);

impl std::fmt::Debug for SharedPager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedPager").finish_non_exhaustive()
    }
}

impl Backend {
    fn len(&self) -> u64 {
        match self {
            Backend::Pager(p) => p.len(),
            #[cfg(any(unix, windows))]
            Backend::Mmap(m) => m.len(),
        }
    }

    fn read(&mut self, offset: u64, len: usize) -> Result<Vec<u8>, PagerError> {
        match self {
            Backend::Pager(p) => p.read(offset, len),
            // The copy happens INSIDE the fault guard but into
            // caller-owned storage: `read_guarded_into` reserves before
            // arming, so a fault mid-copy cannot strand an allocation
            // (the old `|s| s.to_vec()` shape allocated inside the window
            // and leaked it on the fault path — see mmap.rs).
            #[cfg(any(unix, windows))]
            Backend::Mmap(m) => {
                let mut out = Vec::new();
                m.read_guarded_into(offset, len, &mut out)?;
                Ok(out)
            }
        }
    }

    /// Newline count in `[offset, offset + len)`. For the pager, uses cached
    /// page metrics; for mmap, reads the range directly (no pages).
    fn count_newlines_in(&mut self, offset: u64, len: u64) -> Result<usize, PagerError> {
        match self {
            Backend::Pager(p) => {
                if len == 0 {
                    return Ok(0);
                }
                let page_size = PAGE_SIZE_BYTES as u64;
                let end = offset.saturating_add(len);
                let first = offset / page_size;
                let last = end.saturating_sub(1) / page_size;
                if first == last {
                    return Ok(count_newlines(&p.read(offset, len as usize)?));
                }
                let mut total = 0;
                let first_end = (first + 1).saturating_mul(page_size);
                total += count_newlines(&p.read(offset, (first_end - offset) as usize)?);
                for pg in first + 1..last {
                    total += p.page_metrics(pg)?.newlines;
                }
                let last_start = last.saturating_mul(page_size);
                total += count_newlines(&p.read(last_start, (end - last_start) as usize)?);
                Ok(total)
            }
            // Pure byte scan: no allocation, no locks — safe inside the
            // guarded window by construction.
            #[cfg(any(unix, windows))]
            Backend::Mmap(m) => {
                if len == 0 {
                    return Ok(0);
                }
                m.read_guarded(offset, len as usize, |_, s| count_newlines(s))
            }
        }
    }
}

impl SharedPager {
    fn new(pager: Pager) -> Self {
        SharedPager(Rc::new(RefCell::new(Backend::Pager(pager))))
    }

    #[cfg(any(unix, windows))]
    fn new_mmap(mmap: crate::core::mmap::MmapRegion) -> Self {
        SharedPager(Rc::new(RefCell::new(Backend::Mmap(mmap))))
    }

    /// Read `len` bytes at `offset`.
    fn read(&self, offset: u64, len: usize) -> Result<Vec<u8>, PagerError> {
        self.0.borrow_mut().read(offset, len)
    }

    /// Newline count in `[offset, offset + len)`.
    fn count_newlines_in(&self, offset: u64, len: u64) -> Result<usize, PagerError> {
        self.0.borrow_mut().count_newlines_in(offset, len)
    }
}

// ---------------------------------------------------------------------------
// Tree
// ---------------------------------------------------------------------------

/// A leaf: either owned bytes shared between tree versions, or an immutable
/// region of the original file materialized on demand (I5).
#[derive(Debug)]
enum Leaf {
    Owned {
        bytes: Rc<[u8]>,
        /// Lazy newline index (offsets of every `b'\n'`). Built on the first
        /// line lookup that reaches this leaf; (I3) bytes are immutable so a
        /// built index stays valid across snapshots — splits and merges
        /// create new leaves (index `None`), never mutate bytes.
        /// Without this cache, `line_to_byte` would scan up to 64 KiB per
        /// call to find the k-th newline — O(n²) on repeated line lookups.
        lines: RefCell<Option<LineIndex>>,
    },
    Mapped {
        pager: SharedPager,
        offset: u64,
        len: u64,
        /// Cached newline count; `None` until first resolution. (I3: nodes
        /// are immutable, so cached values stay valid across snapshots.)
        newlines: RefCell<Option<usize>>,
        /// Cached newline index (offsets of every `b'\n'` in the region).
        /// Built on first line-lookup; for mmap this is a direct memory scan,
        /// for pager it uses page-skipping. (Without this cache, every
        /// `newline_pos` on a mapped region would scan up to 1 MiB.)
        line_index: RefCell<Option<LineIndex>>,
    },
}

/// Newline offsets within one owned leaf's bytes: `offsets[i]` is the byte
/// offset of the (i+1)-th `b'\n'`. Byte-level only (no encoding awareness),
/// consistent with the tree staying encoding-dumb.
#[derive(Debug, Clone)]
struct LineIndex {
    offsets: Vec<u32>,
}

impl LineIndex {
    fn build(bytes: &[u8]) -> Self {
        let mut offsets = Vec::new();
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                offsets.push(i as u32);
            }
        }
        LineIndex { offsets }
    }

    fn count(&self) -> usize {
        self.offsets.len()
    }
}

impl Leaf {
    /// Owned leaf with a cold newline index.
    fn owned(bytes: Rc<[u8]>) -> Self {
        Leaf::Owned {
            bytes,
            lines: RefCell::new(None),
        }
    }

    /// The empty owned leaf (index known: no newlines).
    fn owned_empty() -> Self {
        Leaf::Owned {
            bytes: Rc::from(&[][..]),
            lines: RefCell::new(Some(LineIndex { offsets: Vec::new() })),
        }
    }

    /// This leaf's newline index, building it on first use. `None` for
    /// mapped leaves (they use page metrics instead).
    fn owned_index(&self) -> Option<Ref<'_, LineIndex>> {
        let (bytes, lines) = match self {
            Leaf::Owned { bytes, lines } => (bytes, lines),
            _ => return None,
        };
        if lines.borrow().is_none() {
            *lines.borrow_mut() = Some(LineIndex::build(bytes));
        }
        Some(Ref::map(lines.borrow(), |o| {
            o.as_ref().expect("newline index built above")
        }))
    }

    fn bytes_len(&self) -> usize {
        match self {
            Leaf::Owned { bytes, .. } => bytes.len(),
            Leaf::Mapped { len, .. } => *len as usize,
        }
    }

    /// Newline count, resolving through the pager for mapped leaves.
    /// The result is cached: each page is scanned at most once ever (I6).
    fn newlines(&self) -> Result<usize, PagerError> {
        match self {
            Leaf::Owned { .. } => Ok(self
                .owned_index()
                .expect("owned leaf has a newline index")
                .count()),
            Leaf::Mapped {
                pager,
                offset,
                len,
                newlines,
                ..
            } => {
                if let Some(n) = *newlines.borrow() {
                    return Ok(n);
                }
                // In-region newlines only. Uses the backend's
                // `count_newlines_in` (pager: page-skipping; mmap: direct scan).
                let total = pager.count_newlines_in(*offset, *len)?;
                *newlines.borrow_mut() = Some(total);
                Ok(total)
            }
        }
    }

    /// Materialize the leaf's bytes: a borrow for owned chunks (zero-copy),
    /// a pager read for mapped regions.
    fn read(&self) -> Result<Cow<'_, [u8]>, PagerError> {
        match self {
            Leaf::Owned { bytes: rc, .. } => Ok(Cow::Borrowed(rc)),
            Leaf::Mapped {
                pager,
                offset,
                len,
                ..
            } => Ok(Cow::Owned(
                pager.read(*offset, *len as usize)?,
            )),
        }
    }

    /// Materialize a sub-range `[start, start+len)` of the leaf: a borrow
    /// for owned chunks (zero-copy), a pager read for mapped regions.
    /// Mapped reads only fetch the pages the range touches — a line lookup
    /// or small slice never materializes the whole mapped region (which can
    /// be 1 MiB). Out-of-range requests clamp; they never panic.
    fn read_range(&self, start: usize, len: usize) -> Result<Cow<'_, [u8]>, PagerError> {
        match self {
            Leaf::Owned { bytes: rc, .. } => {
                debug_assert!(start.saturating_add(len) <= rc.len());
                let avail = rc.len().saturating_sub(start);
                let len = len.min(avail);
                Ok(Cow::Borrowed(&rc[start..start + len]))
            }
            Leaf::Mapped {
                pager,
                offset,
                len: region_len,
                ..
            } => {
                debug_assert!((start as u64).saturating_add(len as u64) <= *region_len);
                let avail = (*region_len as usize).saturating_sub(start);
                let len = len.min(avail);
                // offset + start <= offset + region_len <= file_len: no overflow.
                Ok(Cow::Owned(
                    pager.read(*offset + start as u64, len)?,
                ))
            }
        }
    }
}

#[derive(Debug)]
enum Node {
    Leaf(Leaf),
    Internal {
        children: Vec<Rc<Node>>,
        /// Always known eagerly (sums of child byte lengths).
        bytes: usize,
        /// Lazily resolved + cached (I3).
        newlines: RefCell<Option<usize>>,
    },
}

impl Node {
    fn bytes(&self) -> usize {
        match self {
            Node::Leaf(leaf) => leaf.bytes_len(),
            Node::Internal { bytes, .. } => *bytes,
        }
    }

    fn newlines(&self) -> Result<usize, PagerError> {
        match self {
            Node::Leaf(leaf) => leaf.newlines(),
            Node::Internal {
                children, newlines, ..
            } => {
                if let Some(n) = *newlines.borrow() {
                    return Ok(n);
                }
                let mut total = 0;
                for child in children {
                    total += child.newlines()?;
                }
                *newlines.borrow_mut() = Some(total);
                Ok(total)
            }
        }
    }
}

/// `BText`: a persistent byte B-tree. Cloning is an O(1) snapshot (`Rc`
/// root); edits rebuild only the path to the change.
#[derive(Debug, Clone)]
pub struct BText {
    root: Rc<Node>,
}

impl Default for BText {
    fn default() -> Self {
        Self::new()
    }
}

/// Assemble leaves bottom-up into a balanced tree (I1 by construction:
/// `balanced_groups` never strands a 1-child node).
fn build_balanced(level: Vec<Rc<Node>>) -> Rc<Node> {
    debug_assert!(!level.is_empty());
    if level.len() == 1 {
        return level.into_iter().next().expect("build_balanced: non-empty");
    }
    let next = if level.len() <= BTREE_MAX_CHILDREN {
        vec![internal_of(level)]
    } else {
        balanced_groups(level)
    };
    build_balanced(next)
}

/// Internal node with eagerly summed byte counts (I2).
fn internal_of(children: Vec<Rc<Node>>) -> Rc<Node> {
    debug_assert!(!children.is_empty());
    debug_assert!(children.len() <= BTREE_MAX_CHILDREN);
    let bytes = children.iter().map(|c| c.bytes()).sum();
    Rc::new(Node::Internal {
        children,
        bytes,
        newlines: RefCell::new(None),
    })
}

/// Group an overfull child list (`len` > `BTREE_MAX_CHILDREN`) into balanced
/// internal nodes, each with 2..=`BTREE_MAX_CHILDREN` children. Sizes differ
/// by at most one: a naive `chunks(MAX)` split can strand a trailing 1-child
/// group (I1 violation — found by the invariant checker), so the remainder
/// is distributed instead.
fn balanced_groups(children: Vec<Rc<Node>>) -> Vec<Rc<Node>> {
    debug_assert!(children.len() > BTREE_MAX_CHILDREN);
    let n = children.len();
    let groups = n.div_ceil(BTREE_MAX_CHILDREN);
    let mut out = Vec::with_capacity(groups);
    let mut iter = children.into_iter();
    for g in 0..groups {
        let size = n / groups + usize::from(g < n % groups);
        out.push(internal_of(iter.by_ref().take(size).collect()));
    }
    debug_assert!(iter.next().is_none(), "balanced_groups: exact partition");
    out
}

/// True for UTF-8 continuation bytes (0x80..0xC0).
fn is_utf8_continuation(b: u8) -> bool {
    (b & 0xC0) == 0x80
}

/// UTF-8 sequence length from the lead byte: `None` for continuation bytes
/// and invalid leads (`0xF5..=0xFF`). Shared spec knowledge between
/// [`BText::char_at`] and the test oracle below.
fn utf8_lead_len(lead: u8) -> Option<usize> {
    match lead {
        0x00..=0x7F => Some(1),
        0x80..=0xBF => None,
        0xC0..=0xDF => Some(2),
        0xE0..=0xEF => Some(3),
        0xF0..=0xF4 => Some(4),
        _ => None,
    }
}

impl BText {
    /// Empty text: a single empty owned leaf.
    pub fn new() -> Self {
        BText {
            root: Rc::new(Node::Leaf(Leaf::owned_empty())),
        }
    }

    /// Text from bytes: chunked into owned leaves, assembled bottom-up into
    /// a balanced tree (I1 holds by construction).
    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self::new();
        }
        let leaves: Vec<Rc<Node>> = bytes
            .chunks(MAX_LEAF_BYTES)
            .map(|c| Rc::new(Node::Leaf(Leaf::owned(Rc::from(c)))))
            .collect();
        BText {
            root: build_balanced(leaves),
        }
    }

    /// Open a file lazily: `fstat` only, no content read — O(1) in file size
    /// by construction. The file becomes one mapped leaf per
    /// `MAP_REGION_BYTES` region; bytes (and newline counts) materialize
    /// on first touch.
    ///
    /// Backend (session-7 adoption decision): on Unix this uses the mmap
    /// backend — 29–61x faster random reads than the pager on the large-file
    /// suite, with the full mitigation stack (SIGBUS handler with scoped
    /// recovery, post-read snapshot validation, immutable mapping,
    /// fault-injection tested) in `crate::core::mmap`. A fault becomes a
    /// typed `PagerError`, never process death; a same-length content
    /// replacement becomes `FileChanged` on both backends (the pager
    /// revalidates the snapshot on every read — see `open_mapped_pager`).
    /// On other platforms, and via `open_mapped_pager` everywhere, the
    /// syscall pager is used.
    pub fn open_mapped(path: &Path) -> Result<Self, PagerError> {
        #[cfg(any(unix, windows))]
        {
            Self::open_mapped_mmap(path)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let pager = Pager::open(path)?;
            let len = pager.len();
            if len == 0 {
                return Ok(Self::new());
            }
            let shared = SharedPager::new(pager);
            Ok(Self::from_shared(shared))
        }
    }

    /// Open with the mmap backend (Unix and Windows). This is what
    /// `open_mapped` uses on those platforms (session-7 adoption decision).
    ///
    /// 29–61x faster random reads than the pager on the large-file suite.
    /// The full mitigation stack is implemented (`crate::core::mmap`):
    /// fstat-validated length, a fault handler with scoped recovery (a
    /// fault becomes `PagerError::Truncated`, never process death),
    /// post-read snapshot validation (a same-length replacement becomes
    /// `PagerError::FileChanged`, never silent substitution), an immutable
    /// read-only mapping, and fault-injection tests. External-mutation
    /// semantics match the pager exactly (see `open_mapped_pager`).
    #[cfg(any(unix, windows))]
    pub fn open_mapped_mmap(path: &Path) -> Result<Self, PagerError> {
        let file = std::fs::File::open(path)
            .map_err(|source| PagerError::Io { offset: 0, source })?;
        // Propagate the real `MmapRegion::new` error (which carries the
        // OS error); substituting a generic "mmap failed" would hide the
        // cause (e.g. a Windows `MapViewOfFile` size mismatch).
        let mmap = crate::core::mmap::MmapRegion::new(file)?;
        let len = mmap.len();
        if len == 0 {
            return Ok(Self::new());
        }
        let shared = SharedPager::new_mmap(mmap);
        Ok(Self::from_shared(shared))
    }

    /// Open with the syscall-pager backend (the non-Unix default; also the
    /// explicit fallback on Unix). External-mutation semantics match mmap:
    /// a same-length content replacement under the open file is refused
    /// with `PagerError::FileChanged` — on a cache hit, a cache miss, and
    /// after the copy (TOCTOU) — never silently served from the cache.
    pub fn open_mapped_pager(path: &Path) -> Result<Self, PagerError> {
        let pager = Pager::open(path)?;
        let len = pager.len();
        if len == 0 {
            return Ok(Self::new());
        }
        let shared = SharedPager::new(pager);
        Ok(Self::from_shared(shared))
    }

    fn from_shared(shared: SharedPager) -> Self {
        let len = shared.0.borrow().len();
        let mut leaves = Vec::new();
        let mut offset = 0u64;
        while offset < len {
            let region = (len - offset).min(MAP_REGION_BYTES);
            leaves.push(mapped_leaf(shared.clone(), offset, region));
            offset += region;
        }
        BText {
            root: build_balanced(leaves),
        }
    }

    /// Total byte length. O(1): byte counts are always known eagerly.
    pub fn len(&self) -> usize {
        self.root.bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Total newline count. For fully-owned trees this is O(1) after the
    /// first call (cached); for mapped regions it streams each page at most
    /// once ever, then caches. This is the documented amortized-once cost of
    /// lazy opening — `open_mapped` itself never scans.
    pub fn total_newlines(&self) -> Result<usize, PagerError> {
        self.root.newlines()
    }

    /// Number of lines. A text with N newlines has N+1 lines when non-empty;
    /// an empty text has 1 line (the editor's long-standing convention).
    pub fn line_count(&self) -> Result<usize, PagerError> {
        Ok(self.total_newlines()? + 1)
    }

    /// Byte offset of the start of line `line_idx` (0-based). `None` if
    /// `line_idx` is out of range.
    pub fn line_to_byte(&self, line_idx: usize) -> Result<Option<usize>, PagerError> {
        // Bounds check against the line count — but in the degraded
        // (truncated-file) case the count itself is unknowable. Then we
        // skip the check and let `newline_pos` fail naturally: lines in the
        // readable prefix still resolve, lines in the truncated region
        // surface the pager error.
        let total_lines = match self.line_count() {
            Ok(n) => n,
            Err(_) => usize::MAX,
        };
        if line_idx >= total_lines {
            return Ok(None);
        }
        if line_idx == 0 {
            return Ok(Some(0));
        }
        self.newline_pos(line_idx)
    }

    /// Byte offset just past the `target`-th newline (`target` 1-based),
    /// i.e. the start of 0-based line `target`. A single tree descent; owned
    /// leaves answer from their newline index in O(1). `None` if the target
    /// newline does not exist.
    fn newline_pos(&self, target: usize) -> Result<Option<usize>, PagerError> {
        debug_assert!(target >= 1);
        // Bounds check first: the descent's debug_assert below is a true
        // invariant only when the target exists. Without this, asking for a
        // newline past the end (e.g. line 0's end in a newline-free text)
        // would trip the invariant instead of returning None.
        // Degraded (truncated-file) case: the count is unknowable; skip the
        // check and let the descent fail naturally via the pager.
        let total = match self.total_newlines() {
            Ok(n) => n,
            Err(_) => usize::MAX,
        };
        if target > total {
            return Ok(None);
        }
        // Descend, skipping `target` newlines.
        let mut node = &self.root;
        let mut t = target;
        let mut base = 0usize;
        loop {
            match node.as_ref() {
                Node::Leaf(leaf) => {
                    // (t >= 1: callers pass target >= 1 and descent preserves it.)
                    let found = match leaf {
                        Leaf::Owned { .. } => {
                            // O(1): the target-th newline's offset is indexed.
                            // A 0 here means descent inconsistency → None,
                            // matching the old scan's behavior.
                            t.checked_sub(1).and_then(|i| {
                                leaf.owned_index()
                                    .expect("owned leaf has a newline index")
                                    .offsets
                                    .get(i)
                                    .map(|&off| base + off as usize + 1)
                            })
                        }
                        Leaf::Mapped {
                            pager,
                            offset,
                            len,
                            line_index,
                            ..
                        } => {
                            // Cached newline index: built on first use via a
                            // single region scan, then O(1) lookup. Without
                            // this, every lookup scanned up to 1 MiB.
                            if line_index.borrow().is_none() {
                                let bytes = pager.read(*offset, *len as usize)?;
                                *line_index.borrow_mut() =
                                    Some(LineIndex::build(&bytes));
                            }
                            let idx = line_index.borrow();
                            let idx = idx.as_ref().expect("just built");
                            // `t` is 1-based within the leaf; `base` is the
                            // leaf's text offset.
                            idx.offsets.get(t - 1).map(|&off| base + off as usize + 1)
                        }
                    };
                    match found {
                        Some(pos) => return Ok(Some(pos)),
                        None => {
                            // The count (`total_newlines`) and this lookup
                            // are two separate reads. Under concurrent
                            // external mutation the file can change between
                            // them, so the target may no longer exist. This
                            // is not a logic invariant — return None and let
                            // the caller treat it as a raced read.
                            return Ok(None);
                        }
                    }
                }
                Node::Internal { children, .. } => {
                    let mut acc = 0;
                    let mut descended = false;
                    for child in children {
                        let nl = child.newlines()?;
                        if t <= nl {
                            base += acc;
                            node = child;
                            descended = true;
                            break;
                        }
                        t -= nl;
                        acc += child.bytes();
                    }
                    if !descended {
                        debug_assert!(false, "newline_pos: descent failed");
                        return Ok(None);
                    }
                }
            }
        }
    }

    /// 0-based line index containing byte `offset`. Offsets past the end
    /// clamp to the last line (the editor's convention).
    pub fn byte_to_line(&self, offset: usize) -> Result<usize, PagerError> {
        let offset = offset.min(self.len());
        let mut node = &self.root;
        let mut line = 0usize;
        let mut pos = offset;
        loop {
            match node.as_ref() {
                Node::Leaf(leaf) => {
                    // Only the prefix up to `pos` is needed. Owned leaves:
                    // zero-copy borrow. Mapped leaves: cached page metrics
                    // with partial edge scans — never the whole region.
                    let upto = pos.min(leaf.bytes_len());
                    let n = match leaf {
                        Leaf::Owned { bytes: rc, .. } => count_newlines(&rc[..upto]),
                        Leaf::Mapped { pager, offset, .. } => {
                            pager.count_newlines_in(*offset, upto as u64)?
                        }
                    };
                    return Ok(line + n);
                }
                Node::Internal { children, .. } => {
                    let mut acc = 0usize;
                    let mut descended = false;
                    for (i, child) in children.iter().enumerate() {
                        let cb = child.bytes();
                        let is_last = i + 1 == children.len();
                        if pos < acc + cb || (pos == acc + cb && is_last) {
                            pos -= acc;
                            node = child;
                            descended = true;
                            break;
                        }
                        acc += cb;
                        line += child.newlines()?;
                    }
                    if !descended {
                        debug_assert!(false, "byte_to_line: descent failed");
                        return Ok(line);
                    }
                }
            }
        }
    }

    /// Byte range of line `line_idx`, including the trailing newline when
    /// present. `None` if out of range.
    /// Byte range `[start, end)` of the `line_idx`-th line (0-based), `end`
    /// including the trailing newline when present. `None` if out of range.
    ///
    /// A single tree descent in the common case: the descent targets the
    /// end newline, and the start newline is read from the same leaf's index
    /// (consecutive newlines share a leaf unless the line spans a leaf
    /// boundary, which falls back to a second descent).
    pub fn line_byte_range(&self, line_idx: usize) -> Result<Option<(usize, usize)>, PagerError> {
        // Bounds check against the line count — but in the degraded
        // (truncated-file) case the count itself is unknowable. Then we
        // skip the check and let `newline_pos` fail naturally: lines in the
        // readable prefix still resolve, lines in the truncated region
        // surface the pager error.
        let total_lines = match self.line_count() {
            Ok(n) => n,
            Err(_) => usize::MAX,
        };
        if line_idx >= total_lines {
            return Ok(None);
        }
        if line_idx == 0 {
            let end = match self.newline_pos(1)? {
                Some(p) => p,
                None => self.len(),
            };
            return Ok(Some((0, end)));
        }
        // Descend once for the end newline (1-based ordinal line_idx + 1).
        let mut node = &self.root;
        let mut t = line_idx + 1;
        let mut base = 0usize;
        loop {
            match node.as_ref() {
                Node::Leaf(leaf) => {
                    return match leaf {
                        Leaf::Owned { .. } => {
                            let li = leaf
                                .owned_index()
                                .expect("owned leaf has a newline index");
                            if t == 0 || t > li.count() {
                                // Past the last newline: the final line.
                                let start = match self.newline_pos(line_idx)? {
                                    Some(p) => p,
                                    None => return Ok(None),
                                };
                                Ok(Some((start, self.len())))
                            } else {
                                let wi = t - 1; // within-leaf index of end newline
                                let end = base + li.offsets[wi] as usize + 1;
                                let start = if wi >= 1 {
                                    // Start newline shares this leaf.
                                    base + li.offsets[wi - 1] as usize + 1
                                } else {
                                    // Leaf boundary: one more descent (rare).
                                    match self.newline_pos(line_idx)? {
                                        Some(p) => p,
                                        None => return Ok(None),
                                    }
                                };
                                Ok(Some((start, end)))
                            }
                        }
                        // Mapped leaves: two single descents (rare path).
                        Leaf::Mapped { .. } => {
                            let start = match self.newline_pos(line_idx)? {
                                Some(p) => p,
                                None => return Ok(None),
                            };
                            let end = match self.newline_pos(line_idx + 1)? {
                                Some(p) => p,
                                None => self.len(),
                            };
                            Ok(Some((start, end)))
                        }
                    };
                }
                Node::Internal { children, .. } => {
                    let mut acc = 0;
                    let mut descended = false;
                    for child in children {
                        let nl = child.newlines()?;
                        if t <= nl {
                            base += acc;
                            node = child;
                            descended = true;
                            break;
                        }
                        t -= nl;
                        acc += child.bytes();
                    }
                    if !descended {
                        // t exceeds the total newline count: final line.
                        let start = match self.newline_pos(line_idx)? {
                            Some(p) => p,
                            None => return Ok(None),
                        };
                        return Ok(Some((start, self.len())));
                    }
                }
            }
        }
    }

    /// The character containing byte `byte_pos`, decoded from a small window
    /// around the position (handles multi-byte sequences and leaf-boundary
    /// crossings). `None` for out-of-range positions or invalid UTF-8 at the
    /// position — the tree is encoding-dumb by design; views decode.
    ///
    /// The window is only located, never decoded whole: decoding the whole
    /// window tail would let a *neighboring* character's bytes fail (or
    /// falsely satisfy) validation. The lead byte determines the sequence
    /// length; exactly those bytes are validated.
    pub fn char_at(&self, byte_pos: usize) -> Option<char> {
        let len = self.len();
        if byte_pos >= len {
            return None;
        }
        // A UTF-8 sequence containing `byte_pos` starts at most 3 bytes
        // earlier and is at most 4 bytes long: this window always covers it.
        let win_start = byte_pos.saturating_sub(3);
        let win_end = (byte_pos + 4).min(len);
        let window = self.slice_bytes(win_start..win_end).ok()?;
        // Walk back to the sequence's lead byte (at most 3 steps: the walk
        // cannot pass win_start, which is >= byte_pos - 3).
        let rel_pos = byte_pos - win_start;
        let mut seq_start = rel_pos;
        while seq_start > 0 && is_utf8_continuation(window[seq_start]) {
            seq_start -= 1;
        }
        let seq_len = utf8_lead_len(window[seq_start])?;
        let seq_end = seq_start + seq_len;
        // A sequence running past the window end is truncated (invalid);
        // for a valid sequence the window provably covers it (see above).
        if seq_end > window.len() {
            return None;
        }
        let s = std::str::from_utf8(&window[seq_start..seq_end]).ok()?;
        let ch = s.chars().next()?;
        // The decoded char must actually cover `rel_pos`: in valid UTF-8 the
        // lead byte found above necessarily starts the covering sequence;
        // the range check keeps this honest for the invalid-UTF-8 leftovers
        // (e.g. a stray ASCII byte where the walk stopped).
        if rel_pos < seq_start + ch.len_utf8() {
            Some(ch)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Editing (persistent: path-copying, structural sharing)
// ---------------------------------------------------------------------------

impl BText {
    /// Insert `bytes` at byte position `pos` (clamped to `len()`).
    /// No-op for empty input. Infallible: inserts split mapped-leaf
    /// descriptors instead of reading through the pager (see `insert_at`).
    pub fn insert_bytes(&mut self, pos: usize, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let pos = pos.min(self.len());
        let nodes = insert_at(&self.root, pos, bytes);
        // A non-empty insert into a non-empty replacement list always yields
        // at least one node (leaf splits produce leaves; mapped splits keep
        // the owned middle).
        debug_assert!(!nodes.is_empty());
        self.root = if nodes.len() == 1 {
            nodes.into_iter().next().expect("insert_at: single node")
        } else {
            build_balanced(nodes)
        };
    }

    /// Delete `len` bytes at `pos`, returning the deleted bytes.
    /// Clamps to the available range; no-op (empty vec) when `pos >= len()`.
    ///
    /// The deleted bytes are materialized for the caller (undo/kill-ring),
    /// so a mapped region read can fail: the error aborts the whole delete
    /// and the tree is untouched (nothing is assigned until the new root is
    /// built — atomicity).
    pub fn delete_bytes(&mut self, pos: usize, len: usize) -> Result<Vec<u8>, PagerError> {
        let total = self.len();
        if len == 0 || pos >= total {
            return Ok(Vec::new());
        }
        let len = len.min(total - pos);
        let (root, deleted) = delete_at(&self.root, pos, len)?;
        self.root = root;
        Ok(deleted)
    }

    /// Copy `range` into a fresh `Vec`. Clamps to bounds.
    pub fn slice_bytes(&self, range: std::ops::Range<usize>) -> Result<Vec<u8>, PagerError> {
        let len = self.len();
        let start = range.start.min(len);
        let end = range.end.min(len);
        if start >= end {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(end - start);
        collect_range(&self.root, start, end, 0, &mut out)?;
        Ok(out)
    }

    /// All bytes, in order. Allocates once (use [`BText::write_to`] to stream).
    pub fn collect_bytes(&self) -> Result<Vec<u8>, PagerError> {
        let mut out = Vec::with_capacity(self.len());
        for chunk in self.chunks() {
            out.extend_from_slice(chunk?.as_bytes());
        }
        Ok(out)
    }

    /// Stream all bytes to `w` without materializing the whole text.
    /// Mapped regions stream through the pager; save never needs the whole
    /// file resident.
    pub fn write_to(&self, w: &mut dyn Write) -> Result<(), PagerError> {
        for chunk in self.chunks() {
            w.write_all(chunk?.as_bytes())
                .map_err(|source| PagerError::Io { offset: 0, source })?;
        }
        Ok(())
    }

    /// In-order leaf chunks. Mapped leaves yield one owned copy per leaf;
    /// owned leaves yield zero-copy borrows.
    pub fn chunks(&self) -> ChunkIter<'_> {
        ChunkIter::new(&self.root)
    }

    /// Lines starting at `start_line`, each including its trailing newline
    /// when present (matches the renderer's long-standing expectation).
    pub fn lines_at(&self, start_line: usize) -> LineIter<'_> {
        LineIter::new(self, start_line)
    }
}

/// A chunk yielded by [`BText::chunks`]: borrowed for owned leaves
/// (zero-copy), owned for mapped leaves (one copy per leaf through the pager).
pub enum ChunkRef<'a> {
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
}

impl<'a> ChunkRef<'a> {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            ChunkRef::Borrowed(b) => b,
            ChunkRef::Owned(v) => v,
        }
    }
}

/// In-order iterator over leaf chunks. Items are `Result` because mapped
/// leaves read through the pager, which can fail (truncation is a value, not
/// a panic — callers decide how to surface it; the render path documents its
/// choice at the call site).
pub struct ChunkIter<'a> {
    /// Stack of (node, next-child-index) for iterative DFS.
    stack: Vec<(&'a Node, usize)>,
}

impl<'a> ChunkIter<'a> {
    fn new(root: &'a Node) -> Self {
        ChunkIter { stack: vec![(root, 0)] }
    }
}

impl<'a> Iterator for ChunkIter<'a> {
    type Item = Result<ChunkRef<'a>, PagerError>;

    fn next(&mut self) -> Option<Result<ChunkRef<'a>, PagerError>> {
        loop {
            let (node, idx) = self.stack.pop()?;
            match node {
                Node::Leaf(leaf) => {
                    return Some(match leaf.read() {
                        Ok(Cow::Borrowed(b)) => Ok(ChunkRef::Borrowed(b)),
                        Ok(Cow::Owned(v)) => Ok(ChunkRef::Owned(v)),
                        Err(e) => Err(e),
                    });
                }
                Node::Internal { children, .. } => {
                    if idx < children.len() {
                        self.stack.push((node, idx + 1));
                        self.stack.push((&children[idx], 0));
                    }
                }
            }
        }
    }
}

/// One line of text: borrowed for owned leaves, owned for mapped regions.
pub enum LineRef<'a> {
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
}

impl<'a> LineRef<'a> {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            LineRef::Borrowed(b) => b,
            LineRef::Owned(v) => v,
        }
    }

    /// Lossy text view of the line. For valid UTF-8 (the common case) this
    /// borrows; invalid sequences become U+FFFD without touching the tree's
    /// byte-exact content. The borrow is tied to `&self`, not the tree: an
    /// owned (mapped-region) line cannot outlive this call.
    pub fn as_str_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(self.as_bytes())
    }
}

/// Line iterator: one `line_byte_range` per line, so each `next()` is two
/// O(log n) descents. Lazy — mapped pages materialize only for visited lines.
pub struct LineIter<'a> {
    text: &'a BText,
    next_line: usize,
}

impl<'a> LineIter<'a> {
    fn new(text: &'a BText, start_line: usize) -> Self {
        LineIter {
            text,
            next_line: start_line,
        }
    }
}

impl<'a> Iterator for LineIter<'a> {
    type Item = Result<LineRef<'a>, PagerError>;

    fn next(&mut self) -> Option<Result<LineRef<'a>, PagerError>> {
        let (start, end) = match self.text.line_byte_range(self.next_line) {
            Ok(Some(r)) => r,
            Ok(None) => return None,
            Err(e) => return Some(Err(e)),
        };
        self.next_line += 1;
        Some(self.text.line_ref(start, end))
    }
}

// ---------------------------------------------------------------------------
// Edit-path helpers
// ---------------------------------------------------------------------------

impl BText {
    /// Byte view of `[start, end)` as a [`LineRef`]: borrowed when the range
    /// lies within a single owned leaf, otherwise a single owned copy.
    /// Used by the line iterator; the general case is [`BText::slice_bytes`].
    fn line_ref(&self, start: usize, end: usize) -> Result<LineRef<'_>, PagerError> {
        debug_assert!(start <= end);
        debug_assert!(end <= self.len());
        let mut node = &self.root;
        let mut pos = start;
        loop {
            match node.as_ref() {
                Node::Leaf(leaf) => {
                    // Range-aware: a line inside a mapped region fetches only
                    // its pages, never the whole region.
                    return Ok(match leaf.read_range(pos, end - start)? {
                        Cow::Borrowed(b) => LineRef::Borrowed(b),
                        Cow::Owned(v) => LineRef::Owned(v),
                    });
                }
                Node::Internal { children, .. } => {
                    let mut acc = 0usize;
                    let mut descended = false;
                    for (i, child) in children.iter().enumerate() {
                        let cb = child.bytes();
                        let is_last = i + 1 == children.len();
                        if pos < acc + cb || (pos == acc + cb && is_last) {
                            // If the whole range fits in this child, descend;
                            // otherwise fall back to a copied slice.
                            if pos - acc + (end - start) <= cb {
                                pos -= acc;
                                node = child;
                                descended = true;
                                break;
                            }
                            return Ok(LineRef::Owned(self.slice_bytes(start..end)?));
                        }
                        acc += cb;
                    }
                    if !descended {
                        debug_assert!(false, "line_ref: descent failed");
                        return Ok(LineRef::Owned(self.slice_bytes(start..end)?));
                    }
                }
            }
        }
    }
}

/// Append `tree[start..end)` (absolute) to `out`. `base` is the absolute
/// offset of `node`.
fn collect_range(
    node: &Rc<Node>,
    start: usize,
    end: usize,
    base: usize,
    out: &mut Vec<u8>,
) -> Result<(), PagerError> {
    match node.as_ref() {
        Node::Leaf(leaf) => {
            // Range-aware: mapped leaves only fetch the touched pages.
            let lo = start.saturating_sub(base).min(leaf.bytes_len());
            let hi = end.saturating_sub(base).min(leaf.bytes_len());
            if lo < hi {
                let bytes = leaf.read_range(lo, hi - lo)?;
                out.extend_from_slice(&bytes);
            }
            Ok(())
        }
        Node::Internal { children, .. } => {
            let mut acc = base;
            for child in children {
                let cb = child.bytes();
                if acc < end && acc + cb > start {
                    collect_range(child, start, end, acc, out)?;
                }
                acc += cb;
            }
            Ok(())
        }
    }
}

/// Split an owned byte string into leaves of at most `MAX_LEAF_BYTES`.
fn chunk_leaves(mut bytes: &[u8]) -> Vec<Rc<Node>> {
    let mut out = Vec::new();
    if bytes.is_empty() {
        out.push(Rc::new(Node::Leaf(Leaf::owned_empty())));
        return out;
    }
    while !bytes.is_empty() {
        let take = bytes.len().min(MAX_LEAF_BYTES);
        out.push(Rc::new(Node::Leaf(Leaf::owned(Rc::from(
            &bytes[..take],
        )))));
        bytes = &bytes[take..];
    }
    out
}

/// One mapped leaf node with a fresh (uncached) metrics cell.
fn mapped_leaf(pager: SharedPager, offset: u64, len: u64) -> Rc<Node> {
    Rc::new(Node::Leaf(Leaf::Mapped {
        pager,
        offset,
        len,
        newlines: RefCell::new(None),
        line_index: RefCell::new(None),
    }))
}

/// Insert `bytes` at `pos` (`0 <= pos <= node.bytes()`). Returns the list of
/// nodes that replaces `node` in its parent — usually exactly one; more when
/// a split bubbles up. The input tree is never mutated (path copying).
///
/// Splits splice *flat* into the parent's child list at every level (never
/// nested): a leaf that splits yields its leaves flat, and an internal node
/// whose child list overflows returns its rebalanced groups flat for the
/// parent to adopt as siblings. Nesting a split under a new level per insert
/// would make depth grow linearly under repeated inserts at one spot (found
/// in review: the left spine wrapped one level per insert); flat bubbling
/// keeps every internal node within its arity bounds, which is what keeps
/// depth logarithmic.
///
/// Inserts never touch the pager, so insert is infallible. An insert into a
/// mapped leaf splits the leaf's *descriptor*: `[0..pos)` stays mapped, the
/// inserted bytes become owned leaf/leaves, `[pos..len)` stays mapped. This
/// avoids copying up to 64 KiB of file bytes per keystroke when typing into
/// a lazily-loaded region, and removes the failure mode a materializing
/// insert would have (a truncated-under-open file cannot break typing —
/// reads fail loudly at read time instead).
fn insert_at(node: &Rc<Node>, pos: usize, bytes: &[u8]) -> Vec<Rc<Node>> {
    match node.as_ref() {
        Node::Leaf(leaf) => match leaf {
            Leaf::Owned { bytes: rc, .. } => {
                debug_assert!(pos <= rc.len());
                let mut spliced = Vec::with_capacity(rc.len() + bytes.len());
                spliced.extend_from_slice(&rc[..pos]);
                spliced.extend_from_slice(bytes);
                spliced.extend_from_slice(&rc[pos..]);
                chunk_leaves(&spliced)
            }
            Leaf::Mapped { pager, offset, len, .. } => {
                debug_assert!(pos <= *len as usize);
                let pos = pos as u64;
                let mut split = Vec::with_capacity(3);
                if pos > 0 {
                    split.push(mapped_leaf(pager.clone(), *offset, pos));
                }
                split.extend(chunk_leaves(bytes));
                let rest = *len - pos;
                if rest > 0 {
                    split.push(mapped_leaf(pager.clone(), offset + pos, rest));
                }
                split
            }
        },
        Node::Internal { children, .. } => {
            // `pos` is clamped to `len()` at the public boundary, so this
            // lookup cannot fail. A failure would be an internal logic
            // error; it panics loudly rather than silently dropping the
            // user's insert.
            let (idx, inner) =
                find_child(children, pos).expect("insert_at: pos clamped at public boundary");
            // Fragmentation avoidance: inserting at the very start of a
            // mapped leaf when the previous sibling is a small owned leaf
            // appends to the sibling instead. Without this, typing at a
            // mapped-region boundary strands one 1-byte owned leaf per
            // keystroke; with it, the keystrokes coalesce into one owned run.
            // Byte-identical either way — the position is exactly the
            // sibling boundary.
            let (idx, inner) = match children[idx].as_ref() {
                Node::Leaf(Leaf::Mapped { .. }) if inner == 0 && idx > 0 => {
                    match children[idx - 1].as_ref() {
                        Node::Leaf(Leaf::Owned { bytes: prev, .. })
                            if prev.len() + bytes.len() <= MAX_LEAF_BYTES =>
                        {
                            (idx - 1, prev.len())
                        }
                        _ => (idx, inner),
                    }
                }
                _ => (idx, inner),
            };
            let replacement = insert_at(&children[idx], inner, bytes);
            let mut new_children =
                Vec::with_capacity(children.len() + replacement.len());
            new_children.extend_from_slice(&children[..idx]);
            new_children.extend(replacement);
            new_children.extend_from_slice(&children[idx + 1..]);
            // Coalesce adjacent small owned leaves the mapped-split may have
            // left behind (defragmentation). Mapped leaves are never merged
            // (I5). A merge that flattens to a single node replaces the
            // internal node outright (I1: no 1-child internals survive).
            let new_children = merge_adjacent_owned(new_children);
            if new_children.len() == 1 {
                new_children
            } else if new_children.len() <= BTREE_MAX_CHILDREN {
                vec![internal_of(new_children)]
            } else {
                // Overflow: return the rebalanced groups flat so the parent
                // adopts them as siblings — the split bubbles upward instead
                // of nesting. `balanced_groups` distributes the remainder so
                // no 1-child node is stranded (I1).
                balanced_groups(new_children)
            }
        }
    }
}

/// Locate the child containing byte `pos`, returning (child index, in-child
/// offset). `pos == total` addresses the end (last child).
fn find_child(children: &[Rc<Node>], pos: usize) -> Option<(usize, usize)> {
    let mut acc = 0;
    for (i, child) in children.iter().enumerate() {
        let next = acc + child.bytes();
        if pos < next || (pos == next && i + 1 == children.len()) {
            return Some((i, pos - acc));
        }
        acc = next;
    }
    None
}

/// Delete `[pos, pos+len)` (`len > 0`, in range). Returns the new subtree
/// root and the deleted bytes. Atomic like [`insert_at`].
fn delete_at(node: &Rc<Node>, pos: usize, len: usize) -> Result<(Rc<Node>, Vec<u8>), PagerError> {
    match node.as_ref() {
        Node::Leaf(leaf) => {
            let owned = leaf.read()?.into_owned();
            debug_assert!(pos + len <= owned.len());
            let mut deleted = Vec::with_capacity(len);
            deleted.extend_from_slice(&owned[pos..pos + len]);
            let mut kept = Vec::with_capacity(owned.len() - len);
            kept.extend_from_slice(&owned[..pos]);
            kept.extend_from_slice(&owned[pos + len..]);
            let leaves = chunk_leaves(&kept);
            let new_node = if leaves.len() == 1 {
                leaves.into_iter().next().expect("chunk_leaves: non-empty")
            } else {
                build_balanced(leaves)
            };
            Ok((new_node, deleted))
        }
        Node::Internal { children, .. } => {
            let end = pos + len;
            let mut new_children: Vec<Rc<Node>> = Vec::with_capacity(children.len());
            let mut deleted = Vec::new();
            let mut acc = 0usize;
            for child in children {
                let cb = child.bytes();
                let c_start = acc;
                let c_end = acc + cb;
                if c_end <= pos || c_start >= end {
                    // Untouched: share the subtree (zero copy).
                    new_children.push(Rc::clone(child));
                } else {
                    // Overlaps the delete range: recurse.
                    let d_start = pos.saturating_sub(c_start);
                    let d_end = (end - c_start).min(cb);
                    let (new_child, mut d) = delete_at(child, d_start, d_end - d_start)?;
                    deleted.append(&mut d);
                    if new_child.bytes() > 0 {
                        new_children.push(new_child);
                    }
                    // else: child fully deleted — drop it.
                }
                acc = c_end;
            }
            Ok((collapse_and_merge(new_children), deleted))
        }
    }
}

/// Merge adjacent owned leaves whose combined size fits (defragmentation).
/// One level only: callers run it on a freshly spliced child list, never on
/// a whole subtree. Keeps the child list short after a mapped-split leaves a
/// small owned insert between owned neighbors. Mapped leaves are never
/// merged (I5 — they stay immutable originals). Pure: no pager reads.
fn merge_adjacent_owned(children: Vec<Rc<Node>>) -> Vec<Rc<Node>> {
    let mut merged: Vec<Rc<Node>> = Vec::with_capacity(children.len());
    for child in children {
        let coalesced = match (merged.last(), child.as_ref()) {
            (Some(last), Node::Leaf(Leaf::Owned { bytes: cur, .. })) => match last.as_ref() {
                Node::Leaf(Leaf::Owned { bytes: prev, .. })
                    if prev.len() + cur.len() <= MAX_LEAF_BYTES =>
                {
                    let mut v = Vec::with_capacity(prev.len() + cur.len());
                    v.extend_from_slice(prev);
                    v.extend_from_slice(cur);
                    Some(Rc::new(Node::Leaf(Leaf::owned(Rc::from(&v[..])))))
                }
                _ => None,
            },
            _ => None,
        };
        match coalesced {
            Some(m) => {
                merged.pop();
                merged.push(m);
            }
            None => merged.push(child),
        }
    }
    merged
}

/// Rebuild an internal level after deletion: drop empties (already done by
/// the caller), collapse single-child levels, and merge underfull internal
/// nodes with the next sibling when it fits. The classic minimum fan-out is
/// restored best-effort only; the hard guarantee is I1's arity-2 lower bound
/// (no 1-child internal nodes survive), which is what bounds depth.
fn collapse_and_merge(children: Vec<Rc<Node>>) -> Rc<Node> {
    if children.is_empty() {
        return Rc::new(Node::Leaf(Leaf::owned_empty()));
    }
    if children.len() == 1 {
        return children.into_iter().next().expect("collapse: non-empty");
    }
    // Steal/merge pass to restore the minimum fan-out (I1).
    let mut out: Vec<Rc<Node>> = Vec::with_capacity(children.len());
    let mut i = 0;
    while i < children.len() {
        match children[i].as_ref() {
            Node::Internal {
                children: grand, ..
            } if grand.len() < BTREE_MIN_CHILDREN => {
                // Underfull: merge with the next sibling when possible.
                if i + 1 < children.len()
                    && let Node::Internal {
                        children: next_grand,
                        ..
                    } = children[i + 1].as_ref()
                    && grand.len() + next_grand.len() <= BTREE_MAX_CHILDREN
                {
                    let mut merged = grand.clone();
                    merged.extend(next_grand.iter().cloned());
                    out.push(internal_of(merged));
                    i += 2;
                    continue;
                }
                // Cannot merge (would overflow, or no next sibling):
                // keep as-is. Underfull nodes only arise from deletions and
                // never gain children except via the merges above, so depth
                // stays within the I1 arity-2 bound.
                out.push(Rc::clone(&children[i]));
                i += 1;
            }
            _ => {
                out.push(Rc::clone(&children[i]));
                i += 1;
            }
        }
    }
    // A single node needs no wrapper: `out` holds exactly one element only
    // when the whole level merged into it, and wrapping it would create a
    // 1-child internal node, which I1 forbids below the root. The merged
    // node already carries 2..=BTREE_MAX_CHILDREN children by construction.
    if out.len() == 1 {
        return out.into_iter().next().expect("collapse: merged");
    }
    internal_of(out)
}

// ---------------------------------------------------------------------------
// Position mapping (decision D: pure functions, no anchor objects in the tree)
// ---------------------------------------------------------------------------

/// Which side of an insertion a position exactly at the edit point sticks to.
/// Cursors are typically [`Sticky::After`] (typing moves the cursor past the
/// typed text); marks are typically [`Sticky::Before`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sticky {
    Before,
    After,
}

/// Map a byte position through a single edit, as a pure function.
///
/// `edit_pos`/`removed_len`/`inserted_len` describe the edit; positions after
/// the edit shift by the delta; positions inside the removed range clamp to
/// the edit point (start of the inserted text when [`Sticky::Before`], end
/// when [`Sticky::After`]).
///
/// This is the canonical primitive for keeping cursors/marks/selections
/// coherent across edits — decision D: pure functions, no anchor objects in
/// the tree. Laws (proven V-03): monotonicity, identity, and composition for
/// non-overlapping edits.
pub fn map_pos_through_edit(
    pos: usize,
    edit_pos: usize,
    removed_len: usize,
    inserted_len: usize,
    sticky: Sticky,
) -> usize {
    if pos < edit_pos || (pos == edit_pos && sticky == Sticky::Before) {
        // Strictly before the edit, or exactly at it and sticking left.
        pos
    } else if pos < edit_pos + removed_len {
        // Inside the removed range: the text this pointed at is gone; clamp
        // to the edit point.
        match sticky {
            Sticky::Before => edit_pos,
            Sticky::After => edit_pos + inserted_len,
        }
    } else {
        // At or past the end of the removed range: shift by the delta.
        // `removed_len` is clamped so a nonsensical (over-long) removal
        // saturates instead of underflowing.
        let removed = removed_len.min(pos.saturating_sub(edit_pos));
        pos.saturating_sub(removed).saturating_add(inserted_len)
    }
}

// ---------------------------------------------------------------------------
// Tests: unit, differential model, Tier 1 exhaustive proofs, fuzz
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Small deterministic RNG (xorshift64*; no new dependencies) --------

    struct Rng(u64);

    impl Rng {
        fn new(seed: u64) -> Self {
            Rng(seed | 1)
        }
        fn next_u64(&mut self) -> u64 {
            // xorshift64*
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545F4914F6CDD1D)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next_u64() % (n as u64).max(1)) as usize
        }
        fn byte(&mut self) -> u8 {
            // Biased toward interesting bytes: newlines, UTF-8 lead/
            // continuation bytes, NUL, 0xFF.
            match self.below(16) {
                0 => b'\n',
                1 => 0x00,
                2 => 0xFF,
                3 => 0xC3,
                4 => 0xA9,
                5 => 0xE2,
                6 => 0x82,
                7 => b' ',
                _ => b'a' + self.below(26) as u8,
            }
        }
    }

    /// Naive model: the ground truth every BText op is checked against.
    #[derive(Debug, Clone, Default)]
    struct Model {
        bytes: Vec<u8>,
    }

    impl Model {
        fn insert(&mut self, pos: usize, bytes: &[u8]) {
            let pos = pos.min(self.bytes.len());
            self.bytes.splice(pos..pos, bytes.iter().copied());
        }
        fn delete(&mut self, pos: usize, len: usize) -> Vec<u8> {
            let len = len.min(self.bytes.len().saturating_sub(pos.min(self.bytes.len())));
            let pos = pos.min(self.bytes.len());
            self.bytes.drain(pos..pos + len).collect()
        }
    }

    /// Assert full observable equivalence between tree and model.
    fn assert_equiv(t: &BText, m: &Model) {
        let tb = t.collect_bytes().expect("collect_bytes");
        assert_eq!(tb, m.bytes, "byte content diverged");
        assert_eq!(t.len(), m.bytes.len(), "len() diverged");
        let t_nl = t.total_newlines().expect("total_newlines");
        let m_nl = m.bytes.iter().filter(|&&b| b == b'\n').count();
        assert_eq!(t_nl, m_nl, "newline count diverged");
        assert_eq!(
            t.line_count().expect("line_count"),
            m_nl + 1,
            "line_count diverged"
        );
        // Spot-check line mapping both directions on a few lines.
        let lines = m_nl + 1;
        for li in [0, lines / 2, lines.saturating_sub(1)] {
            let (s, e) = t
                .line_byte_range(li)
                .expect("line_byte_range")
                .expect("line in range");
            // Recompute from the model.
            let mut start = 0;
            for _ in 0..li {
                start = m.bytes[start..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map(|p| start + p + 1)
                    .unwrap_or(m.bytes.len());
            }
            let end = m.bytes[start..]
                .iter()
                .position(|&b| b == b'\n')
                .map(|p| start + p + 1)
                .unwrap_or(m.bytes.len());
            assert_eq!((s, e), (start, end), "line_byte_range({li}) diverged");
            assert_eq!(
                t.byte_to_line(s).expect("byte_to_line"),
                li,
                "byte_to_line({s}) diverged"
            );
        }
    }

    #[test]
    fn empty_tree_basics() {
        let t = BText::new();
        assert_eq!(t.len(), 0);
        assert!(t.is_empty());
        assert_eq!(t.line_count().unwrap(), 1);
        assert_eq!(t.line_to_byte(0).unwrap(), Some(0));
        assert_eq!(t.line_to_byte(1).unwrap(), None);
        assert_eq!(t.byte_to_line(0).unwrap(), 0);
        assert_eq!(t.char_at(0), None);
        assert_eq!(t.slice_bytes(0..10).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn from_bytes_round_trip() {
        let data = b"hello\nworld\n";
        let t = BText::from_bytes(data);
        assert_eq!(t.len(), data.len());
        assert_eq!(t.collect_bytes().unwrap(), data);
        assert_eq!(t.line_count().unwrap(), 3);
        assert_eq!(t.line_to_byte(1).unwrap(), Some(6));
        assert_eq!(t.byte_to_line(7).unwrap(), 1);
        assert_eq!(t.char_at(1), Some('e'));
    }

    #[test]
    fn char_at_multibyte_and_boundaries() {
        // "a" + "é"(2 bytes) + "😀"(4 bytes) + "b"
        let data = "aé😀b".as_bytes();
        let t = BText::from_bytes(data);
        assert_eq!(t.char_at(0), Some('a'));
        assert_eq!(t.char_at(1), Some('é'));
        assert_eq!(t.char_at(2), Some('é')); // continuation byte
        assert_eq!(t.char_at(3), Some('😀'));
        assert_eq!(t.char_at(6), Some('😀')); // last byte of emoji
        assert_eq!(t.char_at(7), Some('b'));
        assert_eq!(t.char_at(8), None);
        // Invalid UTF-8: lone continuation byte.
        let t2 = BText::from_bytes(b"a\x80b");
        assert_eq!(t2.char_at(0), Some('a'));
        assert_eq!(t2.char_at(1), None);
        assert_eq!(t2.char_at(2), Some('b'));
    }

    // -- V-01: exhaustive bounded proof — edits ≡ Vec<u8> model ------------
    //
    // BOUND (stated per the honesty rule): all op sequences of length ≤ 5
    // over an 8-op alphabet (insert/delete × {start,end} × {"x","yz"}/{1,3}),
    // plus all sequences of length ≤ 3 over a 12-op alphabet that also covers
    // mid-buffer positions. Every sequence is checked for full observable
    // equivalence (bytes, len, newline/line counts, line mappings both ways).
    // V-02 (undo algebra) is checked in the same loop: after each sequence,
    // applying the recorded inverses in reverse must restore the original.

    #[derive(Debug, Clone)]
    enum Op {
        Ins { at_start: bool, text: &'static [u8] },
        Del { at_start: bool, len: usize },
        AbsDel { pos: usize, len: usize },
        AbsIns { pos: usize, text: Vec<u8> },
    }

    fn apply_op(t: &mut BText, m: &mut Model, op: Op, undo_log: &mut Vec<Op>) {
        match op {
            Op::Ins { at_start, text } => {
                let pos = if at_start { 0 } else { m.bytes.len() };
                t.insert_bytes(pos, text);
                m.insert(pos, text);
                // Record the inverse precisely (absolute position).
                undo_log.push(Op::AbsDel { pos, len: text.len() });
            }
            Op::Del { at_start, len } => {
                let pos = if at_start { 0 } else { m.bytes.len().saturating_sub(len) };
                let deleted_m = m.delete(pos, len);
                let deleted_t = t.delete_bytes(pos, len).expect("in-memory delete");
                assert_eq!(deleted_t, deleted_m, "delete_bytes return diverged");
                undo_log.push(Op::AbsIns {
                    pos,
                    text: deleted_m,
                });
            }
            _ => unreachable!("apply_op: only Ins/Del"),
        }
    }

    #[test]
    fn v01_v02_exhaustive_edits_and_undo() {
        // Extended op enum for absolute inverses.
        // (Defined locally to keep the alphabet small in the enumeration.)
        let alphabet_8 = [
            Op::Ins { at_start: true, text: b"x" },
            Op::Ins { at_start: true, text: b"yz" },
            Op::Ins { at_start: false, text: b"x" },
            Op::Ins { at_start: false, text: b"yz" },
            Op::Del { at_start: true, len: 1 },
            Op::Del { at_start: true, len: 3 },
            Op::Del { at_start: false, len: 1 },
            Op::Del { at_start: false, len: 3 },
        ];
        let mut cases = 0u64;
        // Lengths 1..=5 over 8 ops.
        let mut seq: Vec<usize> = Vec::new();
        fn run_seq(alphabet: &[Op], seq: &[usize], cases: &mut u64) {
            let mut t = BText::from_bytes(b"seed\ncontent\n");
            let mut m = Model { bytes: b"seed\ncontent\n".to_vec() };
            let mut undo_log: Vec<Op> = Vec::new();
            for &oi in seq {
                apply_op(&mut t, &mut m, alphabet[oi].clone(), &mut undo_log);
            }
            assert_equiv(&t, &m);
            // V-02: undo-all must restore the original byte-exact.
            for inv in undo_log.iter().rev() {
                match inv {
                    Op::AbsDel { pos, len } => {
                        let dt = t.delete_bytes(*pos, *len).expect("in-memory undo delete");
                        let dm = m.delete(*pos, *len);
                        assert_eq!(dt, dm);
                    }
                    Op::AbsIns { pos, text } => {
                        t.insert_bytes(*pos, text);
                        m.insert(*pos, text);
                    }
                    _ => unreachable!(),
                }
            }
            assert_equiv(&t, &m);
            assert_eq!(t.collect_bytes().unwrap(), b"seed\ncontent\n");
            *cases += 1;
        }
        // Iterative deepening enumeration.
        fn enumerate(alphabet: &[Op], depth: usize, seq: &mut Vec<usize>, cases: &mut u64) {
            if depth == 0 {
                run_seq(alphabet, seq, cases);
                return;
            }
            for oi in 0..alphabet.len() {
                seq.push(oi);
                enumerate(alphabet, depth - 1, seq, cases);
                seq.pop();
            }
        }
        for len in 1..=5 {
            enumerate(&alphabet_8, len, &mut seq, &mut cases);
        }
        // 8 + 64 + 512 + 4096 + 32768 = 37448 cases.
        assert_eq!(cases, 37448, "enumeration must cover the full bound");
    }

    // -- Structural invariant checker (I1/I2/I4 + depth bound) -------------
    //
    // Used by the fuzz loop after every op. Returns the violation as a
    // string so the fuzz harness can persist the reproducer before failing.

    fn check_tree_invariants(t: &BText) -> Result<(), String> {
        fn walk(
            node: &Node,
            is_root: bool,
            depth: usize,
            max_depth: &mut usize,
            file_len: u64,
        ) -> Result<(), String> {
            *max_depth = (*max_depth).max(depth);
            match node {
                Node::Leaf(leaf) => {
                    if let Leaf::Owned { bytes: rc, .. } = leaf
                        && rc.len() > MAX_LEAF_BYTES
                    {
                        return Err(format!(
                            "I4 violated: owned leaf of {} bytes exceeds MAX_LEAF_BYTES={}",
                            rc.len(),
                            MAX_LEAF_BYTES
                        ));
                    }
                    // Mapped leaves must stay inside the file they map.
                    if let Leaf::Mapped { offset, len, .. } = leaf
                        && offset + len > file_len
                    {
                        return Err(format!(
                            "I5 violated: mapped [{offset}, {offset}+{len}) exceeds file len {file_len}"
                        ));
                    }
                    Ok(())
                }
                Node::Internal { children, bytes, .. } => {
                    let n = children.len();
                    if is_root {
                        if !(1..=BTREE_MAX_CHILDREN).contains(&n) {
                            return Err(format!("I1 violated: root has {n} children"));
                        }
                    } else if !(2..=BTREE_MAX_CHILDREN).contains(&n) {
                        return Err(format!("I1 violated: internal node has {n} children"));
                    }
                    let sum: usize = children.iter().map(|c| c.bytes()).sum();
                    if *bytes != sum {
                        return Err(format!(
                            "I2 violated: node bytes={bytes} != child sum={sum}"
                        ));
                    }
                    for c in children {
                        walk(c, false, depth + 1, max_depth, file_len)?;
                    }
                    Ok(())
                }
            }
        }
        let mut max_depth = 0usize;
        walk(&t.root, true, 0, &mut max_depth, t.len() as u64)?;
        // Every internal node has >= 2 children (I1), so depth is O(log n);
        // the absolute cap catches degenerate chains (e.g. a nesting bug).
        if max_depth > 32 {
            return Err(format!("depth bound violated: depth {max_depth}"));
        }
        Ok(())
    }

    /// Stronger observable-equivalence check than `assert_equiv`: full
    /// line-map verification in both directions plus a `char_at` oracle on
    /// a sample of positions. Returns the violation for corpus persistence.
    fn check_equiv_full(t: &BText, m: &Model, rng: &mut Rng) -> Result<(), String> {
        let tb = t
            .collect_bytes()
            .map_err(|e| format!("collect_bytes failed: {e}"))?;
        if tb != m.bytes {
            return Err(format!(
                "byte content diverged: tree {} bytes, model {} bytes",
                tb.len(),
                m.bytes.len()
            ));
        }
        if t.len() != m.bytes.len() {
            return Err("len() diverged".to_string());
        }
        let t_nl = t
            .total_newlines()
            .map_err(|e| format!("total_newlines failed: {e}"))?;
        let m_nl = m.bytes.iter().filter(|&&b| b == b'\n').count();
        if t_nl != m_nl {
            return Err(format!("newline count diverged: {t_nl} vs {m_nl}"));
        }
        let lc = t.line_count().map_err(|e| format!("line_count failed: {e}"))?;
        if lc != m_nl + 1 {
            return Err(format!("line_count diverged: {lc} vs {}", m_nl + 1));
        }
        // Model line starts, computed in a single O(n) pass so the per-line
        // checks below stay O(1) on the model side.
        let mut starts: Vec<usize> = Vec::with_capacity(lc);
        starts.push(0);
        for (i, &b) in m.bytes.iter().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        assert_eq!(starts.len(), lc, "model line-start miscount");
        // Every line, both directions.
        for li in 0..lc {
            let start = starts[li];
            let end = if li + 1 < lc {
                starts[li + 1]
            } else {
                m.bytes.len()
            };
            let (s, e) = t
                .line_byte_range(li)
                .map_err(|e| format!("line_byte_range({li}) failed: {e}"))?
                .ok_or_else(|| format!("line_byte_range({li}) returned None"))?;
            if (s, e) != (start, end) {
                return Err(format!(
                    "line_byte_range({li}) = ({s},{e}), model = ({start},{end})"
                ));
            }
            let l = t
                .byte_to_line(s)
                .map_err(|e| format!("byte_to_line({s}) failed: {e}"))?;
            if l != li {
                return Err(format!("byte_to_line({s}) = {l}, want {li}"));
            }
            // line_to_byte agrees too.
            let ltb = t
                .line_to_byte(li)
                .map_err(|e| format!("line_to_byte({li}) failed: {e}"))?;
            if ltb != Some(start) {
                return Err(format!("line_to_byte({li}) = {ltb:?}, want {start}"));
            }
        }
        // char_at oracle on a sample of positions (independent naive
        // implementation: walk back to the lead byte, decode, range-check).
        let n = m.bytes.len();
        for _ in 0..4 {
            let pos = if n == 0 { 0 } else { rng.below(n + 1) };
            let want = oracle_char_at(&m.bytes, pos);
            let got = t.char_at(pos);
            if got != want {
                return Err(format!("char_at({pos}) = {got:?}, oracle = {want:?}"));
            }
        }
        Ok(())
    }

    /// Naive independent oracle for `char_at`: walk back at most 3 bytes to
    /// the lead byte, then decode exactly the one sequence the lead byte
    /// announces. Deliberately written straight from the UTF-8 spec, not
    /// from the implementation's structure. (Decoding to end-of-buffer
    /// would let an invalid byte *after* the character poison the result;
    /// the character containing `pos` depends only on its own sequence.)
    ///
    /// The lead-byte length table is shared spec knowledge with `char_at`
    /// (see `utf8_lead_len`); what this oracle checks independently is the
    /// windowing: lead-byte location across leaf boundaries, the walk-back
    /// cap, truncation at buffer end, and the coverage range check.
    fn oracle_char_at(bytes: &[u8], pos: usize) -> Option<char> {
        if pos >= bytes.len() {
            return None;
        }
        let mut start = pos;
        while start > 0 && pos - start < 3 && (bytes[start] & 0xC0) == 0x80 {
            start -= 1;
        }
        let seq_len = utf8_lead_len(bytes[start])?;
        let end = start + seq_len;
        if end > bytes.len() {
            return None;
        }
        let s = std::str::from_utf8(&bytes[start..end]).ok()?;
        let ch = s.chars().next()?;
        if pos < start + ch.len_utf8() {
            Some(ch)
        } else {
            None
        }
    }

    #[test]
    fn depth_stays_logarithmic_under_repeated_large_inserts() {
        // Regression pin for a real bug found in review: `insert_at` used to
        // wrap a split leaf's children in `build_balanced`, nesting one new
        // level per large insert at the same spot — linear depth. Leaf
        // splits must splice flat into the parent's child list instead.
        let mut t = BText::from_bytes(b"seed");
        let big = vec![b'x'; MAX_LEAF_BYTES * 4 + 7];
        for _ in 0..50 {
            t.insert_bytes(0, &big);
        }
        let mut max_depth = 0usize;
        fn depth(n: &Node, d: usize, max: &mut usize) {
            *max = (*max).max(d);
            if let Node::Internal { children, .. } = n {
                for c in children {
                    depth(c, d + 1, max);
                }
            }
        }
        depth(&t.root, 0, &mut max_depth);
        // 50 x ~256 KiB ~= 12.8 MiB; a healthy arity-32 tree is ~3 deep.
        // Anything above 6 means the nesting bug is back.
        assert!(
            max_depth <= 6,
            "depth blew up to {max_depth} under repeated large inserts"
        );
        check_tree_invariants(&t).expect("invariants");
        assert_eq!(t.len(), 4 + 50 * big.len());
        // Content is exactly 50 copies of `big` followed by "seed".
        let tb = t.collect_bytes().expect("collect");
        assert_eq!(&tb[..big.len()], &big[..]);
        assert_eq!(&tb[tb.len() - 4..], b"seed");
    }

    // -- G-series: collapse_and_merge single-child regression ----------------
    //
    // When a level shrinks to exactly two underfull internal siblings that
    // fit together, the merge pass produces one node; that node must be
    // returned directly, not wrapped in a new 1-child internal node (which
    // would violate the non-root arity-2 lower bound, I1).
    //
    // The first test drives the merge path deterministically: two 10-child
    // internal nodes (underfull: < BTREE_MIN_CHILDREN == 16) merge into a
    // 20-child node (<= BTREE_MAX_CHILDREN == 32). The second test exercises
    // the path end to end with seeded random deletes on a 3-level tree.

    #[test]
    fn g001_collapse_merge_yields_no_single_child_internal() {
        fn underfull_internal(kids: usize) -> Rc<Node> {
            let leaves: Vec<Rc<Node>> = (0..kids)
                .map(|_| Rc::new(Node::Leaf(Leaf::owned(Rc::from(*b"x")))))
                .collect();
            internal_of(leaves)
        }
        let merged = collapse_and_merge(vec![underfull_internal(10), underfull_internal(10)]);
        match merged.as_ref() {
            Node::Internal { children, .. } => {
                assert!(
                    children.len() != 1,
                    "collapse_and_merge produced a 1-child internal node (I1 violated)"
                );
                assert_eq!(
                    children.len(),
                    20,
                    "expected the two underfull siblings merged into 20 grandchildren"
                );
            }
            Node::Leaf(_) => panic!("merging two internal nodes must stay internal"),
        }
    }

    #[test]
    fn g002_deep_tree_deletes_hold_invariants() {
        // 3 levels: 33*33 leaves of 8 KiB ~= 8.7 MiB, seeded content.
        let mut rng = Rng::new(0xB7EC5);
        let mut data = Vec::with_capacity(33 * 33 * 8192);
        for _ in 0..33 * 33 * 8192 {
            data.push(rng.byte());
        }
        let mut t = BText::from_bytes(&data);
        check_tree_invariants(&t).expect("invariants after build");
        // Seeded random deletes shrink levels toward the merge path.
        for i in 0..200usize {
            let len = t.len();
            if len == 0 {
                break;
            }
            let pos = rng.below(len);
            let dlen = rng.below(len - pos).clamp(1, 1 << 16);
            t.delete_bytes(pos, dlen).expect("in-memory delete");
            if i % 10 == 0 {
                check_tree_invariants(&t).expect("invariants under seeded deletes");
            }
        }
        check_tree_invariants(&t).expect("invariants after seeded deletes");
        assert_eq!(t.len(), t.collect_bytes().expect("collect").len());
    }

    /// Deterministic end-to-end reachability: a mid-level merge whose result
    /// cannot merge upward (blocked by a full sibling) must not leave a
    /// 1-child internal node in the tree. Drives the real `delete_at` path:
    /// X = [a1, a2] (two underfull internal siblings) merges to one node,
    /// and the root's merge pass keeps it because the next sibling B is full
    /// (1 + 32 > BTREE_MAX_CHILDREN).
    #[test]
    fn g003_mid_level_merge_checked_by_invariants() {
        fn leaf() -> Rc<Node> {
            Rc::new(Node::Leaf(Leaf::owned(Rc::from(*b"x"))))
        }
        fn internal(kids: usize) -> Rc<Node> {
            internal_of((0..kids).map(|_| leaf()).collect())
        }
        let x = internal_of(vec![internal(10), internal(10)]);
        let b = internal(32);
        let total = x.bytes() + b.bytes();
        let mut t = BText {
            root: internal_of(vec![x, b]),
        };
        // One byte inside a1: X's children stay two underfull internals and
        // merge; X's merged result cannot merge upward past the full B.
        t.delete_bytes(0, 1).expect("in-memory delete");
        check_tree_invariants(&t).expect("no 1-child internal node may survive");
        assert_eq!(t.len(), total - 1);
        assert!(t.collect_bytes().expect("collect").iter().all(|&c| c == b'x'));
    }

    // -- V-03: position-mapping laws (decision D) ---------------------------
    //
    // BOUND (stated per the honesty rule): exhaustive over edit_pos 0..6,
    // removed_len 0..5, inserted_len 0..5, pos 0..10, both stickies
    // (6*5*5*10*2 = 3000 pointwise cases) plus monotonicity over all
    // position pairs per parameter set. Each law is stated independently of
    // the implementation — as the plain reading of the documented spec —
    // and the marker oracle below checks the semantics against a Vec<u8>
    // model instead of against formulas at all.

    #[test]
    fn v03_map_pos_laws_exhaustive() {
        let mut cases = 0u64;
        for edit_pos in 0..6usize {
            for removed_len in 0..5usize {
                for inserted_len in 0..5usize {
                    for &sticky in &[Sticky::Before, Sticky::After] {
                        let f = |pos: usize| {
                            map_pos_through_edit(pos, edit_pos, removed_len, inserted_len, sticky)
                        };
                        for pos in 0..10usize {
                            // L1 identity: a no-op edit moves nothing.
                            if removed_len == 0 && inserted_len == 0 {
                                assert_eq!(f(pos), pos, "L1 identity failed");
                            }
                            // L2 before-region: positions strictly before
                            // the edit are fixed.
                            if pos < edit_pos {
                                assert_eq!(f(pos), pos, "L2 before-region failed");
                            }
                            // L3 past-region: past the removed range, the
                            // position shifts by exactly the delta. (The
                            // pos == edit_pos + Before case belongs to L4:
                            // branch order in the implementation.)
                            if pos >= edit_pos + removed_len
                                && !(pos == edit_pos && sticky == Sticky::Before)
                            {
                                assert_eq!(
                                    f(pos),
                                    pos - removed_len + inserted_len,
                                    "L3 delta failed"
                                );
                            }
                            // L4 insertion-point stickiness (pure insert):
                            // Before stays, After moves past the new text.
                            if removed_len == 0 && pos == edit_pos {
                                let want = match sticky {
                                    Sticky::Before => edit_pos,
                                    Sticky::After => edit_pos + inserted_len,
                                };
                                assert_eq!(f(pos), want, "L4 stickiness failed");
                            }
                            // L5 removed-range clamp: positions inside the
                            // removed span collapse to the edit point. At
                            // the boundary, After-sticky positions are
                            // inside the removed span too.
                            if (pos > edit_pos && pos < edit_pos + removed_len)
                                || (pos == edit_pos
                                    && sticky == Sticky::After
                                    && removed_len > 0)
                            {
                                let want = match sticky {
                                    Sticky::Before => edit_pos,
                                    Sticky::After => edit_pos + inserted_len,
                                };
                                assert_eq!(f(pos), want, "L5 clamp failed");
                            }
                            cases += 1;
                        }
                        // L6 monotonicity: order is preserved.
                        let mut prev = f(0);
                        for pos in 1..10usize {
                            let cur = f(pos);
                            assert!(prev <= cur, "L6 monotonicity failed");
                            prev = cur;
                        }
                        // L7 insert/delete round-trip: inserting k bytes at
                        // p and deleting k at p restores every position
                        // (away from p outright; at p for matched stickies).
                        if removed_len == 0 && inserted_len > 0 {
                            let k = inserted_len;
                            for npos in 0..10usize {
                                if npos == edit_pos {
                                    continue;
                                }
                                let a = f(npos);
                                let b = map_pos_through_edit(a, edit_pos, k, 0, sticky);
                                assert_eq!(b, npos, "L7 round-trip failed");
                            }
                            let a = f(edit_pos);
                            let b = map_pos_through_edit(a, edit_pos, k, 0, sticky);
                            assert_eq!(b, edit_pos, "L7 boundary round-trip failed");
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 6 * 5 * 5 * 10 * 2, "law grid must be fully covered");
    }

    #[test]
    fn v03_map_pos_marker_oracle() {
        // Independent semantic oracle: embed unique 2-byte markers in a
        // Vec<u8> model, apply a random edit to the model, and require
        // map_pos_through_edit to track every surviving marker to its new
        // byte offset. This checks what position mapping MEANS, not a
        // restatement of the formula.
        let mut rng = Rng::new(0xC0FFEE123456789);
        for trial in 0..3000u32 {
            // Model with unique markers (0xFE never appears elsewhere).
            let n_markers = 1 + rng.below(6);
            let mut model: Vec<u8> = Vec::new();
            let mut marker_pos: Vec<usize> = Vec::new();
            for m in 0..n_markers {
                for _ in 0..rng.below(8) {
                    let b = rng.byte();
                    model.push(if b == 0xFE { 0xFD } else { b });
                }
                marker_pos.push(model.len());
                model.push(0xFE);
                model.push(m as u8);
            }
            let tail = rng.below(8);
            model.extend(std::iter::repeat_n(b'z', tail));
            let edit_pos = rng.below(model.len() + 1);
            let removed = rng.below(6).min(model.len().saturating_sub(edit_pos));
            let ins_len = rng.below(6);
            let mut inserted = Vec::with_capacity(ins_len);
            for _ in 0..ins_len {
                let b = rng.byte();
                inserted.push(if b == 0xFE { 0xFD } else { b });
            }
            let mut new_model = model.clone();
            new_model.splice(
                edit_pos..edit_pos + removed,
                inserted.iter().copied(),
            );
            for (m, &old) in marker_pos.iter().enumerate() {
                // Classification is about the marker's START position: was
                // the byte it pointed at spliced out? (A zero-size edit, or
                // an edit starting inside the marker's second byte, leaves
                // the start position live.)
                let start_removed = old >= edit_pos && old < edit_pos + removed;
                for &sticky in &[Sticky::Before, Sticky::After] {
                    let new_pos =
                        map_pos_through_edit(old, edit_pos, removed, ins_len, sticky);
                    if !start_removed {
                        // Boundary subtlety: a position exactly at the edit
                        // point with Sticky::Before sticks to the LEFT of
                        // inserted text — the marker moved right, so the
                        // position no longer points at it. It must point at
                        // the inserted text instead (grounded in the model).
                        if old == edit_pos && removed == 0 && sticky == Sticky::Before && ins_len > 0 {
                            assert_eq!(new_pos, edit_pos, "trial {trial}: boundary Before");
                            assert!(
                                new_model[new_pos..].starts_with(&inserted),
                                "trial {trial}: boundary Before must point at inserted text"
                            );
                            continue;
                        }
                        let want = if old < edit_pos {
                            old
                        } else if old == edit_pos {
                            // removed == 0 here (else start_removed); Before
                            // with ins_len > 0 handled above.
                            match sticky {
                                Sticky::Before => edit_pos,
                                Sticky::After => edit_pos + ins_len,
                            }
                        } else {
                            old - removed + ins_len
                        };
                        assert_eq!(
                            new_pos, want,
                            "trial {trial}: marker {m} tracking diverged"
                        );
                        assert_eq!(
                            new_model[new_pos], 0xFE,
                            "trial {trial}: marker {m} first byte not at mapped position"
                        );
                        // The second marker byte survives only if the edit
                        // didn't touch the marker's range at all.
                        if edit_pos + removed <= old || edit_pos >= old + 2 {
                            assert_eq!(
                                new_model[new_pos + 1],
                                m as u8,
                                "trial {trial}: marker {m} second byte diverged"
                            );
                        }
                        continue;
                    }
                    // Consumed marker: the clamp law, grounded in the model.
                    let want = match sticky {
                        Sticky::Before => edit_pos,
                        Sticky::After => edit_pos + ins_len,
                    };
                    assert_eq!(
                        new_pos, want,
                        "trial {trial}: consumed marker {m} clamp"
                    );
                    if sticky == Sticky::Before && ins_len > 0 {
                        assert!(
                            new_model[want..].starts_with(&inserted),
                            "trial {trial}: Before-clamp must point at inserted text"
                        );
                    }
                    if sticky == Sticky::After {
                        // After-clamp points at whatever followed the
                        // removed range in the old model.
                        let follower = &model[edit_pos + removed..];
                        assert!(
                            new_model[want..].starts_with(follower),
                            "trial {trial}: After-clamp must point at the follower"
                        );
                    }
                }
            }
        }
    }

    // -- V-12: pager coherence + truncation ---------------------------------

    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("erax-btext-{tag}-{}", std::process::id()))
    }

    fn count_leaves(node: &Rc<Node>) -> usize {
        match node.as_ref() {
            Node::Leaf(_) => 1,
            Node::Internal { children, .. } => children.iter().map(count_leaves).sum(),
        }
    }

    /// Typing keystroke-by-keystroke at a mapped-region boundary must not
    /// strand one leaf per keystroke (fragmentation bug found in review).
    /// The boundary redirect coalesces the keystrokes into one owned run.
    #[test]
    fn typing_at_mapped_boundary_does_not_fragment() {
        let mut data = vec![b'a'; MAP_REGION_BYTES as usize];
        data.extend_from_slice(&vec![b'b'; 1000]);
        let path = temp_path("frag");
        std::fs::write(&path, &data).expect("write temp file");
        let mut t = BText::open_mapped(&path).expect("open_mapped");
        assert_eq!(count_leaves(&t.root), 2, "two mapped regions");

        let boundary = MAP_REGION_BYTES as usize;
        let mut model = data.clone();
        for i in 0..500usize {
            let ch = [b'0' + (i % 10) as u8];
            t.insert_bytes(boundary + i, &ch);
            model.splice(boundary + i..boundary + i, ch);
        }
        assert_eq!(t.collect_bytes().expect("collect"), model);
        let leaves = count_leaves(&t.root);
        assert!(
            leaves <= 8,
            "typing fragmented into {leaves} leaves (expected ~3)"
        );
        check_tree_invariants(&t).expect("invariants");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn v12_pager_coherence_and_truncation() {
        // Deterministic content: 3 full pages + a partial page, with
        // newlines scattered so metrics paths are exercised.
        let mut data = Vec::new();
        let total = 3 * PAGE_SIZE_BYTES + 12345;
        let mut rng = Rng::new(0x12345678);
        for i in 0..total {
            let b = if i % 97 == 0 {
                b'\n'
            } else {
                rng.byte()
            };
            data.push(b);
        }
        let path = temp_path("v12");
        std::fs::write(&path, &data).expect("write temp file");

        let t = BText::open_mapped(&path).expect("open_mapped");
        // Opening is O(1) in file size: len() needs no page fetch.
        assert_eq!(t.len(), data.len());
        assert_eq!(
            t.collect_bytes().expect("collect_bytes"),
            data,
            "mapped content != file bytes"
        );
        let want_nl = data.iter().filter(|&&b| b == b'\n').count();
        assert_eq!(t.total_newlines().expect("total_newlines"), want_nl);
        assert_eq!(t.line_count().expect("line_count"), want_nl + 1);
        // Cross-page slice (straddles the page 0/1 boundary).
        let s = PAGE_SIZE_BYTES - 10;
        assert_eq!(
            t.slice_bytes(s..s + 25).expect("slice"),
            &data[s..s + 25],
            "cross-page slice diverged"
        );
        // Line mapping on the mapped tree (exercises the page-skipping
        // newline search): every line start round-trips through the model.
        let mut starts = vec![0usize];
        for (i, &b) in data.iter().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        assert_eq!(t.line_count().expect("line_count"), starts.len());
        for (li, &st) in starts.iter().enumerate().step_by(49) {
            assert_eq!(
                t.line_to_byte(li).expect("line_to_byte"),
                Some(st),
                "mapped line_to_byte({li}) diverged"
            );
            assert_eq!(
                t.byte_to_line(st).expect("byte_to_line"),
                li,
                "mapped byte_to_line({st}) diverged"
            );
            let (s, e) = t
                .line_byte_range(li)
                .expect("line_byte_range")
                .expect("line in range");
            let e2 = if li + 1 < starts.len() {
                starts[li + 1]
            } else {
                data.len()
            };
            assert_eq!((s, e), (st, e2), "mapped line_byte_range({li}) diverged");
            // The line's bytes through the lazy line iterator.
            let line = t
                .lines_at(li)
                .next()
                .expect("lines_at yields")
                .expect("line read");
            assert_eq!(line.as_bytes(), &data[st..e2], "mapped line {li} bytes diverged");
        }
        // Edits on a mapped tree: insert splits descriptors (no read), the
        // untouched mapped regions stay byte-exact.
        let mut t2 = t.clone();
        t2.insert_bytes(PAGE_SIZE_BYTES + 7, b"<<<EDIT>>>\n");
        let tb2 = t2.collect_bytes().expect("collect after edit");
        let mut expect2 = data.clone();
        expect2.splice(PAGE_SIZE_BYTES + 7..PAGE_SIZE_BYTES + 7, b"<<<EDIT>>>\n".iter().copied());
        assert_eq!(tb2, expect2, "mapped edit diverged");

        // Truncate the file UNDER the open buffer (after open, before the
        // affected pages are fetched): reads past the new end must report
        // Truncated — a value, not a panic and not SIGBUS.
        //
        // NOTE on cache semantics: pages fetched before the truncation stay
        // readable (the pager cache is the open-time snapshot, consistent
        // with "reads are bounds-checked against the length recorded at
        // open"). Only unfetched pages observe the truncation.
        let tu = BText::open_mapped(&path).expect("open_mapped for truncation");
        // Touch only page 0, so later pages are still unfetched.
        assert_eq!(
            tu.slice_bytes(0..100).expect("prefix slice"),
            &data[0..100]
        );
        std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("reopen")
            .set_len(PAGE_SIZE_BYTES as u64 + 100)
            .expect("truncate");
        assert_eq!(tu.len(), data.len(), "len() is the open-time snapshot");
        match tu.collect_bytes() {
            Err(PagerError::Truncated { .. }) => {}
            other => panic!("expected Truncated, got {other:?}"),
        }
        // The page cached before truncation still reads (snapshot).
        assert_eq!(
            tu.slice_bytes(0..100).expect("cached prefix slice"),
            &data[0..100]
        );

        // Empty file maps to the empty tree.
        std::fs::write(&path, b"").expect("empty temp file");
        let te = BText::open_mapped(&path).expect("open_mapped empty");
        assert!(te.is_empty());
        assert_eq!(te.line_count().expect("line_count"), 1);

        std::fs::remove_file(&path).expect("remove temp file");
    }

    // -- V-13: streaming save is byte-exact ----------------------------------

    #[test]
    fn v13_write_to_is_byte_exact() {
        // Binary-hostile content: NULs, all 256 byte values, invalid UTF-8.
        let mut data: Vec<u8> = (0..=255u8).collect();
        data.extend_from_slice(b"\n\xff\xfe invalid \xe2\x82 end\n");
        let mut t = BText::from_bytes(&data);
        let mut m = Model { bytes: data };
        // Edit, then stream: untouched regions must be byte-exact.
        t.insert_bytes(3, b"INSERTED\n");
        m.insert(3, b"INSERTED\n");
        let dm = m.delete(10, 5);
        let dt = t.delete_bytes(10, 5).expect("in-memory delete");
        assert_eq!(dt, dm);
        let mut out = Vec::new();
        t.write_to(&mut out).expect("write_to");
        assert_eq!(out, m.bytes, "streamed save diverged from model");

        // Through a real file, on a mapped (lazy) tree.
        let path = temp_path("v13");
        std::fs::write(&path, &m.bytes).expect("write temp file");
        let t2 = BText::open_mapped(&path).expect("open_mapped");
        let mut out2 = Vec::new();
        t2.write_to(&mut out2).expect("write_to mapped");
        assert_eq!(out2, m.bytes, "mapped streaming save diverged");

        // Edit the mapped tree, then stream: the mapped regions stream
        // through the pager, byte-exact, without ever being fully resident.
        let mut t3 = t2.clone();
        t3.insert_bytes(0, b"HEADER\n");
        let mut out3 = Vec::new();
        t3.write_to(&mut out3).expect("write_to edited-mapped");
        let mut expect3 = b"HEADER\n".to_vec();
        expect3.extend_from_slice(&m.bytes);
        assert_eq!(out3, expect3, "edited-mapped streaming save diverged");

        std::fs::remove_file(&path).expect("remove temp file");
    }

    // -- Differential fuzz vs Vec<u8> ----------------------------------------
    //
    // Seeded xorshift (no new deps). Ops are applied in small batches; after
    // each batch the tree is checked for full observable equivalence plus
    // structural invariants. On failure the op log is persisted to
    // ~/workspace/erax-work/fuzz-corpus/btext/ BEFORE panicking, so the
    // reproducer survives. Scale: ERAX_FUZZ_ITERS batches (default 20_000),
    // ERAX_FUZZ_SEED. The heavy soak runs in release mode via the env var
    // and is recorded in PROGRESS.md (debug builds are ~100x slower per op;
    // the in-suite default is sized for a debug `cargo test`).

    #[derive(Debug, Clone)]
    enum FuzzOp {
        Ins { pos: usize, bytes: Vec<u8> },
        Del { pos: usize, len: usize },
    }

    fn fuzz_fail(iter: u64, seed: u64, msg: &str, log: &[FuzzOp], t: &BText, m: &Model) -> ! {
        let mut txt = format!("seed={seed}\niter={iter}\nerror={msg}\nops:\n");
        for op in log {
            // Cap dumped bytes so the corpus stays readable.
            match op {
                FuzzOp::Ins { pos, bytes } => {
                    txt.push_str(&format!("  ins pos={pos} len={} bytes={:02x?}\n", bytes.len(), &bytes[..bytes.len().min(32)]));
                }
                FuzzOp::Del { pos, len } => {
                    txt.push_str(&format!("  del pos={pos} len={len}\n"));
                }
            }
        }
        txt.push_str(&format!(
            "model_len={} tree_len={}\n",
            m.bytes.len(),
            t.len()
        ));
        if let Ok(home) = std::env::var("HOME") {
            let dir = std::path::PathBuf::from(home)
                .join("workspace/erax-work/fuzz-corpus/btext");
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join(format!("failure-{seed:x}-{iter}.txt")), &txt);
        }
        panic!("btext fuzz failure (reproducer persisted to fuzz-corpus/btext/ when writable):\n{txt}");
    }

    #[test]
    fn fuzz_differential_vs_model() {
        let iters: u64 = std::env::var("ERAX_FUZZ_ITERS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20_000);
        let seed: u64 = std::env::var("ERAX_FUZZ_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0x9E3779B97F4A7C15);
        let mut rng = Rng::new(seed);
        let mut log: Vec<FuzzOp> = Vec::new();
        // Small initial buffer; refreshed periodically to keep per-batch cost O(KB).
        let init: Vec<u8> = (0..rng.below(1024)).map(|_| rng.byte()).collect();
        let mut t = BText::from_bytes(&init);
        let mut m = Model { bytes: init };
        for iter in 0..iters {
            if iter > 0 && iter % 500 == 0 {
                let init: Vec<u8> = (0..rng.below(1024)).map(|_| rng.byte()).collect();
                t = BText::from_bytes(&init);
                m = Model { bytes: init };
                log.clear();
            }
            // A batch of 1..=8 ops per equivalence check: the checks dominate
            // cost, and the op log still pinpoints the failing batch.
            let batch = 1 + rng.below(8);
            for _ in 0..batch {
                let op = if rng.below(2) == 0 || m.bytes.is_empty() {
                    let pos = rng.below(m.bytes.len() + 1);
                    let n = rng.below(40);
                    let bytes: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
                    FuzzOp::Ins { pos, bytes }
                } else {
                    let pos = rng.below(m.bytes.len() + 1);
                    let len = rng.below(40);
                    FuzzOp::Del { pos, len }
                };
                match &op {
                    FuzzOp::Ins { pos, bytes } => {
                        t.insert_bytes(*pos, bytes);
                        m.insert(*pos, bytes);
                    }
                    FuzzOp::Del { pos, len } => {
                        let dm = m.delete(*pos, *len);
                        match t.delete_bytes(*pos, *len) {
                            Ok(dt) => {
                                if dt != dm {
                                    fuzz_fail(iter, seed, "delete_bytes return diverged", &log, &t, &m);
                                }
                            }
                            Err(e) => fuzz_fail(
                                iter,
                                seed,
                                &format!("delete_bytes failed on in-memory tree: {e}"),
                                &log,
                                &t,
                                &m,
                            ),
                        }
                    }
                }
                log.push(op);
            } // end batch
            if let Err(msg) = check_equiv_full(&t, &m, &mut rng)
                .and(check_tree_invariants(&t))
            {
                fuzz_fail(iter, seed, &msg, &log, &t, &m);
            }
        }
    }
}
