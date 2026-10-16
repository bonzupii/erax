//! Memory-mapped read-only file regions.
//!
//! The complete mitigation set for mapping untrusted file bytes:
//!  1. **Validated length**: `len` comes from `fstat` at open; we never
//!     build a slice at or beyond it (mitigation #1).
//!  2. **Post-read snapshot validation**: one `fstat` after every guarded
//!     read. Same length + different mtime means the content was replaced
//!     (truncate+regrow or in-place rewrite) — the bytes just read are
//!     discarded with `PagerError::FileChanged`. Checking *after* the deref
//!     is precise where a pre-read check has a TOCTOU window. Truncation is
//!     caught by the guard; the length cache short-circuits repeat faults.
//!     (mitigation #2).
//!  3. **SIGBUS handler with scoped recovery**: a fault inside a guarded
//!     read becomes `PagerError::Truncated`, never process death
//!     (mitigation #3).
//!  4. **Immutable snapshot**: `PROT_READ`/`MAP_PRIVATE`; the mapping is
//!     never written through (mitigation #5).
//!  5. **Fault-injection tests** (`tests/mmap_faults.rs`) prove the handler:
//!     deterministic truncation-under-map, shrink-to-zero under active
//!     reads, write-burst storms, rename-over.
//!
//! # Residual risks (documented honestly)
//! - A content replacement that also *grows* the file, or one that preserves
//!   `mtime`, is not detected (same residual as the pager backend).
//! - Already-faulted pages are not re-faulted after a regrow: the mtime
//!   check refuses the read rather than risk serving mixed old/new bytes.
//! - The handler only converts faults *inside an armed guard window*; every
//!   deref of mapping memory in this crate happens inside one (the module
//!   boundary is the choke point — see `read_guarded`).
//!
//! # Safety contract
//! - `ptr`/`map_len` come from a successful `libc::mmap` (`PROT_READ`,
//!   `MAP_PRIVATE`); the mapping is valid until `munmap` in `Drop`.
//! - The `File` is kept open, pinning the mapping's backing inode.
//! - All derefs go through `read_guarded`, which validates bounds first and
//!   arms the SIGBUS guard around the deref window.
//!
//! Platform note: Unix-only (`#[cfg(unix)]` at the `mod` declaration).
//! The `libc` crate does not expose `sigsetjmp`/`siglongjmp` on this
//! platform, so they are declared here via `extern "C"` (see `guard`).

use crate::core::btext::PagerError;
use std::fs::File;
#[cfg(not(miri))]
use std::os::unix::io::AsRawFd;
use std::time::SystemTime;

// -- Unsafe leaves (module level): each holds exactly one unsafe operation
// and no safe logic. The workspace denies `unsafe_code`; these per-item
// `#[allow(unsafe_code)]` attributes are the only unsafe surface here.

/// `mmap` a read-only private mapping of `map_len` bytes of `fd`.
///
/// SAFETY: `fd` must be a valid open file descriptor, `map_len > 0`, offset
/// 0 is page-aligned. Returns `MAP_FAILED` on error (checked by caller).
/// The mapping is valid until `raw_munmap`; the caller keeps `fd` open.
#[cfg(not(miri))]
#[allow(unsafe_code)]
fn raw_mmap(fd: libc::c_int, map_len: usize) -> *mut libc::c_void {
    unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            map_len,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            fd,
            0,
        )
    }
}

/// Unmap a previous `raw_mmap` region.
///
/// SAFETY: `ptr`/`len` must be the exact values from `raw_mmap`, not
/// already unmapped.
#[cfg(not(miri))]
#[allow(unsafe_code)]
fn raw_munmap(ptr: *mut libc::c_void, len: usize) {
    unsafe {
        libc::munmap(ptr, len);
    }
}

/// Build a byte slice over mapped memory at `base + offset`. Not
/// dereferenced here.
///
/// SAFETY: `[base+offset, base+offset+len)` must lie within a live
/// `PROT_READ` mapping. The caller dereferences only inside
/// `guard::guarded`, which converts a fault into a typed error.
#[allow(unsafe_code)]
fn mapped_slice_at<'a>(base: *const u8, offset: usize, len: usize) -> &'a [u8] {
    unsafe { std::slice::from_raw_parts(base.add(offset), len) }
}

