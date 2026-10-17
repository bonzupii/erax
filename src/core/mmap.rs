//! Memory-mapped read-only file regions.
//!
//! The complete mitigation set for mapping untrusted file bytes:
//!  1. **Validated length**: `len` comes from `fstat` at open; we never
//!     build a slice at or beyond it (mitigation #1).
//!  2. **Post-read snapshot validation**: one `fstat` after every guarded
//!     read. A read range extending beyond the current length means the
//!     snapshot shrank there — `Truncated` (the guard catches shrinks that
//!     fault; the range check catches the ones within the last mapped page,
//!     which fault nothing; a pure shrink still serves its intact prefix).
//!     Same length + different mtime means the content was replaced
//!     (truncate+regrow or in-place rewrite) — the bytes just read are
//!     discarded with `PagerError::FileChanged`. Checking *after* the deref
//!     is precise where a pre-read check has a TOCTOU window. The length
//!     cache short-circuits repeat faults. (mitigation #2).
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
//! Platform note: Unix and Windows. The OS veneer (mapping creation via
//! `mmap`/`CreateFileMappingW`, fault trapping via a `SIGBUS` handler / a
//! vectored exception handler) is the only OS-specific code; everything
//! above it — the region, the branded guard, the snapshot revalidation —
//! is shared. The `libc` crate does not expose `sigsetjmp`/`siglongjmp` on
//! unix, so they are declared via `extern "C"` (see `guard`); the Windows
//! kernel32 and CRT imports are hand-declared the same way (no new
//! dependency).

use crate::core::btext::PagerError;
use std::fs::File;
#[cfg(all(unix, not(miri)))]
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
#[cfg(all(unix, not(miri)))]
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
#[cfg(all(unix, not(miri)))]
#[allow(unsafe_code)]
fn raw_munmap(ptr: *mut libc::c_void, len: usize) {
    unsafe {
        libc::munmap(ptr, len);
    }
}

// -- Windows veneer: hand-declared kernel32 + CRT imports (no new
// dependency, matching the unix hand-declared `sigsetjmp`). Only mapping
// creation/destruction lives here; the guard's VEH trapping is in
// `guard::os`. --

#[cfg(windows)]
type Handle = *mut std::ffi::c_void;
#[cfg(windows)]
type Dword = u32;

// Win32 API surface used by the mapping veneer and the VEH guard.
// `extern "system"` is the WinAPI calling convention; the CRT
// (`setjmp`/`longjmp`) uses `extern "C"`.
#[cfg(windows)]
#[allow(unsafe_code)]
unsafe extern "system" {
    fn CreateFileMappingW(
        h_file: Handle,
        lp_file_mapping_attributes: *mut std::ffi::c_void,
        fl_protect: Dword,
        dw_maximum_size_high: Dword,
        dw_maximum_size_low: Dword,
        lp_name: *const u16,
    ) -> Handle;
    fn MapViewOfFile(
        h_file_mapping_object: Handle,
        dw_desired_access: Dword,
        dw_file_offset_high: Dword,
        dw_file_offset_low: Dword,
        dw_number_of_bytes_to_map: usize,
    ) -> *mut std::ffi::c_void;
    fn UnmapViewOfFile(lp_base_address: *mut std::ffi::c_void) -> i32;
    fn CloseHandle(h_object: Handle) -> i32;
    fn AddVectoredExceptionHandler(
        first: u32,
        vectored_handler: *const std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
}

#[cfg(windows)]
const PAGE_READONLY: Dword = 0x02;
#[cfg(windows)]
const FILE_MAP_READ: Dword = 0x0004;

/// Create a read-only file mapping of `len` bytes of `handle` and map a
/// view of it. Returns the view base and the mapping handle (closed in
/// `raw_munmap`).
///
/// SAFETY: `handle` must be a valid open file handle, `len > 0`. The view
/// is valid until `raw_munmap`; the caller keeps `handle` open.
///
/// Note: the view size is `len`, not a page-rounded `map_len` — Win32
/// requires `dwNumberOfBytesToMap` to be within the mapping object (which
/// `CreateFileMappingW` sizes to the file); unlike unix `mmap`, a view
/// past the mapping size fails instead of faulting on access.
#[cfg(all(windows, not(miri)))]
#[allow(unsafe_code)]
fn raw_mmap(handle: Handle, len: usize) -> Result<(*mut std::ffi::c_void, Handle), std::io::Error> {
    unsafe {
        let mapping = CreateFileMappingW(
            handle,
            std::ptr::null_mut(),
            PAGE_READONLY,
            0,
            0,
            std::ptr::null(),
        );
        if mapping.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let ptr = MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, len);
        if ptr.is_null() {
            let e = std::io::Error::last_os_error();
            CloseHandle(mapping);
            return Err(e);
        }
        Ok((ptr, mapping))
    }
}

