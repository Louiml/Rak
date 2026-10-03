//! ## Pointer provenance
//!
//! `ffi_write` and `ffi_read` used to do this:
//!
//! ```ignore
//! unsafe { *((ptr as usize + off) as *mut u8) = byte }
//! ```
//!
//! with no check that `ptr` was ever allocated, and none that `off` was inside it.
//! Since `ffi_ptr(n)` builds a pointer from *any* integer, that made
//! `ffi_write(ffi_ptr(ADDRESS), 0, 0x41)` an arbitrary write anywhere the process
//! can reach, and `ffi_read` the matching read. `ffi_cstr_to_string` was worse in a
//! different way: it scanned for a NUL byte with no bound at all, so a buffer with
//! no NUL in it walked off the end into unmapped memory and killed the process --
//! reachable from a pointer that came out of a network response.
//!
//! A pointer is now *provenanced* if Rak allocated it (`ffi_alloc`,
//! `ffi_string_to_cstr`) or was told it owns a region (`ffi_trust`). Provenanced
//! pointers are range-checked on **every** access against the size recorded at
//! allocation; a pointer that is neither is refused rather than trusted.
//!
//! The escape hatch is deliberate. A language with FFI that refused every foreign
//! address would be unusable, and resolving a symbol then calling it is legitimate.
//! What is not acceptable is an address that is *silently* accepted, because that
//! is indistinguishable from a safety check that always passes. So `ffi_trust` turns
//! "unchecked pointer arithmetic" into an assertion you have to write down -- the
//! same bargain `unsafe { reason }` makes everywhere else in Rak.

//! Foreign Function Interface — dynamic loader and low-level call trampolines.
//!
//! This module wraps `libloading` (a cross-platform `dlopen`/`LoadLibrary`
//! abstraction) and provides two low-level call trampolines that dispatch a
//! foreign function pointer with integer/pointer arguments passed as `u64`
//! bit patterns.
//!
//! The Rak value↔C marshalling lives in the `rakc` interpreter and VM (they
//! each have their own `Value` enum); this module stays Rak-agnostic so both
//! backends share it.

use libloading::Library;
use std::collections::HashMap;
use std::sync::Mutex;
use std::os::raw::c_void;

/// A loaded shared library handle. Dropping it calls `dlclose`/`FreeLibrary`.
pub struct LibHandle {
    pub lib: Library,
}

/// Load a shared library by path or SONAME (`libc.so.6`, `msvcrt.dll`,
/// `libSystem.dylib`).
pub fn load(path: &str) -> Result<LibHandle, String> {
    unsafe {
        Library::new(path)
            .map(|lib| LibHandle { lib })
            .map_err(|e| format!("ffi: cannot load '{}': {}", path, e))
    }
}

/// Resolve a symbol to a raw address. Returns an OS error message on failure.
pub fn sym_addr(h: &LibHandle, name: &str) -> Result<usize, String> {
    unsafe {
        h.lib
            .get::<*mut c_void>(name.as_bytes())
            .map(|s| *s as usize)
            .map_err(|e| format!("ffi: symbol '{}' not found: {}", name, e))
    }
}

/// Platform default C library names to try, in order. On Windows we prefer
/// the UCRT, then the legacy `msvcrt.dll`, then `kernel32.dll` (which exports
/// `GetTickCount`-style helpers) so a basic `extern "C"` block resolves.
pub fn default_lib_names() -> &'static [&'static str] {
    #[cfg(target_os = "linux")]
    {
        &["libc.so.6", "libc.so.7", "libc.so"]
    }
    #[cfg(target_os = "macos")]
    {
        &["libSystem.dylib", "libc.dylib"]
    }
    #[cfg(target_os = "windows")]
    {
        &["ucrtbase.dll", "msvcrt.dll", "kernel32.dll", "ntdll.dll"]
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        &["libc.so"]
    }
}

/// Load the first available platform default C library.
pub fn load_default() -> Result<LibHandle, String> {
    let mut last = String::new();
    for name in default_lib_names() {
        match load(name) {
            Ok(h) => return Ok(h),
            Err(e) => last = e,
        }
    }
    Err(format!("ffi: cannot load a default C library: {}", last))
}