/// SIGBUS recovery machinery (private).
///
/// A fault inside a guarded read must become a typed error, not process
/// death. Mechanism: a process-wide `SIGBUS` handler installed once, plus a
/// thread-local "protected region" stack. `read_guarded` arms the region
/// around the deref window; the handler compares the faulting address
/// against the current thread's innermost region and `siglongjmp`s back on
/// a match, otherwise delegates to the previous disposition (so unrelated
/// SIGBUSes behave exactly as they would without us).
///
/// Design notes:
/// - The handler body is async-signal-safe: a thread-local read
///   (lock-free), integer arithmetic, and `siglongjmp`. No allocation, no
///   locking, no panics.
/// - The `siglongjmp` target is the `sigsetjmp` site *inside* `guarded`,
///   i.e. below every Rust frame that holds a lock or a `RefCell` guard.
///   Only destructors between the `sigsetjmp` call and the fault are
///   skipped — that span is controlled here (the armed `Guard` has no
///   `Drop`; it is restored manually on the fault path).
/// - Registration is process-lifetime (`Once`); erax installs no other
///   `SIGBUS` disposition (verified 2026-10-15: no `signal_hook` dep, no
///   `sigaction` calls anywhere else in the tree; `std` only ignores
///   `SIGPIPE`), and the previous disposition is chained, so any future
///   one composes.
/// - Under Miri there are no real signals: installation is skipped and the
///   guard degrades to a pass-through (documented; fault-injection tests are
///   `#[cfg(not(miri))]`).
/// - `__sigsetjmp` carries no `returns_twice` metadata: the attribute is
///   unstable and unavailable on the pinned stable toolchain, so the
///   soundness argument is structural and must be re-checked if `guarded`
///   changes. What LLVM's single-return assumption could break is a local
///   that is *modified* between `sigsetjmp` and `siglongjmp` and then read
///   — the classic setjmp hazard. Audit of `guarded`: `base`, `end`, `env`,
///   and the closure `f` are all set before the call and never modified
///   after it; the fault path (`jmp_val != 0`) reads only the thread-local
///   slot (re-read, not a stale register) and drops `f` without calling
///   it. An opaque `extern "C"` call already forces LLVM to spill
///   caller-saved state around it per the platform ABI, and `siglongjmp`
///   restores callee-saved registers to their call-time values, so the
///   fault path observes the same stack memory the first pass left.
///   Rule for future edits: no new local may be assigned between the
///   `__sigsetjmp` call and the end of `guarded`.
mod guard {
    use std::cell::Cell;
    #[cfg(not(miri))]
    use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
    #[cfg(not(miri))]
    use std::sync::Once;

    /// Opaque storage for the C `sigjmp_buf`. glibc needs 320 bytes on
    /// x86_64 and 432 on aarch64; musl needs less. 512 bytes at 16-byte
    /// alignment covers all realistic Linux targets with margin. Rust never
    /// inspects the contents — only `sigsetjmp`/`siglongjmp` touch it.
    #[repr(C, align(16))]
    struct JmpBuf([std::mem::MaybeUninit<u8>; 512]);

    // `libc` does not expose these on this platform; declared directly.
    // Both are async-signal-safe per POSIX. Note: glibc implements
    // `sigsetjmp` as a *macro* expanding to `__sigsetjmp` — the exported
    // symbol is `__sigsetjmp`, so that is what we link against.
    // Under Miri these are never called (`guarded` is a pass-through there).
    #[cfg(not(miri))]
    #[allow(unsafe_code)]
    unsafe extern "C" {
        fn __sigsetjmp(env: *mut JmpBuf, savemask: libc::c_int) -> libc::c_int;
        fn siglongjmp(env: *mut JmpBuf, val: libc::c_int) -> !;
    }

    // -- Unsafe leaves: each holds exactly one unsafe operation and no
    // safe logic. The SAFETY comment on each states the contract its
    // callers uphold; the callers themselves are safe functions. --
    //
    // Rationale: the workspace denies `unsafe_code`; these per-item
    // `#[allow(unsafe_code)]` attributes are the only unsafe surface in
    // the data-structure module (owner pre-approved for memory mapping).

    /// Dereference a guard pointer.
    ///
    /// SAFETY: `p` must be a live `&Guard` on the calling thread's stack.
    /// The signal handler upholds this: it runs synchronously on the
    /// faulting thread, and the guard outlives its armed window (disarm
    /// restores the slot; the fault path pops the dead guard explicitly).
    #[cfg(any(not(miri), test))]
    #[allow(unsafe_code)]
    fn guard_from_raw<'a>(p: *const Guard) -> &'a Guard {
        unsafe { &*p }
    }

    /// Read the faulting address from a signal's `siginfo_t`.
    ///
    /// SAFETY: `info` must be a valid `siginfo_t` for the delivered signal
    /// (the kernel provides it to the `SA_SIGINFO` handler).
    #[cfg(not(miri))]
    #[allow(unsafe_code)]
    fn siginfo_fault_addr(info: *mut libc::siginfo_t) -> usize {
        unsafe { (*info).si_addr() as usize }
    }

    /// Jump back to the recovery point. Diverges.
    ///
    /// SAFETY: `env` must be a live `JmpBuf` on the calling thread's stack,
    /// previously initialized by `__sigsetjmp`. `siglongjmp` is
    /// async-signal-safe.
    #[cfg(not(miri))]
    #[allow(unsafe_code)]
    fn jump_to_recovery(env: *mut JmpBuf) -> ! {
        unsafe { siglongjmp(env, 1) }
    }