/// Unmap a view and close its file-mapping object.
///
/// SAFETY: `ptr`/`mapping` must be the exact values from `raw_mmap`, not
/// already unmapped/closed.
#[cfg(all(windows, not(miri)))]
#[allow(unsafe_code)]
fn raw_munmap(ptr: *mut std::ffi::c_void, mapping: Handle) {
    unsafe {
        UnmapViewOfFile(ptr);
        CloseHandle(mapping);
    }
}

/// Brand token for the guarded window. Minted only by `guard::guarded`
/// while the SIGBUS guard is armed, and handed to the read closure with an
/// invariant lifetime. A `&'brand [u8]` built under the brand cannot be
/// named outside the armed window: the closure's `for<'brand>` bound makes
/// returning it a compile error. `Copy` (it is a pure marker) so the
/// closure can hand it to both the slice builder and the inner closure.
/// Zero-sized; erases at codegen.
#[derive(Clone, Copy)]
pub struct Scope<'brand> {
    _brand: std::marker::PhantomData<&'brand mut &'brand ()>,
}

/// A validated range of mapped memory. Construction discharges the bounds
/// proof (`offset + len <= region length`) once, in `MmapRegion::range`;
/// slices are built from this only inside the armed window, so call sites
/// never re-derive bounds arithmetic from a raw pointer.
struct MappedRange {
    base: std::ptr::NonNull<u8>,
    len: usize,
}

impl MappedRange {
    /// Build the slice inside the armed window, under the guard's brand.
    /// The returned reference's lifetime is the brand, so the closure
    /// cannot name it outside the window — returning it is a compile
    /// error. This is the ONLY `from_raw_parts` over mapping memory left
    /// in the crate; it runs only while the guard is armed.
    ///
    /// SAFETY (unsafe leaf): `[base, base+len)` must lie within a live
    /// `PROT_READ` mapping — discharged by `MmapRegion::range` — and the
    /// guard must be armed over it — proved by the `Scope`, which only
    /// `guard::guarded` can mint.
    #[allow(unsafe_code)]
    fn slice<'brand>(&self, _scope: Scope<'brand>) -> &'brand [u8] {
        if self.len == 0 {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.base.as_ptr(), self.len) }
    }
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
/// - `sigsetjmp` carries no `returns_twice` metadata: the attribute is
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
///   `sigsetjmp` call and the end of `guarded`.
mod guard {
    use std::cell::Cell;
    #[cfg(all(unix, not(miri)))]
    use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
    #[cfg(not(miri))]
    use std::sync::Once;

    /// Opaque storage for the jump buffer. Unix: C `sigjmp_buf` (glibc
    /// needs 320 bytes on x86_64 and 432 on aarch64; FreeBSD amd64 needs
    /// 96; musl needs less; Darwin's layout is the largest of the set).
    /// Windows: the CRT `_JUMP_BUFFER` (256 bytes on x86_64). 1024 bytes
    /// at 16-byte alignment covers every target with margin. Rust never
    /// inspects the contents — only the setjmp/longjmp primitives touch
    /// it.
    #[repr(C, align(16))]
    struct JmpBuf([std::mem::MaybeUninit<u8>; 1024]);

