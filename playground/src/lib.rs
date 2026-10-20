//! Rak in the browser.
//!
//! The docs site has a playground: a text box, a Run button, and a console.
//! This crate is what makes the Run button work. It compiles Rak's *real*
//! interpreter to WebAssembly, so a program that runs in the playground is a
//! program that runs under `rakc` — same lexer, same parser, same evaluator,
//! same output. Nothing is re-implemented here.
//!
//! # The ABI
//!
//! WebAssembly has no strings, so the boundary is deliberately dull. Everything
//! is a *linear-memory offset*, which is what a `usize` is on wasm32:
//!
//! - `alloc(len)` reserves room for the program and returns the offset. The
//!   caller writes the program there.
//! - `run(ptr, len)` runs it and returns the length of the output. The output
//!   bytes are at `out_ptr()`.
//! - `free(ptr, len)` gives the buffer back.
//!
//! JavaScript reads the numbers as unsigned (`>>> 0`) and re-reads
//! `memory.buffer` after every call, because a growing memory detaches the
//! old `ArrayBuffer` view.
//!
//! # What is not here
//!
//! The build has no `http` (the TLS stack cannot be compiled to wasm) and no
//! `async` (a page has no event loop). The builtins that need them are not
//! registered, so a program that calls one gets "Unknown function" — the same
//! answer the interpreter gives for a name it does not have.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Fill `dest` with random bytes, from the browser.
///
/// `getrandom` has no wasm backend of its own that does not drag in
/// `wasm-bindgen`, so the `custom` backend is used instead: this function is
/// registered with it, and the bytes come from JavaScript's
/// `crypto.getRandomValues`.
///
/// The import lives in the `rak` module, which the page supplies. The return
/// value is zero for success and non-zero for failure, so the caller can tell
/// the difference between "the browser refused" and "it worked".
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn browser_random(dest: &mut [u8]) -> Result<(), getrandom::Error> {
    #[link(wasm_import_module = "rak")]
    extern "C" {
        fn random_bytes(dest: *mut u8, len: usize) -> i32;
    }
    let rc = unsafe { random_bytes(dest.as_mut_ptr(), dest.len()) };
    if rc == 0 {
        Ok(())
    } else {
        Err(getrandom::Error::UNSUPPORTED)
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
getrandom::register_custom_getrandom!(browser_random);

/// The buffer the program text is written into.
///
/// A fixed static rather than a heap allocation, so the offset handed to
/// JavaScript is an index into linear memory rather than a host address.
static mut PROGRAM: [u8; PROGRAM_CAPACITY] = [0; PROGRAM_CAPACITY];
const PROGRAM_CAPACITY: usize = 256 * 1024;

/// The buffer the output is written into.
static mut OUTPUT: [u8; OUTPUT_CAPACITY] = [0; OUTPUT_CAPACITY];
const OUTPUT_CAPACITY: usize = 512 * 1024;

/// The length of the last run's output.
static OUTPUT_LEN: AtomicUsize = AtomicUsize::new(0);

/// Reserve `len` bytes and return their offset in the module's memory.
///
/// The program text goes into a fixed region, so the offset is always zero; the
/// function exists so the calling convention reads like a conventional
/// allocator. The *address* of that region is `program_ptr()`, because the
/// offset and the address are different things: the caller has to write at
/// `program_ptr() + offset`.
///
/// The export is wasm-only. `#[no_mangle]` on a native build would put a
/// symbol named `alloc` into the binary, and `free` below collided with the C
/// runtime's `free` — the test binary would not link at all. The functions
/// exist on every target, because the tests call them; only the *exported
/// symbol* is wasm-only, which is what the page actually needs.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub extern "C" fn alloc(len: usize) -> usize {
    let _ = len;
    0
}

/// The address in the module's memory where the program text belongs.
///
/// The caller writes the program here. It is not the same as the offset
/// `alloc` returns, because a wasm address is a location in linear memory while
/// the offset is relative to this region.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub extern "C" fn program_ptr() -> usize {
    std::ptr::addr_of!(PROGRAM).cast::<u8>() as usize
}