    /// Reinterpret a captured handler address as a 3-arg `SA_SIGINFO` fn.
    ///
    /// SAFETY: `addr` must be the `sa_sigaction` captured from the previous
    /// `sigaction` disposition at install, which used `SA_SIGINFO`.
    #[cfg(not(miri))]
    #[allow(unsafe_code)]
    fn prev_siginfo_fn(
        addr: usize,
    ) -> extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void) {
        unsafe { std::mem::transmute(addr) }
    }

    /// Reinterpret a captured handler address as a plain 1-arg `sa_handler`.
    ///
    /// SAFETY: `addr` must be the `sa_handler` captured from the previous
    /// `sigaction` disposition at install, which did NOT use `SA_SIGINFO`.
    #[cfg(not(miri))]
    #[allow(unsafe_code)]
    fn prev_handler_fn(addr: usize) -> extern "C" fn(libc::c_int) {
        unsafe { std::mem::transmute(addr) }
    }

    /// Restore the default `SIGBUS` disposition and re-raise. Diverges.
    ///
    /// Restore the default `SIGBUS` disposition and re-raise.
    ///
    /// SAFETY: both calls are async-signal-safe; only called for a fault
    /// outside our guarded windows, where dying with `SIGBUS` is the
    /// correct behavior (as if we were never installed).
    #[cfg(not(miri))]
    #[allow(unsafe_code)]
    fn restore_default_and_reraise() {
        unsafe {
            libc::signal(libc::SIGBUS, libc::SIG_DFL);
            libc::raise(libc::SIGBUS);
        }
    }

    /// A zeroed `sigaction` (fields initialized by the caller).
    ///
    /// SAFETY: zeroed bytes are a valid initial `sigaction` image; the
    /// caller initializes every field before use.
    #[cfg(not(miri))]
    #[allow(unsafe_code)]
    fn sigaction_zeroed() -> libc::sigaction {
        unsafe { std::mem::zeroed() }
    }

    /// Empty a signal set.
    ///
    /// SAFETY: `set` must point to a live `sigset_t`. `sigemptyset` is
    /// async-signal-safe.
    #[cfg(not(miri))]
    #[allow(unsafe_code)]
    fn sigemptyset(set: *mut libc::sigset_t) -> libc::c_int {
        unsafe { libc::sigemptyset(set) }
    }

    /// Install a `SIGBUS` disposition, capturing the previous one.
    ///
    /// SAFETY: `new` must be a fully initialized `sigaction`; `old` a live
    /// slot for the previous disposition. Called once at install.
    #[cfg(not(miri))]
    #[allow(unsafe_code)]
    fn sigaction_install(new: &libc::sigaction, old: &mut libc::sigaction) -> libc::c_int {
        unsafe { libc::sigaction(libc::SIGBUS, new, old) }
    }

    /// Query the current `SIGBUS` disposition without changing it.
    ///
    /// SAFETY: `old` must be a live slot for the disposition. Test-only.
    #[cfg(test)]
    #[allow(unsafe_code)]
    fn sigaction_query(old: &mut libc::sigaction) -> libc::c_int {
        unsafe { libc::sigaction(libc::SIGBUS, std::ptr::null(), old) }
    }

    /// A zeroed `JmpBuf` for `raw_sigsetjmp` to initialize.
    ///
    /// SAFETY: zeroed `MaybeUninit` bytes are a valid initial `sigjmp_buf`
    /// image; `raw_sigsetjmp` initializes it before any use.
    #[allow(unsafe_code)]
    fn jmp_buf_zeroed() -> JmpBuf {
        unsafe { std::mem::zeroed() }
    }

    /// Read the `prev` link of a dead guard.
    ///
    /// SAFETY: `dead` must be the `Guard` armed by the calling `guarded`
    /// invocation; its `prev` field is plain data, untouched by the fault.
    #[allow(unsafe_code)]
    fn guard_prev_link(dead: *const Guard) -> *const Guard {
        unsafe { (*dead).prev }
    }

    /// One armed protected region, living on the arming thread's stack.
    struct Guard {
        /// Start of the protected address range (the mapping base).
        /// Read only by the signal handler, which does not exist under Miri.
        #[cfg_attr(miri, allow(dead_code))]
        base: usize,
        /// One past the end of the protected range.
        /// Read only by the signal handler, which does not exist under Miri.
        #[cfg_attr(miri, allow(dead_code))]
        end: usize,
        /// Recovery point for `siglongjmp` (stack slot in `guarded`).
        /// Read only by the signal handler, which does not exist under Miri.
        #[cfg_attr(miri, allow(dead_code))]
        env: *mut JmpBuf,
        /// Link for stack discipline when guards nest.
        prev: *const Guard,
    }

    // `*const Guard` is not `Send`/`Sync`, but this is thread-local
    // storage — never shared between threads. `Cell` access is lock-free.
    thread_local! {
        static ACTIVE: Cell<*const Guard> = const { Cell::new(std::ptr::null()) };
    }

    #[cfg(not(miri))]
    static INSTALL: Once = Once::new();
    /// Previous disposition, captured at install: 0 = default, 1 = ignore,
    /// 2 = custom handler (address in `PREV_HANDLER`, with `PREV_SIGINFO`
    /// recording whether it is a 3-arg `SA_SIGINFO` handler or a plain 1-arg
    /// `sa_handler`). Written once inside `INSTALL.call_once` before any
    /// signal can reach the new handler on a guarded thread (guards can
    /// only arm after `ensure_installed` returns); read-only afterwards.
    /// Atomics keep the handler free of `static mut`.
    #[cfg(not(miri))]
    static PREV_KIND: AtomicU8 = AtomicU8::new(0);
    #[cfg(not(miri))]
    static PREV_HANDLER: AtomicUsize = AtomicUsize::new(0);
    /// Whether the previous handler used `SA_SIGINFO` (3-arg) or is a plain
    /// 1-arg `sa_handler`. Only meaningful when `PREV_KIND == 2`.
    #[cfg(not(miri))]
    static PREV_SIGINFO: AtomicU8 = AtomicU8::new(0);

    /// Our `SIGBUS` handler (`SA_SIGINFO`).
    #[cfg(not(miri))]
    extern "C" fn on_sigbus(
        sig: libc::c_int,
        info: *mut libc::siginfo_t,
        ctx: *mut libc::c_void,
    ) {
        let ours = ACTIVE.try_with(|slot| {
            let p = slot.get();
            if p.is_null() {
                return false;
            }
            let g = guard_from_raw(p);
            let addr = siginfo_fault_addr(info);
            // Branchless range check: `addr in [base, end)`.
            if addr.wrapping_sub(g.base) < g.end.wrapping_sub(g.base) {
                jump_to_recovery(g.env)
            }
            false
        });
        if ours == Ok(true) {
            return; // Unreachable: `siglongjmp` never returns.
        }
        // Not a fault inside one of our guarded windows: behave as if we
        // were never installed.
        delegate(sig, info, ctx);
    }

    /// Previous-disposition chaining for foreign `SIGBUS`es.
    /// Only called from the signal handler; async-signal-safe.
    #[cfg(not(miri))]
    fn delegate(sig: libc::c_int, info: *mut libc::siginfo_t, ctx: *mut libc::c_void) {
        match PREV_KIND.load(Ordering::Relaxed) {
            1 => {} // previously ignored: keep ignoring
            2 => {
                let addr = PREV_HANDLER.load(Ordering::Relaxed);
                if PREV_SIGINFO.load(Ordering::Relaxed) == 1 {
                    let f = prev_siginfo_fn(addr);
                    f(sig, info, ctx);
                } else {
                    // Plain 1-arg `sa_handler` (was installed without
                    // SA_SIGINFO): call with just the signal number.
                    let f = prev_handler_fn(addr);
                    f(sig);
                }
            }
            _ => {
                // Previously default: restore it and re-raise so the process
                // dies with SIGBUS exactly as it would have without us.
                restore_default_and_reraise();
            }
        }
    }

    /// Install the process-wide `SIGBUS` handler (once). Called before any
    /// guarded read.
    pub(super) fn ensure_installed() {
        #[cfg(miri)]
        {
            // Miri cannot install or deliver real signals.
            return;
        }
        #[cfg(not(miri))]
        INSTALL.call_once(|| {
            // Zeroed `sigaction`, then every field initialized below;
            // `SA_SIGINFO` selects the 3-argument handler.
            let mut new: libc::sigaction = sigaction_zeroed();
            new.sa_sigaction = on_sigbus as *const () as usize;
            new.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
            sigemptyset(&mut new.sa_mask);
            let mut old: libc::sigaction = sigaction_zeroed();
            if sigaction_install(&new, &mut old) != 0 {
                // `sigaction` failing here is a broken-kernel
                // situation; fail fast rather than run unprotected.
                panic!("erax mmap: sigaction(SIGBUS) failed");
            }
            let kind = if old.sa_sigaction == libc::SIG_DFL {
                0
            } else if old.sa_sigaction == libc::SIG_IGN {
                1
            } else {
                2
            };
            PREV_KIND.store(kind, Ordering::Relaxed);
            PREV_HANDLER.store(old.sa_sigaction, Ordering::Relaxed);
            PREV_SIGINFO.store(
                u8::from(old.sa_flags & libc::SA_SIGINFO != 0),
                Ordering::Relaxed,
            );
        });
    }

    /// Run `f` with `[base, end)` protected: a `SIGBUS` fault inside `f` at
    /// an address in `[base, end)` returns `Err(())` instead of killing the
    /// process. Faults elsewhere propagate to the previous disposition.
    ///
    /// Contract on `f`: it must not hold non-async-signal-safe locks across
    /// the call — `siglongjmp` skips destructors between here and the
    /// fault. (Our call sites satisfy this: the only work inside the
    /// window is a `memcpy` or a pure byte scan.)
    /// Exception to the leaf rule: `__sigsetjmp` captures the *calling*
    /// frame, so it cannot be extracted to a leaf — `siglongjmp` would
    /// return into a dead frame (undefined behavior). The unsafe operation
    /// here is still exactly one call; everything else in this function is
    /// safe code built on the leaves above.
    #[allow(unsafe_code)]
    pub(super) fn guarded<T>(base: usize, end: usize, f: impl FnOnce() -> T) -> Result<T, ()> {
        // Miri cannot install or deliver real signals, nor execute the raw
        // `__sigsetjmp`/`siglongjmp` FFI — so under Miri the recovery point
        // is never taken (`jmp_val` stays 0). The arming/disarming logic
        // below still runs and is checked by Miri; the memory-backed
        // `MmapRegion` cannot fault, so recovery is never needed. (The
        // fault path compiles under Miri but never executes.)
        ensure_installed();
        let mut env: JmpBuf = jmp_buf_zeroed();
        #[cfg(miri)]
        let jmp_val = 0;
        // `savemask = 1`: restore the signal mask on `siglongjmp`, so the
        // SIGBUS blocked during handler execution doesn't leak into normal
        // execution.
        //
        // SAFETY: `env` is a live stack slot for the whole guarded window
        // (kept alive past the fault window by the `black_box` below); this
        // call executes in `guarded`'s own frame, which is where
        // `siglongjmp` returns to.
        #[cfg(not(miri))]
        let jmp_val = unsafe { __sigsetjmp(&mut env as *mut JmpBuf, 1) };
        if jmp_val == 0 {
            // Arm: push onto this thread's guard stack (linked `prev`
            // gives stack discipline for nested guards).
            let prev = ACTIVE.with(|slot| {
                let old = slot.get();
                // `guard` is not yet constructed; link it below.
                old
            });
            let guard = Guard {
                base,
                end,
                env: &mut env as *mut JmpBuf,
                prev,
            };
            // Disarm guard: restores the slot even if `f` panics.
            struct Disarm {
                prev: *const Guard,
            }
            impl Drop for Disarm {
                fn drop(&mut self) {
                    ACTIVE.with(|slot| slot.set(self.prev));
                }
            }
            ACTIVE.with(|slot| slot.set(&guard as *const Guard));
            let _disarm = Disarm { prev };
            let out = f();
            // Liveness anchor: the signal handler reads `guard` and `env`
            // via ACTIVE, which the optimizer cannot see. After `slot.set`,
            // neither is read again in normal control flow — without this,
            // release builds reuse their stack slots for `f`'s temporaries
            // and the handler reads garbage (real bug: release-only SIGBUS
            // death, debug fine). `black_box` extends their liveness past
            // the fault window.
            std::hint::black_box((&guard, &env));
            // `Disarm::drop` runs here (and on unwind): slot restored.
            Ok(out)
        } else {
            // Arrived via `siglongjmp`: a mapping fault inside `f`. The `f`
            // frame — and the `Disarm` above it — are gone (longjmp skips
            // destructors), so pop the dead guard manually. A fault always
            // targets the innermost armed guard: the handler reads the
            // current top of this thread's stack, so popping exactly one is
            // correct, including under nesting.
            ACTIVE.with(|slot| {
                let dead = slot.get();
                let prev = if dead.is_null() {
                    std::ptr::null()
                } else {
                    guard_prev_link(dead)
                };
                slot.set(prev);
            });
            Err(())
        }
    } // close fn guarded

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn guard_passthrough_no_fault() {
            // No fault: the closure runs and its value comes back.
            let r: Result<u32, ()> = guarded(0x1000, 0x2000, || 42);
            assert_eq!(r, Ok(42));
        }

        #[test]
        fn guard_restores_slot_after_use() {
            let _: Result<(), ()> = guarded(0x1000, 0x2000, || ());
            ACTIVE.with(|slot| assert!(slot.get().is_null(), "slot must be disarmed"));
        }

        #[test]
        fn guard_nesting_disarms_in_order() {
            let r: Result<u32, ()> = guarded(0x1000, 0x2000, || {
                let inner: Result<u32, ()> = guarded(0x3000, 0x4000, || 7);
                assert_eq!(inner, Ok(7));
                ACTIVE.with(|slot| {
                    let p = slot.get();
                    assert!(!p.is_null());
                    // Test-only; the outer guard is armed.
                    let g = guard_from_raw(p);
                    assert_eq!((g.base, g.end), (0x1000, 0x2000));
                });
                9
            });
            assert_eq!(r, Ok(9));
            ACTIVE.with(|slot| assert!(slot.get().is_null()));
        }

        #[test]
        fn handler_installed_once() {
            ensure_installed();
            ensure_installed();
            // Second call is a no-op (Once); reaching here is the test.
            #[cfg(not(miri))]
            {
                // The handler must now be ours: query the disposition.
                let mut cur: libc::sigaction = sigaction_zeroed();
                let rc = sigaction_query(&mut cur);
                assert_eq!(rc, 0);
                assert_eq!(cur.sa_sigaction, on_sigbus as *const () as usize);
            }
        }
    }
}