    // `libc` does not expose these on this platform; declared directly.
    // Both are async-signal-safe per POSIX. The link name of the setjmp
    // half is per-libc: glibc's header defines `sigsetjmp` as a macro
    // expanding to `__sigsetjmp` and exports only that name; Darwin, the
    // BSDs, and musl export the plain POSIX name.
    // Under Miri these are never called (`guarded` is a pass-through there).
    #[cfg(all(unix, not(miri)))]
    #[allow(unsafe_code)]
    unsafe extern "C" {
        #[cfg_attr(all(target_os = "linux", target_env = "gnu"), link_name = "__sigsetjmp")]
        fn sigsetjmp(env: *mut JmpBuf, savemask: libc::c_int) -> libc::c_int;
        fn siglongjmp(env: *mut JmpBuf, val: libc::c_int) -> !;
    }

    // Windows CRT recovery primitives, declared directly (no new
    // dependency). `setjmp`/`longjmp` are async-signal-safe for the VEH
    // handler's purposes: the handler runs on the faulting thread, and
    // `longjmp` diverges without returning through the exception dispatcher.
    #[cfg(all(windows, not(miri)))]
    #[allow(unsafe_code)]
    unsafe extern "C" {
        fn setjmp(env: *mut JmpBuf) -> std::ffi::c_int;
        fn longjmp(env: *mut JmpBuf, val: std::ffi::c_int) -> !;
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
    #[cfg(all(unix, not(miri)))]
    #[allow(unsafe_code)]
    fn siginfo_fault_addr(info: *mut libc::siginfo_t) -> usize {
        unsafe { (*info).si_addr() as usize }
    }

    /// Jump back to the recovery point. Diverges.
    ///
    /// SAFETY: `env` must be a live `JmpBuf` on the calling thread's stack,
    /// previously initialized by `raw_setjmp`. The jump primitive is
    /// async-signal-safe.
    #[cfg(all(unix, not(miri)))]
    #[allow(unsafe_code)]
    fn jump_to_recovery(env: *mut JmpBuf) -> ! {
        unsafe { siglongjmp(env, 1) }
    }

    /// Jump back to the recovery point (Windows). Diverges.
    ///
    /// SAFETY: `env` must be a live `JmpBuf` on the calling thread's stack,
    /// previously initialized by `raw_setjmp`. The VEH handler runs on the
    /// faulting thread; `longjmp` diverges without returning through the
    /// exception dispatcher.
    #[cfg(all(windows, not(miri)))]
    #[allow(unsafe_code)]
    fn jump_to_recovery(env: *mut JmpBuf) -> ! {
        unsafe { longjmp(env, 1) }
    }

    /// Checkpoint the stack for recovery. Unix: `sigsetjmp` with
    /// `savemask = 1`; Windows: CRT `setjmp`.
    ///
    /// SAFETY: `env` must be a live `JmpBuf` stack slot on the calling
    /// thread, kept alive for the whole guarded window (see `guarded`).
    #[cfg(all(unix, not(miri)))]
    #[allow(unsafe_code)]
    fn raw_setjmp(env: *mut JmpBuf) -> libc::c_int {
        unsafe { sigsetjmp(env, 1) }
    }

    /// Checkpoint the stack for recovery (Windows: CRT `setjmp`).
    ///
    /// SAFETY: `env` must be a live `JmpBuf` stack slot on the calling
    /// thread, kept alive for the whole guarded window (see `guarded`).
    #[cfg(all(windows, not(miri)))]
    #[allow(unsafe_code)]
    fn raw_setjmp(env: *mut JmpBuf) -> std::ffi::c_int {
        unsafe { setjmp(env) }
    }

    /// Register `veh_handler` as the first vectored exception handler.
    ///
    /// SAFETY: `veh_handler` is a valid vectored handler function; the
    /// registration is process-lifetime and never removed.
    #[cfg(all(windows, not(miri)))]
    #[allow(unsafe_code)]
    fn add_veh_handler() -> *mut std::ffi::c_void {
        unsafe {
            super::AddVectoredExceptionHandler(
                1,
                veh_handler as *const () as *const std::ffi::c_void,
            )
        }
    }

    /// Reinterpret a captured handler address as a 3-arg `SA_SIGINFO` fn.
    ///
    /// SAFETY: `addr` must be the `sa_sigaction` captured from the previous
    /// `sigaction` disposition at install, which used `SA_SIGINFO`.
    #[cfg(all(unix, not(miri)))]
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
    #[cfg(all(unix, not(miri)))]
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
    #[cfg(all(unix, not(miri)))]
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
    #[cfg(all(unix, not(miri)))]
    #[allow(unsafe_code)]
    fn sigaction_zeroed() -> libc::sigaction {
        unsafe { std::mem::zeroed() }
    }

    /// Empty a signal set.
    ///
    /// SAFETY: `set` must point to a live `sigset_t`. `sigemptyset` is
    /// async-signal-safe.
    #[cfg(all(unix, not(miri)))]
    #[allow(unsafe_code)]
    fn sigemptyset(set: *mut libc::sigset_t) -> libc::c_int {
        unsafe { libc::sigemptyset(set) }
    }

    /// Install a `SIGBUS` disposition, capturing the previous one.
    ///
    /// SAFETY: `new` must be a fully initialized `sigaction`; `old` a live
    /// slot for the previous disposition. Called once at install.
    #[cfg(all(unix, not(miri)))]
    #[allow(unsafe_code)]
    fn sigaction_install(new: &libc::sigaction, old: &mut libc::sigaction) -> libc::c_int {
        unsafe { libc::sigaction(libc::SIGBUS, new, old) }
    }

    /// Query the current `SIGBUS` disposition without changing it.
    ///
    /// SAFETY: `old` must be a live slot for the disposition. Test-only.
    #[cfg(all(unix, test))]
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
    /// Atomics keep the handler free of `static mut`. Unix only; the
    /// Windows VEH chain needs no previous-disposition capture (foreign
    /// faults get `EXCEPTION_CONTINUE_SEARCH`, so the system's handlers run
    /// exactly as without us).
    #[cfg(all(unix, not(miri)))]
    static PREV_KIND: AtomicU8 = AtomicU8::new(0);
    #[cfg(all(unix, not(miri)))]
    static PREV_HANDLER: AtomicUsize = AtomicUsize::new(0);
    /// Whether the previous handler used `SA_SIGINFO` (3-arg) or is a plain
    /// 1-arg `sa_handler`. Only meaningful when `PREV_KIND == 2`.
    #[cfg(all(unix, not(miri)))]
    static PREV_SIGINFO: AtomicU8 = AtomicU8::new(0);

    /// Our `SIGBUS` handler (`SA_SIGINFO`).
    #[cfg(all(unix, not(miri)))]
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
    #[cfg(all(unix, not(miri)))]
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

    // -- Windows VEH fault trapping: the `EXCEPTION_IN_PAGE_ERROR`
    // (0xC0000006) counterpart of the unix SIGBUS handler. A vectored
    // exception handler registered once for the process; it checks the
    // fault address against the armed range exactly like `on_sigbus` and
    // recovers via the CRT `longjmp`, with the same contract. --

    /// Win32 `EXCEPTION_RECORD` (only the fields we read).
    #[cfg(all(windows, not(miri)))]
    #[repr(C)]
    struct ExceptionRecord {
        exception_code: u32,
        exception_flags: u32,
        exception_record: *mut ExceptionRecord,
        exception_address: *mut std::ffi::c_void,
        number_parameters: u32,
        exception_information: [usize; 15],
    }

    /// Win32 `EXCEPTION_POINTERS` (context record is opaque to us).
    #[cfg(all(windows, not(miri)))]
    #[repr(C)]
    struct ExceptionPointers {
        exception_record: *mut ExceptionRecord,
        context_record: *mut std::ffi::c_void,
    }

    #[cfg(all(windows, not(miri)))]
    const EXCEPTION_IN_PAGE_ERROR: u32 = 0xC000_0006;
    /// Tell the dispatcher to keep searching: foreign faults behave as if
    /// we were never installed (the `delegate` counterpart). Win32
    /// `EXCEPTION_CONTINUE_SEARCH` is 0; -1 is
    /// `EXCEPTION_CONTINUE_EXECUTION`, which resumes the excepting
    /// instruction and faults again.
    #[cfg(all(windows, not(miri)))]
    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

    /// Our vectored exception handler. Checks an in-page-error's fault
    /// address (`ExceptionInformation[1]`) against the armed range and
    /// `longjmp`s back on a match; every other exception continues the
    /// search. Runs on the faulting thread; async-signal-safe body (a
    /// thread-local read, integer arithmetic, `longjmp`).
    #[cfg(all(windows, not(miri)))]
    #[allow(unsafe_code)]
    unsafe extern "system" fn veh_handler(
        info: *mut ExceptionPointers,
    ) -> i32 {
        // SAFETY: the OS provides a valid `EXCEPTION_POINTERS` to a
        // vectored handler; we read only `ExceptionRecord` and its
        // `ExceptionInformation[1]` (the fault address for in-page errors).
        unsafe {
            let record = (*info).exception_record;
            if (*record).exception_code == EXCEPTION_IN_PAGE_ERROR {
                let fault_addr = (*record).exception_information[1];
                let ours = ACTIVE.try_with(|slot| {
                    let p = slot.get();
                    if p.is_null() {
                        return false;
                    }
                    let g = guard_from_raw(p);
                    // Branchless range check: `addr in [base, end)`.
                    if fault_addr.wrapping_sub(g.base) < g.end.wrapping_sub(g.base) {
                        jump_to_recovery(g.env);
                    }
                    false
                });
                // `jump_to_recovery` diverges; reaching here means the
                // fault is not inside our window.
                let _ = ours;
            }
            EXCEPTION_CONTINUE_SEARCH
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
        #[cfg(all(unix, not(miri)))]
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
        #[cfg(all(windows, not(miri)))]
        INSTALL.call_once(|| {
            // First handler in the VEH chain; foreign exceptions get
            // `EXCEPTION_CONTINUE_SEARCH`, so the system's handlers run
            // exactly as without us (no previous-disposition capture needed).
            let h = add_veh_handler();
            if h.is_null() {
                // A failed VEH install is a broken-OS situation; fail fast
                // rather than run unprotected.
                panic!("erax mmap: AddVectoredExceptionHandler failed");
            }
        });
    }

    /// Run `f` with `[base, end)` protected: a `SIGBUS` fault inside `f` at
    /// an address in `[base, end)` returns `Err(())` instead of killing the
    /// process. Faults elsewhere propagate to the previous disposition.
    ///
    /// `f` receives the arming `Scope` under a higher-ranked brand: the
    /// slice it reads is built inside this window (see `MappedRange`), and
    /// the brand makes returning that slice a compile error — the reference
    /// cannot outlive the armed window, by types rather than discipline.
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
    pub(super) fn guarded<T>(
        base: usize,
        end: usize,
        f: impl for<'brand> FnOnce(super::Scope<'brand>) -> T,
    ) -> Result<T, ()> {
        // `f` is wrapped in `ManuallyDrop`: on the fault path (recovery via
        // longjmp), the compiler cannot know `f` was already consumed by the
        // `FnOnce` call, so it would emit drop glue for `f`'s captures —
        // double-dropping them. We move it out via `into_inner` and call in
        // the armed branch, and never drop on the fault path (the storage
        // leaks, bounded and exceptional — leaking is safe, double-drop is
        // not).
        let f = std::mem::ManuallyDrop::new(f);
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
        // Unix `savemask = 1` (inside `raw_setjmp`): restore the signal mask
        // on recovery, so the SIGBUS blocked during handler execution
        // doesn't leak into normal execution.
        //
        // SAFETY: `env` is a live stack slot for the whole guarded window
        // (kept alive past the fault window by the `black_box` below); this
        // call executes in `guarded`'s own frame, which is where recovery
        // returns to.
        #[cfg(not(miri))]
        let jmp_val = raw_setjmp(&mut env as *mut JmpBuf);
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
            // The Scope is minted here, inside the armed window: the
            // closure's brand is this window's lifetime, unnameable
            // outside. `into_inner` moves `f` out of the `ManuallyDrop`
            // (safe: the wrapper is consumed); the `FnOnce` call consumes
            // it, dropping captures exactly once.
            let f = std::mem::ManuallyDrop::into_inner(f);
            let out = f(super::Scope {
                _brand: std::marker::PhantomData,
            });
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
            //
            // `f` is NOT dropped here: it was consumed by the `FnOnce` call
            // on the first pass (or never reached, if the fault preceded the
            // call — impossible in practice, but the `ManuallyDrop` makes it
            // safe either way). Dropping would double-drop captures.
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
            let r: Result<u32, ()> = guarded(0x1000, 0x2000, |_| 42);
            assert_eq!(r, Ok(42));
        }

        #[test]
        fn guard_restores_slot_after_use() {
            let _: Result<(), ()> = guarded(0x1000, 0x2000, |_| ());
            ACTIVE.with(|slot| assert!(slot.get().is_null(), "slot must be disarmed"));
        }

        #[test]
        fn guard_nesting_disarms_in_order() {
            let r: Result<u32, ()> = guarded(0x1000, 0x2000, |_| {
                let inner: Result<u32, ()> = guarded(0x3000, 0x4000, |_| 7);
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
            #[cfg(all(unix, not(miri)))]
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
    /// Windows only: the file-mapping object behind the view, closed in
    /// `Drop` (the unix mapping dies with `munmap` alone).
    #[cfg(windows)]
    _mapping: Handle,
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
                #[cfg(windows)]
                _mapping: std::ptr::null_mut(),
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
        #[cfg(all(unix, not(miri)))]
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
        #[cfg(all(windows, not(miri)))]
        {
            use std::os::windows::io::AsRawHandle;
            // The view covers exactly `len` (no page-rounding: Win32 views
            // cannot extend past the mapping object). `map_len` is the view
            // size, so the VEH guard range matches the actual mapping.
            let map_len = len as usize;
            // `len > 0`; the handle is valid and open. The view is valid
            // until `raw_munmap` in Drop; `File` (and the mapping object)
            // are kept open.
            let handle = file.as_raw_handle() as Handle;
            let (ptr, mapping) =
                raw_mmap(handle, map_len).map_err(|source| PagerError::Io { offset: 0, source })?;
            Ok(MmapRegion {
                ptr: ptr as *const u8,
                len,
                map_len,
                _file: file,
                _mapping: mapping,
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
    /// The closure receives the `Scope` brand and the slice under a
    /// higher-ranked lifetime: the slice is built inside the armed window
    /// (not before, as a previous shape did), and returning it from the
    /// closure is a compile error. The bounds proof is discharged once in
    /// `range`; the closure never sees a raw pointer.
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
        // faults and the `Err(())` arm below refreshes it.
        //
        // Stale-small is possible: the file may have regrown since the
        // shrink was observed. The fast path does one fresh fstat to
        // confirm; if the file regrew past `offset`, fall through to the
        // guarded read so `post_read_check` applies the `FileChanged` arm.
        // (Without this, a shrink→regrow leaves `Truncated` sticky
        // forever — a wrong error and a pager/mmap divergence, since the
        // pager revalidates with a fresh fstat.)
        if len > 0 && offset >= self.cached_len.get() {
            if let Ok(meta) = self._file.metadata() {
                let cur_len = meta.len();
                self.cached_len.set(cur_len);
                if offset < cur_len {
                    return Ok(());
                }
            }
            return Err(PagerError::Truncated {
                offset,
                wanted: len as u64,
            });
        }
        Ok(())
    }

    /// Post-read snapshot validation (one fstat). A read range extending
    /// beyond the current length means the pinned snapshot is gone there —
    /// `Truncated`, the pager's short-pread counterpart. (The guard catches
    /// shrinks that fault, but a shrink within the last mapped page serves
    /// stale bytes silently without this range check.) A pure shrink still
    /// serves its intact prefix byte-exact; only the gone region refuses.
    /// Same length + different mtime means the content was replaced
    /// (truncate+regrow or in-place rewrite) — the bytes just read cannot
    /// be trusted, discard them: `FileChanged`. A longer length is a
    /// tolerated append: reads are clamped to the pinned length, so the
    /// new tail is never read (semantic 1). The cache is refreshed as a
    /// side effect.
    ///
    /// Residuals (documented): a replacement preserving mtime, or one
    /// racing this fstat itself, is not detected — same as the pager.
    fn post_read_check(&self, offset: u64, len: usize) -> Result<(), PagerError> {
        if let Ok(meta) = self._file.metadata() {
            let cur_len = meta.len();
            self.cached_len.set(cur_len);
            if offset.saturating_add(len as u64) > cur_len {
                return Err(PagerError::Truncated {
                    offset,
                    wanted: len as u64,
                });
            }
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

    /// Validate `[offset, offset+len)` and wrap it as a `MappedRange`,
    /// discharging the bounds proof once. Call sites never re-derive
    /// bounds arithmetic from the raw pointer.
    fn range(&self, offset: u64, len: usize) -> Result<MappedRange, PagerError> {
        self.check_range(offset, len)?;
        if len == 0 {
            return Ok(MappedRange {
                base: std::ptr::NonNull::dangling(),
                len: 0,
            });
        }
        // `check_range` discharged `offset + len <= self.len`; `len > 0`
        // gives `self.len > 0`, so `ptr` is non-null (the null-ptr region is
        // empty-only, by construction), and `offset <= self.len - len`
        // bounds the add. Integer-to-pointer `as` casts are safe; the
        // dereference happens only in `MappedRange::slice`, under the guard.
        let addr = (self.ptr as usize).saturating_add(offset as usize);
        let base = std::ptr::NonNull::new(addr as *mut u8)
            .expect("non-null base for non-empty validated range");
        Ok(MappedRange { base, len })
    }

    pub fn read_guarded<T>(
        &self,
        offset: u64,
        len: usize,
        f: impl for<'brand> FnOnce(Scope<'brand>, &'brand [u8]) -> T,
    ) -> Result<T, PagerError> {
        let range = self.range(offset, len)?;
        let base = self.ptr as usize;
        // The slice is built INSIDE the armed window (`range.slice(scope)`),
        // and the brand keeps it there: `f` cannot return it.
        let out = guard::guarded(base, base + self.map_len, |scope| {
            f(scope, range.slice(scope))
        });
        match out {
            Ok(out) => {
                self.post_read_check(offset, len)?;
                Ok(out)
            }
            Err(()) => {
                // Fault on the mapping: the file shrank under the open
                // length. Refresh the cache so the fast path above
                // short-circuits subsequent reads.
                if let Ok(meta) = self._file.metadata() {
                    self.cached_len.set(meta.len());
                }
                Err(PagerError::Truncated {
                    offset,
                    wanted: len as u64,
                })
            }
        }
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
    /// Prefer this over `read_guarded(offset, len, |_, s| s.to_vec())` for
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
        let range = self.range(offset, len)?;
        let base = self.ptr as usize;
        // Reserved before arming: no allocation can occur inside the window
        // whose owner would be skipped by a fault.
        out.reserve(len);
        let copied: Result<(), ()> = guard::guarded(base, base + self.map_len, |scope| {
            out.extend_from_slice(range.slice(scope));
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
        if let Err(e) = self.post_read_check(offset, len) {
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
        #[cfg(all(unix, not(miri)))]
        if self.map_len > 0 {
            // `ptr`/`map_len` are the exact values from `raw_mmap`.
            raw_munmap(self.ptr as *mut libc::c_void, self.map_len);
        }
        #[cfg(all(windows, not(miri)))]
        if self.map_len > 0 {
            // `ptr`/`_mapping` are the exact values from `raw_mmap`.
            raw_munmap(self.ptr as *mut std::ffi::c_void, self._mapping);
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
        let all = region.read_guarded(0, 65536, |_, s| s.to_vec()).expect("read all");
        assert_eq!(all, content);
        let sub = region.read_guarded(1000, 5000, |_, s| s.to_vec()).expect("read sub");
        assert_eq!(sub, &content[1000..6000]);
        let one = region.read_guarded(65535, 1, |_, s| s[0]).expect("read one");
        assert_eq!(one, content[65535]);
        let empty: Vec<u8> = region.read_guarded(0, 0, |_, s| s.to_vec()).expect("read empty");
        assert!(empty.is_empty());
        // Pure-scan closure (the count_newlines_in path shape).
        let nls = region
            .read_guarded(0, 65536, |_, s| s.iter().filter(|&&b| b == b'\n').count())
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
        match region.read_guarded(3, 10, |_, s| s.to_vec()) {
            Err(PagerError::OutOfRange { offset, len, file_len }) => {
                assert_eq!((offset, len, file_len), (3, 10, 5));
            }
            other => panic!("expected OutOfRange, got {other:?}"),
        }
        // Offset arithmetic overflow.
        match region.read_guarded(u64::MAX, 8, |_, s| s.to_vec()) {
            Err(PagerError::OutOfRange { .. }) => {}
            other => panic!("expected OutOfRange on overflow, got {other:?}"),
        }
    }

    #[test]
    fn read_guarded_empty_file() {
        let (_g, file) = tmp_file("empty", b"");
        let region = MmapRegion::new(file).expect("map failed");
        assert!(region.is_empty());
        let empty: Vec<u8> = region.read_guarded(0, 0, |_, s| s.to_vec()).expect("empty read");
        assert!(empty.is_empty());
        match region.read_guarded(0, 1, |_, s| s.to_vec()) {
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
        region.read_guarded(0, 8192, |_, s| s.to_vec()).expect("baseline");
        // Truncate through a writable handle (the read-only mapped handle
        // cannot truncate; reopen by path — same inode). Windows refuses
        // the resize while the mapping is open (ERROR_USER_MAPPED_FILE),
        // so the state this test reaches on Unix cannot occur there; the
        // Windows branch pins the refusal and the intact region instead.
        #[cfg(unix)]
        {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&g.path)
                .expect("open for truncate")
                .set_len(100)
                .expect("set_len failed");
            match region.read_guarded(0, 8192, |_, s| s.to_vec()) {
                Err(PagerError::Truncated { offset, wanted }) => {
                    assert_eq!(offset, 0);
                    assert_eq!(wanted, 8192);
                }
                other => panic!("expected Truncated, got {other:?}"),
            }
            // The surviving prefix still reads.
            let prefix = region.read_guarded(0, 100, |_, s| s.to_vec()).expect("prefix read");
            assert_eq!(prefix, vec![0xABu8; 100]);
        }
        #[cfg(windows)]
        {
            let err = std::fs::OpenOptions::new()
                .write(true)
                .open(&g.path)
                .expect("open for truncate")
                .set_len(100)
                .expect_err("Windows must refuse to truncate a mapped file");
            assert_eq!(err.raw_os_error(), Some(1224));
            let full = region
                .read_guarded(0, 8192, |_, s| s.to_vec())
                .expect("region intact after refused truncate");
            assert_eq!(full, vec![0xABu8; 8192]);
        }
    }
}
