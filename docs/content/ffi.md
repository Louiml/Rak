# FFI & memory-mapped files

## Declarative C bindings

```rak
extern "C" {
    fn abs(n: i32) -> i32
}
dump abs(-42)                    // 42
```

Optional explicit library:

```rak
extern "C" from "libc.so.6" {
    fn getpid() -> i32
}
```

Signatures support `*T` pointer types, `void` returns, and `...` varargs
(best-effort marshalled by runtime type after the fixed params).

## Dynamic loader

```rak
let libc = ffi_load("libc.so.6") // or "ucrtbase.dll" / "libSystem.dylib"
dump libc.call("abs", [-9])      // 9 — args as a single Rak array
libc.close()

let p = ffi_ptr(0xDEADBEEF)      // raw pointer value
let buf = ffi_alloc(256)         // ffi-managed buffer
ffi_write(buf, 0, 0x41)
dump ffi_read(buf, 0)            // 0x41
dump ffi_cstr_to_string(buf)     // "A" (copies, NUL-terminated)
let cs = ffi_string_to_cstr("hi") // string -> NUL-terminated *i8
ffi_free(buf)
```

- `lib.call(symbol, args)` takes the args as a **single array** and returns an
  `i64` (untyped dynamic call). The declarative `extern "C"` form unmarshals
  per the declared return type.
- Strings passed to C are `CString`s owned by the call frame (freed on
  return). `ffi_string_to_cstr` returns an ffi-managed pointer freed with
  `ffi_free`.
- Errors: `ffi: symbol 'foo' not found: <os err>`, `ffi: cannot load '<lib>':
  <os err>`, `ffi: cannot marshal <type> to C` — never a panic.
- Pointers are opaque `u64`; dereferencing is explicit via
  `ffi_read`/`ffi_write`/`ffi_cstr_to_string`.

Works on both the interpreter and the bytecode VM (`Op::FFICall`/`Op::FFIClose`
natives; `extern` declarations are baked into `Value::NativeFn` globals).

## Memory-mapped files

Map huge PCAP / log files into memory and inspect them zero-copy.

```rak
let m = mmap_open("trace.pcap", "r")    // "r" | "rw"
dump mmap_size(m)
let hdr = mmap_slice(m, 0, 8)           // zero-copy view (keeps the mapping alive)
dump hdr[0]                             // first byte (zero-copy)
dump mmap_find(m, "\xff\xd8\xff")       // byte search -> offset (or -1)
for (off, len) in mmap_lines_off(m, "\n") {
    dump string(mmap_slice(m, off, len))
}
mmap_close(m)
```

- `mmap_slice` returns a zero-copy view; single-byte indexing reads directly
  from the mapped pages on both backends. Range slicing (`s[a..b]`) and binary
  pattern matching over slices are interpreter-only.
- `mmap_close` is a convenience — the `Arc<MmapHandle>` keeps the mapping alive
  until all slices drop, preventing use-after-unmap.
- Out-of-bounds: `mmap_slice: [off, off+len) = [...] out of range (len ...)`.
- A 4 GiB PCAP is mapped, not loaded — peak RAM is the OS page cache for
  touched pages plus slice metadata.

## C header bindgen

`rakc bindgen header.h -o bindings.rak` reads a C header and generates a Rak
file with a function per C function (calling `extern_call`) and a struct per C
struct. Pointers map to `u64`, `char*` to `string`, numeric types to
`i8`..`u64` and `f32`/`f64`. Build with `cargo build --release --features
bindgen`.