/// A read-only memory mapping of a file's `[0, len)`.
pub struct MmapRegion {
    ptr: *const u8,
    /// Validated length from fstat at open. Never deref beyond this.
    len: u64,
    /// Actual mapping size (page-rounded up from `len`).
    map_len: usize,
    /// Kept open: pins the inode behind the mapping.
    _file: File,
    /// Miri stand-in for the OS mapping (see `new`): Miri cannot execute
    /// `mmap`, so the "mapping" is a heap allocation. `ptr` points into it;
    /// Miri then checks every `from_raw_parts` in `read_guarded` for
    /// provenance, bounds, and alignment with full rigor.
    #[cfg(miri)]
    _backing: Vec<u8>,
    open_mtime: Option<SystemTime>,
    /// Last observed file length (from the post-read fstat). Pre-read
    /// short-circuit: if `offset >= cached_len`, the region is known-gone —
    /// return `Truncated` without faulting (avoids a signal storm of
    /// repeated SIGBUS deliveries on a truncated file). Refreshed by every
    /// post-read fstat and by the fault path. May be stale-large (a shrink
    /// after our last read) — the SIGBUS guard backs it up; never
    /// stale-small in a way that causes spurious errors (see `read_guarded`).
    /// `Cell` for interior mutability: a benign cache update.
    cached_len: std::cell::Cell<u64>,
}