/// Release a buffer previously handed out by [`alloc`].
///
/// A no-op: the program text lives in a static, so there is nothing to free.
///
/// The export is wasm-only; see the note on [`alloc`].
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub extern "C" fn free(_ptr: usize, _len: usize) {}

/// Run a program; returns the length of the output.
///
/// The output bytes are at [`out_ptr`]. They stay valid until the next `run`.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub extern "C" fn run(source_ptr: usize, source_len: usize) -> usize {
    let source = if source_len == 0 {
        String::new()
    } else {
        let len = source_len.min(PROGRAM_CAPACITY);
        let base = std::ptr::addr_of!(PROGRAM).cast::<u8>() as usize;
        let bytes = unsafe { std::slice::from_raw_parts((base + source_ptr) as *const u8, len) };
        String::from_utf8_lossy(bytes).into_owned()
    };

    let out = run_source(&source);
    let len = out.len().min(OUTPUT_CAPACITY);
    unsafe {
        let dst = &mut *std::ptr::addr_of_mut!(OUTPUT);
        dst[..len].copy_from_slice(&out[..len]);
    }
    OUTPUT_LEN.store(len, Ordering::SeqCst);
    len
}

/// The offset of the last run's output in the module's memory.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub extern "C" fn out_ptr() -> usize {
    std::ptr::addr_of!(OUTPUT).cast::<u8>() as usize
}

/// The length of the last run's output.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub extern "C" fn out_len() -> usize {
    OUTPUT_LEN.load(Ordering::SeqCst)
}

/// The version of Rak the playground is running, for the site to display.
///
/// A static NUL-terminated string, so JavaScript can read it as a C string.
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub extern "C" fn rak_version() -> usize {
    c"1.1.0".as_ptr() as usize
}

/// Run a program, returning the bytes to show in the console.
///
/// The output is the program's own, then a status line. On success the status
/// is `exit 0`; on failure it is the error the interpreter produced, with the
/// same wording `rakc run` prints.
fn run_source(source: &str) -> Vec<u8> {
    let mut interp = rakc::interpreter::Interpreter::with_base_dir(".".to_string());
    // A depth cap, for the same reason the CLI has one: a script from a
    // stranger should not be able to exhaust the browser's stack.
    interp.set_max_depth(rakc::interpreter::DEFAULT_MAX_DEPTH);

    let mut text = String::new();
    match interp.run_source(source) {
        Ok(_) => {
            for line in interp.take_output() {
                text.push_str(&line);
                text.push('\n');
            }
            text.push_str("exit 0");
        }
        Err(e) => {
            // The program's output is still worth showing: a program that
            // printed three lines and then raised has said something.
            for line in interp.take_output() {
                text.push_str(&line);
                text.push('\n');
            }
            text.push_str(&format!("error: {}", e));
        }
    }
    text.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_str(src: &str) -> String {
        let bytes = run_source(src);
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn runs_a_program_and_reports_its_output() {
        let out = run_str("dump 1 + 1");
        assert!(out.contains("2"), "got: {}", out);
        assert!(out.contains("exit 0"), "got: {}", out);
    }

    #[test]
    fn a_failing_program_reports_the_error() {
        let out = run_str("raise \"boom\"");
        assert!(out.contains("boom"), "got: {}", out);
    }

    #[test]
    fn a_syntax_error_is_reported_not_panicked() {
        let out = run_str("fn ( {");
        assert!(out.contains("error"), "got: {}", out);
    }

    #[test]
    fn the_abi_round_trips_a_program() {
        let src = b"dump \"hi\"";
        let ptr = alloc(src.len());
        unsafe {
            let base = std::ptr::addr_of!(PROGRAM).cast::<u8>() as usize;
            std::ptr::copy_nonoverlapping(src.as_ptr(), (base + ptr) as *mut u8, src.len());
        }
        let len = run(ptr, src.len());
        let out_ptr = out_ptr();
        let out = unsafe { std::slice::from_raw_parts(out_ptr as *const u8, len) };
        assert!(
            String::from_utf8_lossy(out).contains("hi"),
            "got: {:?}",
            out
        );
        assert_eq!(out_len(), len);
        free(ptr, src.len());
    }
}
