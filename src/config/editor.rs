//! Editor behavior settings: typed `pub const`s.
//!
//! Edit the values and rebuild. A wrong type is a compile error (principle 1).

/// Spaces per tab stop.
pub const TAB_WIDTH: usize = 8;

/// Use tabs (true) or spaces (false) for indentation.
pub const USE_TABS: bool = true;

/// Maximum bytes kept in the kill ring entry; oversized kills are reported to
/// the user, never silently dropped.
pub const KILL_RING_MAX_BYTES: usize = 10 * 1024 * 1024;

/// Adjacent single-character edits are grouped for undo up to this many.
/// This is the one grouping rule; it is a setting, not a magic number.
pub const UNDO_SMALL_EDIT_BATCH: usize = 100;

/// Shell spawned for `shell-command` and the integrated terminal.
/// A typed constant, not a hardcoded literal at the spawn site.
pub const SHELL: &str = "sh";

/// Pager / lazy-loading tunables for the in-house byte B-tree (`core::btext`).
/// All bounds are hard caps: memory stays bounded no matter the file size.
///
/// Pager page size in bytes. Files are materialized from disk in units of
/// this size; a page is the atomic unit of the LRU cache.
pub const PAGE_SIZE_BYTES: usize = 64 * 1024;

/// Maximum number of pager pages resident in memory at once.
/// 256 pages × 64 KiB = 16 MiB worst-case page-cache residency per mapped file.
pub const PAGE_CACHE_MAX_PAGES: usize = 256;

/// Mapped-leaf region size in bytes. A lazily opened file is represented as
/// one tree leaf per region; regions are split into pages on first touch.
pub const MAP_REGION_BYTES: u64 = 1024 * 1024;

/// Owned leaves larger than this are split after edits (keeps leaves small
/// enough for cheap splice operations). A 64 KiB leaf makes every 1-byte
/// insert pay a 64 KiB memmove; 8 KiB keeps splices cheap and the tree
/// shallow.
pub const MAX_LEAF_BYTES: usize = 8 * 1024;

/// B-tree fan-out cap. Internal nodes hold at most this many children;
/// the minimum (except root) is half this.
pub const BTREE_MAX_CHILDREN: usize = 32;