impl MmapRegion {
    /// Map `file` read-only. `len` is taken from fstat here (validated
    /// length before any deref — mitigation #1).
    pub fn new(file: File) -> Result<Self, PagerError> {
        guard::ensure_installed();
        let meta = file.metadata().map_err(|e| PagerError::Io { offset: 0, source: e })?;
        let len = meta.len();
        let open_mtime = meta.modified().ok();
        if len == 0 {
            // Empty file: no mapping needed; reads are bounds-checked to empty.
            return Ok(MmapRegion {
                ptr: std::ptr::null(),
                len: 0,
                map_len: 0,
                _file: file,
                #[cfg(miri)]
                _backing: Vec::new(),
                open_mtime,
                cached_len: std::cell::Cell::new(0),
            });
        }
        // Under Miri there is no real `mmap`: read the file into a heap
        // allocation and treat it as the mapping. `ptr` points into
        // `_backing`, so Miri validates every derived slice for provenance
        // and bounds. (External-mutation races cannot occur under Miri —
        // the revalidation/fault paths are still exercised logically.)
        #[cfg(miri)]
        {
            use std::io::Read;
            let mut backing = Vec::new();
            let mut f = file.try_clone().map_err(|e| PagerError::Io { offset: 0, source: e })?;
            f.read_to_end(&mut backing)
                .map_err(|e| PagerError::Io { offset: 0, source: e })?;
            // Defensive: the file may have grown between fstat and read;
            // the validated length is authoritative — truncate the view.
            backing.truncate(len as usize);
            let ptr = backing.as_ptr();
            let map_len = backing.len();
            return Ok(MmapRegion {
                ptr,
                len,
                map_len,
                _file: file,
                _backing: backing,
                open_mtime,
                cached_len: std::cell::Cell::new(len),
            });
        }
        #[cfg(not(miri))]
        {
            let page = 4096usize; // OS page size (x86_64)
            let map_len = (len as usize).div_ceil(page) * page;
            // `len > 0`, fd is valid and open, offset 0 is page-aligned,
            // PROT_READ|MAP_PRIVATE, MAP_FAILED checked below. The mapping is
            // valid until munmap in Drop; `File` is kept open.
            let ptr = raw_mmap(file.as_raw_fd(), map_len);
            if ptr == libc::MAP_FAILED {
                return Err(PagerError::Io { offset: 0, source: std::io::Error::last_os_error() });
            }
            Ok(MmapRegion {
                ptr: ptr as *const u8,
                len,
                map_len,
                _file: file,
                open_mtime,
                cached_len: std::cell::Cell::new(len),
            })
        }
    }