/// Call a foreign function at `addr` with integer/pointer arguments. Each
/// argument is a `u64` bit pattern (ints use their two's-complement bits;
/// pointers are the raw address; strings are the address of a NUL-terminated
/// buffer). Up to 8 arguments are supported; missing slots are zero-filled.
/// The result is returned in the integer/pointer return register (rax/eax).
///
/// # Safety
/// FFI is inherently unsafe. The caller must ensure `addr` is a valid C
/// function pointer and that the argument count and bit-pattern kinds match
/// the real C signature (integer/pointer args only). Float arguments are not
/// supported by this trampoline — they require `libffi` (future work).
pub unsafe fn call_int(addr: usize, args: &[u64]) -> u64 {
    type Fn8 = extern "C" fn(u64, u64, u64, u64, u64, u64, u64, u64) -> u64;
    let f: Fn8 = std::mem::transmute(addr);
    let mut a = [0u64; 8];
    for (i, v) in args.iter().enumerate().take(8) {
        a[i] = *v;
    }
    f(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7])
}

/// Like [`call_int`] but the foreign function returns an `f64` (result lands in
/// `xmm0`). Arguments are still integer/pointer (float *arguments* are not
/// supported without `libffi`).
///
/// # Safety
/// See [`call_int`].
pub unsafe fn call_float(addr: usize, args: &[u64]) -> f64 {
    type Fn8F = extern "C" fn(u64, u64, u64, u64, u64, u64, u64, u64) -> f64;
    let f: Fn8F = std::mem::transmute(addr);
    let mut a = [0u64; 8];
    for (i, v) in args.iter().enumerate().take(8) {
        a[i] = *v;
    }
    f(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_default_c_library() {
        // At least one platform default C library must load.
        assert!(load_default().is_ok());
    }

    #[test]
    fn sym_addr_resolves_a_known_symbol() {
        let h = load_default().expect("default libc");
        // `strlen`/`rand`/`abs` exist on all three platforms' C runtimes.
        let resolved =
            sym_addr(&h, "strlen").or_else(|_| sym_addr(&h, "rand")).or_else(|_| sym_addr(&h, "abs"));
        assert!(resolved.is_ok(), "expected at least one common C symbol to resolve");
    }
}

/// Hard ceiling on a C-string scan.
///
/// A legitimate `char*` is NUL-terminated; one longer than this is a pointer
/// mistake, and reading it costs a segfault rather than an error.
pub const MAX_CSTR: usize = 1024 * 1024;

/// The regions Rak is allowed to touch through a raw pointer.
///
/// Shared by both backends so the bounds logic lives in one tested place rather
/// than being re-implemented at four call sites per backend.
#[derive(Debug, Default)]
pub struct Allocations {
    /// Address -> length. Populated by `ffi_alloc`, `ffi_string_to_cstr` and
    /// `ffi_trust`.
    regions: Mutex<HashMap<u64, usize>>,
}

impl Allocations {
    pub fn new() -> Allocations {
        Allocations::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, usize>> {
        // A poisoned lock means another thread panicked while recording an
        // allocation. The map is still structurally sound, so recover rather than
        // turning one panic into a cascade.
        self.regions.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Record that `ptr` owns `len` bytes.
    pub fn record(&self, ptr: u64, len: usize) {
        self.lock().insert(ptr, len);
    }

    /// Forget an allocation, returning its length. `None` if it was not ours.
    pub fn release(&self, ptr: u64) -> Option<usize> {
        self.lock().remove(&ptr)
    }

    /// Is `ptr` a base Rak knows about? Provenance is exact, not a range.
    pub fn is_known(&self, ptr: u64) -> bool {
        self.lock().contains_key(&ptr)
    }

    /// Check that `[ptr+off, ptr+off+len)` lies inside a region Rak owns.
    ///
    /// The arithmetic is done in `u128` on purpose: `ptr + off` in `u64` wraps, and
    /// a bounds check that a large offset can defeat is not a check.
    pub fn check(&self, ptr: u64, off: u64, len: u64) -> Result<(), String> {
        let Some(&size) = self.lock().get(&ptr) else {
            return Err(format!(
                "pointer {ptr:#x} is not a region Rak owns: it did not come from \
                 ffi_alloc or ffi_string_to_cstr. If it is a library-owned address, \
                 declare it with ffi_trust({ptr:#x}, <length>); until then every read \
                 and write through it is refused."
            ));
        };
        if (off as u128) + (len as u128) > size as u128 {
            return Err(format!(
                "access of {len} byte(s) at {ptr:#x}+{off:#x} runs past the end of a \
                 {size}-byte allocation"
            ));
        }
        Ok(())
    }

    /// How far a NUL-terminated scan at `ptr + off` may go.
    ///
    /// Refuses an unknown pointer rather than scanning blind.
    pub fn cstr_cap(&self, ptr: u64, off: u64) -> Result<usize, String> {
        let Some(&size) = self.lock().get(&ptr) else {
            return Err(format!(
                "pointer {ptr:#x} is not a region Rak owns; use ffi_trust to declare \
                 a library-owned address first"
            ));
        };
        Ok(((size as u128).saturating_sub(off as u128)).min(MAX_CSTR as u128) as usize)
    }
}

#[cfg(test)]
mod provenance_tests {
    use super::*;

    #[test]
    fn an_unknown_pointer_is_refused_by_name() {
        let a = Allocations::new();
        let err = a.check(0x1000, 0, 1).unwrap_err();
        assert!(err.contains("not a region Rak owns"), "{}", err);
        assert!(err.contains("ffi_trust"), "it should say what to do: {}", err);
    }

    #[test]
    fn an_in_bounds_access_is_allowed() {
        let a = Allocations::new();
        a.record(0x1000, 16);
        assert!(a.check(0x1000, 0, 16).is_ok(), "the whole allocation");
        assert!(a.check(0x1000, 15, 1).is_ok(), "the last byte");
        assert!(a.check(0x1000, 8, 8).is_ok(), "the middle");
    }

    #[test]
    fn an_out_of_bounds_access_is_refused() {
        let a = Allocations::new();
        a.record(0x1000, 16);
        assert!(a.check(0x1000, 16, 1).is_err(), "one past the end");
        assert!(a.check(0x1000, 0, 17).is_err(), "a read spanning the end");
        let err = a.check(0x1000, 15, 2).unwrap_err();
        assert!(err.contains("runs past the end"), "{}", err);
        assert!(err.contains("16-byte"), "it should state the size: {}", err);
    }

    #[test]
    fn an_overflowing_offset_cannot_defeat_the_check() {
        // In u64 these wrap to small numbers and the check would pass.
        let a = Allocations::new();
        a.record(0x1000, 16);
        assert!(a.check(0x1000, u64::MAX, 1).is_err(), "max offset");
        assert!(a.check(0x1000, u64::MAX - 3, 8).is_err(), "near-max offset");
        assert!(a.check(0x1000, u64::MAX / 2, 4).is_err(), "half-max offset");
    }

    #[test]
    fn a_zero_length_region_allows_nothing() {
        let a = Allocations::new();
        a.record(0x2000, 0);
        assert!(a.check(0x2000, 0, 1).is_err());
    }

    #[test]
    fn release_reports_whether_the_pointer_was_ours() {
        let a = Allocations::new();
        a.record(0x3000, 8);
        assert_eq!(a.release(0x3000), Some(8));
        assert_eq!(a.release(0x3000), None, "a double free is not silently ok");
        assert!(a.check(0x3000, 0, 1).is_err(), "freed memory is not ours again");
    }

    #[test]
    fn a_cstr_scan_is_bounded_by_the_allocation() {
        let a = Allocations::new();
        a.record(0x4000, 8);
        assert_eq!(a.cstr_cap(0x4000, 0).unwrap(), 8, "at most the allocation");
        assert_eq!(a.cstr_cap(0x4000, 4).unwrap(), 4, "offset reduces the budget");
        assert!(a.cstr_cap(0x9999, 0).is_err(), "an unknown pointer is refused");
    }

    #[test]
    fn provenance_is_exact_not_a_range() {
        let a = Allocations::new();
        a.record(0x6000, 32);
        assert!(a.is_known(0x6000));
        assert!(!a.is_known(0x6001), "an interior address is not the base");
        assert!(a.check(0x6001, 0, 1).is_err());
    }
}
