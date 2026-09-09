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