    /// Run `f` over the `[offset, offset+len)` bytes of the mapping under
    /// the full mitigation stack: immutable bounds check, the SIGBUS-guarded
    /// deref, then post-read snapshot validation.
    ///
    /// All-or-nothing (the pager contract): a shrink under the open length
    /// yields `Err(PagerError::Truncated)`, never a short slice. A fault
    /// inside the guarded window becomes `Truncated` — never process death.
    /// A same-length content replacement observed after the read becomes
    /// `Err(PagerError::FileChanged)` — never silent substitution.
    ///
    /// This is the ONLY path that derefs mapping memory in the crate: the
    /// module boundary is the choke point, so every mapping deref is
    /// guarded by construction.
    ///
    /// Why the validation is *post*-read: `MAP_PRIVATE` read-faults map the
    /// page cache (not copy-on-write), so an in-place file rewrite is
    /// visible in already-faulted pages WITHOUT faulting — a pre-read mtime
    /// check has a TOCTOU window, and a cached mtime can miss replacements.
    /// Checking *after* the deref is precise: if mtime is unchanged, no
    /// replacement happened before we finished reading, so the bytes are
    /// the pinned snapshot; if it changed, we discard them. One fstat per
    /// read.
    /// Validate `[offset, offset+len)` against the open length and the
    /// shrink cache. Shared by the guarded read primitives below.
    fn check_range(&self, offset: u64, len: usize) -> Result<(), PagerError> {
        let end = offset
            .checked_add(len as u64)
            .ok_or(PagerError::OutOfRange {
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
        // Fast path: a previous read observed a shrink covering this
        // offset — return `Truncated` without faulting (avoids a signal
        // storm of repeated SIGBUS deliveries). `cached_len` may be
        // stale-large (shrink after our last read): then the guarded deref
        // faults and the `Err(())` arm below refreshes it. It is never
        // stale-small in a way that spuriously errors: it only shrinks via
        // observed fstats, and `offset < self.len` here, so a stale-small
        // `cached_len` can only come from a genuine observed shrink — in
        // which case `Truncated` is correct anyway (the file really did
        // shrink; if it regrew, the post-read fstat below refreshes the
        // cache and the next read proceeds).
        if len > 0 && offset >= self.cached_len.get() {
            return Err(PagerError::Truncated {
                offset,
                wanted: len as u64,
            });
        }
        Ok(())
    }

    /// Post-read snapshot validation (one fstat): same length + different
    /// mtime means the content was replaced (truncate+regrow or in-place
    /// rewrite) — the bytes just read cannot be trusted, discard them.
    /// A changed length is fine: shrinks are caught by the guard above,
    /// and appends are invisible by design (semantic 1). The cache is
    /// refreshed as a side effect.
    ///
    /// Residuals (documented): a replacement preserving mtime, or one
    /// racing this fstat itself, is not detected — same as the pager.
    fn post_read_check(&self, offset: u64) -> Result<(), PagerError> {
        if let Ok(meta) = self._file.metadata() {
            let cur_len = meta.len();
            self.cached_len.set(cur_len);
            if cur_len == self.len
                && let Some(cur_mtime) = meta.modified().ok()
                && let Some(open_mtime) = self.open_mtime
                && cur_mtime != open_mtime
            {
                return Err(PagerError::FileChanged { offset });
            }
        }
        Ok(())
    }

    pub fn read_guarded<T>(
        &self,
        offset: u64,
        len: usize,
        f: impl FnOnce(&[u8]) -> T,
    ) -> Result<T, PagerError> {
        self.check_range(offset, len)?;
        if len == 0 {
            // No deref needed; run `f` on the empty slice for uniformity.
            return Ok(f(&[]));
        }
        // `ptr` is non-null: `len > 0` and `end <= self.len` imply the file
        // was non-empty at open, the only path that sets a null `ptr`.
        debug_assert!(!self.ptr.is_null());
        let base = self.ptr as usize;
        // The slice is *constructed* here but not dereferenced — the first
        // actual memory access happens inside `guard::guarded`, which
        // converts any fault into `Err(())`. `offset + len <= self.len <=
        // map_len`, so the slice is within the mapping.
        let slice = mapped_slice_at(self.ptr, offset as usize, len);
        let out = match guard::guarded(base, base + self.map_len, || f(slice)) {
            Ok(out) => out,
            Err(()) => {
                // Fault on the mapping: the file shrank under the open
                // length. Refresh the cache so the fast path above
                // short-circuits subsequent reads.
                if let Ok(meta) = self._file.metadata() {
                    self.cached_len.set(meta.len());
                }
                return Err(PagerError::Truncated {
                    offset,
                    wanted: len as u64,
                });
            }
        };
        self.post_read_check(offset)?;
        Ok(out)
    }

    /// Copy `[offset, offset+len)` into `out` under the SIGBUS guard.
    ///
    /// This is the allocation-safe bulk-read primitive. `out` is
    /// caller-owned: the reserve happens *before* the guard is armed, and
    /// the in-window work is a plain slice copy whose length commit is
    /// skipped if a fault longjmps out — so `out` keeps length 0 and no
    /// allocation owned by the window is stranded. On `Err`, `out` is left
    /// empty (the post-read check may also reject just-read bytes).
    ///
    /// Prefer this over `read_guarded(offset, len, |s| s.to_vec())` for
    /// copies: the `to_vec` closure allocates *inside* the guarded window,
    /// and a fault during the copy `siglongjmp`s out, skipping that
    /// allocation's destructor — a bounded leak per faulted read. No new
    /// `unsafe` is needed here: `extend_from_slice` is safe code, and the
    /// guard only ever skips frames below the caller's ownership of `out`.
    pub fn read_guarded_into(
        &self,
        offset: u64,
        len: usize,
        out: &mut Vec<u8>,
    ) -> Result<(), PagerError> {
        // Cleared up front so the "empty on Err" contract is uniform across
        // every error path (validation, fault, post-read rejection).
        out.clear();
        self.check_range(offset, len)?;
        if len == 0 {
            return Ok(());
        }
        debug_assert!(!self.ptr.is_null());
        let base = self.ptr as usize;
        let slice = mapped_slice_at(self.ptr, offset as usize, len);
        // Reserved before arming: no allocation can occur inside the window
        // whose owner would be skipped by a fault.
        out.reserve(len);
        let copied: Result<(), ()> = guard::guarded(base, base + self.map_len, || {
            out.extend_from_slice(slice);
        });
        match copied {
            Ok(()) => {}
            Err(()) => {
                // Fault mid-copy: `extend_from_slice`'s length commit was
                // skipped, so `out` still has length 0. Refresh the cache
                // so the fast path short-circuits subsequent reads.
                if let Ok(meta) = self._file.metadata() {
                    self.cached_len.set(meta.len());
                }
                return Err(PagerError::Truncated {
                    offset,
                    wanted: len as u64,
                });
            }
        }
        if let Err(e) = self.post_read_check(offset) {
            out.clear();
            return Err(e);
        }
        Ok(())
    }

    /// Validated length.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// True if the mapped file was empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Drop for MmapRegion {
    fn drop(&mut self) {
        // Under Miri there is no mapping: `_backing` drops itself.
        #[cfg(not(miri))]
        if self.map_len > 0 {
            // `ptr`/`map_len` are the exact values from `raw_mmap`.
            raw_munmap(self.ptr as *mut libc::c_void, self.map_len);
        }
    }
}

// SAFETY: the mapping is read-only and never mutated through `self`;
// sharing `&MmapRegion` across threads is safe (like `&[u8]`).
#[allow(unsafe_code)]
unsafe impl Send for MmapRegion {}
#[allow(unsafe_code)]
unsafe impl Sync for MmapRegion {}

#[cfg(test)]
mod region_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    struct TmpGuard {
        path: std::path::PathBuf,
    }

    impl Drop for TmpGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn tmp_file(tag: &str, content: &[u8]) -> (TmpGuard, File) {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "erax-mmap-unit-{}-{id}-{tag}",
            std::process::id()
        ));
        std::fs::write(&path, content).expect("write temp file");
        let file = File::open(&path).expect("open temp file");
        (TmpGuard { path }, file)
    }

    #[test]
    fn read_guarded_roundtrip() {
        let content: Vec<u8> = (0..65536u32).map(|i| (i & 0xff) as u8).collect();
        let (_g, file) = tmp_file("roundtrip", &content);
        let region = MmapRegion::new(file).expect("map failed");
        assert_eq!(region.len(), 65536);
        // Whole range, sub-ranges, single bytes, empty read.
        let all = region.read_guarded(0, 65536, |s| s.to_vec()).expect("read all");
        assert_eq!(all, content);
        let sub = region.read_guarded(1000, 5000, |s| s.to_vec()).expect("read sub");
        assert_eq!(sub, &content[1000..6000]);
        let one = region.read_guarded(65535, 1, |s| s[0]).expect("read one");
        assert_eq!(one, content[65535]);
        let empty: Vec<u8> = region.read_guarded(0, 0, |s| s.to_vec()).expect("read empty");
        assert!(empty.is_empty());
        // Pure-scan closure (the count_newlines_in path shape).
        let nls = region
            .read_guarded(0, 65536, |s| s.iter().filter(|&&b| b == b'\n').count())
            .expect("scan");
        assert_eq!(nls, content.iter().filter(|&&b| b == b'\n').count());
    }

    #[test]
    fn read_guarded_into_roundtrip() {
        let content: Vec<u8> = (0..65536u32).map(|i| (i & 0xff) as u8).collect();
        let (_g, file) = tmp_file("into-roundtrip", &content);
        let region = MmapRegion::new(file).expect("map failed");
        let mut out = Vec::new();
        // Whole range, sub-range, single byte, empty read — each must match
        // the `read_guarded` copy path byte for byte.
        region.read_guarded_into(0, 65536, &mut out).expect("read all");
        assert_eq!(out, content);
        region.read_guarded_into(1000, 5000, &mut out).expect("read sub");
        assert_eq!(out, &content[1000..6000]);
        region.read_guarded_into(65535, 1, &mut out).expect("read one");
        assert_eq!(out, &[content[65535]]);
        region.read_guarded_into(0, 0, &mut out).expect("read empty");
        assert!(out.is_empty());
        // Pre-filled buffer: stale contents are replaced, not appended to.
        out.extend_from_slice(b"stale");
        region.read_guarded_into(0, 4, &mut out).expect("overwrite");
        assert_eq!(out, &content[0..4]);
        // Out-of-range behaves like the generic primitive.
        match region.read_guarded_into(3, 70000, &mut out) {
            Err(PagerError::OutOfRange { offset, len, file_len }) => {
                assert_eq!((offset, len, file_len), (3, 70000, 65536));
            }
            other => panic!("expected OutOfRange, got {other:?}"),
        }
        assert!(out.is_empty(), "out must be empty after a failed read");
    }

    #[test]
    fn read_guarded_out_of_range() {
        let (_g, file) = tmp_file("oor", b"hello");
        let region = MmapRegion::new(file).expect("map failed");
        match region.read_guarded(3, 10, |s| s.to_vec()) {
            Err(PagerError::OutOfRange { offset, len, file_len }) => {
                assert_eq!((offset, len, file_len), (3, 10, 5));
            }
            other => panic!("expected OutOfRange, got {other:?}"),
        }
        // Offset arithmetic overflow.
        match region.read_guarded(u64::MAX, 8, |s| s.to_vec()) {
            Err(PagerError::OutOfRange { .. }) => {}
            other => panic!("expected OutOfRange on overflow, got {other:?}"),
        }
    }

    #[test]
    fn read_guarded_empty_file() {
        let (_g, file) = tmp_file("empty", b"");
        let region = MmapRegion::new(file).expect("map failed");
        assert!(region.is_empty());
        let empty: Vec<u8> = region.read_guarded(0, 0, |s| s.to_vec()).expect("empty read");
        assert!(empty.is_empty());
        match region.read_guarded(0, 1, |s| s.to_vec()) {
            Err(PagerError::OutOfRange { .. }) => {}
            other => panic!("expected OutOfRange, got {other:?}"),
        }
    }

    // Truncation is inherently unobservable under Miri: the Miri stand-in
    // is a heap snapshot taken at open, so post-open file mutation cannot
    // affect it. Covered natively here and by tests/mmap_faults.rs.
    #[cfg(not(miri))]
    #[test]
    fn read_guarded_truncated_file() {
        // Truncation observed via revalidation (epoch 0 revalidates on the
        // first read): deterministic without needing the signal path.
        let (g, file) = tmp_file("trunc", &[0xABu8; 8192]);
        let region = MmapRegion::new(file).expect("map failed");
        // Sanity: reads fine before truncation.
        region.read_guarded(0, 8192, |s| s.to_vec()).expect("baseline");
        // Truncate through a writable handle (the read-only mapped handle
        // cannot truncate; reopen by path — same inode).
        std::fs::OpenOptions::new()
            .write(true)
            .open(&g.path)
            .expect("open for truncate")
            .set_len(100)
            .expect("set_len failed");
        match region.read_guarded(0, 8192, |s| s.to_vec()) {
            Err(PagerError::Truncated { offset, wanted }) => {
                assert_eq!(offset, 0);
                assert_eq!(wanted, 8192);
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
        // The surviving prefix still reads.
        let prefix = region.read_guarded(0, 100, |s| s.to_vec()).expect("prefix read");
        assert_eq!(prefix, vec![0xABu8; 100]);
    }
}
