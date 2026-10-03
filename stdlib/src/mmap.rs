//! Memory-mapped files & zero-copy I/O.
//!
//! Wraps `memmap2` to map a file into memory so large PCAP / log files can be
//! inspected without loading them into RAM. Slices are returned as
//! `MmapHandle`-backed views (the Rak `Value::MmapSlice` keeps the mapping
//! alive), so indexing and pattern matching read directly from the mapped
//! pages with no copy.

use memmap2::{Mmap, MmapOptions};
use std::cell::UnsafeCell;
use std::fs::File;
use std::sync::Arc;

/// A mapped file. `Ro` is a read-only mapping; `Rw` is a writable one. The
/// `File` is kept alive for the mapping's lifetime.
pub struct MmapHandle {
    pub inner: MmapInner,
    pub _file: File,
}

pub enum MmapInner {
    Ro(Mmap),
    Rw(WritableMapping),
}

/// A writable mapping that can also be read through `&self`.
///
/// `MmapMut`'s write path is `IndexMut`, which needs `&mut MmapMut`, and this
/// crate hands out `Arc<MmapHandle>` — a shared reference, because a `bytes` view
/// into the mapping has to be able to outlive the expression that made it. memmap2
/// 0.9 offers no `&self` write accessor (`as_ptr_mut` arrived later), so the
/// interior mutability lives here rather than at twenty-odd call sites.
///
/// SAFETY, for a caller of this type: mutation must not overlap a read of the same
/// bytes. In Rak that holds because a builtin runs to completion and the mapping is
/// not shared across threads without the caller's own synchronisation — the handle
/// is `Send + Sync`, so a Rak program *could* move it into a thread and write while
/// another read the same offset, and the compiler would not stop it. That is the
/// same hazard `Arc<Mutex<..>>` exists to prevent, and the honest description is
/// that a writable mapping is a single-owner resource that Rak does not yet enforce.
/// Reads are unaffected and remain safe.
pub struct WritableMapping(UnsafeCell<memmap2::MmapMut>);

// SAFETY: see the type's doc comment. `MmapMut` is itself `Send`/`Sync`-able for
// the same reason, and the mapping is not mutated concurrently by Rak's own code.
unsafe impl Send for WritableMapping {}
unsafe impl Sync for WritableMapping {}

impl WritableMapping {
    fn new(m: memmap2::MmapMut) -> Self {
        WritableMapping(UnsafeCell::new(m))
    }

    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: as documented on the type. `MmapMut` derefs to `[u8]`, and the
        // reference lives only as long as `&self`; nothing hands out a `&mut` to
        // the mapping while it is alive.
        unsafe { &*self.0.get() }
    }

    /// Store one byte. `off` is bounds-checked by the caller.
    fn write_byte(&self, off: usize, byte: u8) {
        // SAFETY: as `as_slice` above, plus `off < len`, checked before the call.
        unsafe {
            let base = (*self.0.get()).as_mut_ptr();
            std::ptr::write(base.add(off), byte);
        }
    }
}

impl MmapHandle {
    /// Borrow the mapped bytes.
    pub fn as_slice(&self) -> &[u8] {
        match &self.inner {
            MmapInner::Ro(m) => m.as_ref(),
            MmapInner::Rw(m) => m.as_slice(),
        }
    }

    pub fn len(&self) -> usize {
        self.as_slice().len()
    }
}

/// Open and map a file. `mode` is `"r"` (read-only), `"rw"` (read+write,
/// requires the file to be writable), or `"rw_new"` (create/truncate to the
/// given size — not yet supported, falls back to `rw`).
pub fn open(path: &str, mode: &str) -> Result<Arc<MmapHandle>, String> {
    let writable = match mode {
        "r" => false,
        "rw" | "rw_new" => true,
        other => {
            return Err(format!(
                "mmap_open: unknown mode '{}', use \"r\" or \"rw\"",
                other
            ))
        }
    };
    let file = if writable {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| format!("mmap_open: cannot open '{}': {}", path, e))?
    } else {
        File::open(path).map_err(|e| format!("mmap_open: cannot open '{}': {}", path, e))?
    };
    let inner = if writable {
        let m = unsafe { MmapOptions::new().map_mut(&file) }
            .map_err(|e| format!("mmap_open: cannot map '{}': {}", path, e))?;
        MmapInner::Rw(WritableMapping::new(m))
    } else {
        let m = unsafe { Mmap::map(&file) }
            .map_err(|e| format!("mmap_open: cannot map '{}': {}", path, e))?;
        MmapInner::Ro(m)
    };
    Ok(Arc::new(MmapHandle { inner, _file: file }))
}

/// Total mapped length in bytes.
pub fn size(h: &MmapHandle) -> usize {
    h.len()
}

/// Search for `needle` in the mapped region, returning the byte offset of the
/// first match or `None`.
pub fn find(h: &MmapHandle, needle: &[u8]) -> Option<usize> {
    h.as_slice().windows(needle.len()).position(|w| w == needle)
}

/// Write one byte through a writable mapping.
///
/// Only a `"rw"` mapping accepts this; a read-only one is refused by name rather
/// than by letting the OS fault. The bounds check happens *before* anything is
/// written, so a rejected write leaves the mapping exactly as it was.
///
/// `MmapMut`'s write path (`IndexMut`) needs `&mut MmapMut`, which a handle behind
/// an `Arc` cannot produce — every read path in this crate works through `&self`.
/// `as_ptr_mut` is memmap2's own safe accessor for exactly that situation.
pub fn write_byte(h: &MmapHandle, off: usize, byte: u8) -> Result<(), String> {
    match &h.inner {
        // No builtin name in the message: the VM wraps native errors as
        // `<builtin>: <msg>` and an already-prefixed message reads as
        // `mmap_write: mmap_write: ...`.
        MmapInner::Ro(_) => {
            Err("the mapping is read-only; open it with mmap_open(path, \"rw\")".to_string())
        }
        MmapInner::Rw(m) => {
            if off >= h.len() {
                return Err(format!(
                    "offset {} is out of range (mapping is {} bytes)",
                    off,
                    h.len()
                ));
            }
            m.write_byte(off, byte);
            Ok(())
        }
    }
}

/// Overwrite a run of bytes. All-or-nothing: the range is checked before the first
/// write, so a request that runs off the end does not leave the file partly
/// modified.
pub fn write_bytes_at(h: &MmapHandle, off: usize, data: &[u8]) -> Result<(), String> {
    if data.len() > h.len().saturating_sub(off) {
        return Err(format!(
            "{} bytes at offset {} runs past the end of a {} byte mapping",
            data.len(),
            off,
            h.len()
        ));
    }
    for (i, b) in data.iter().enumerate() {
        write_byte(h, off + i, *b)?;
    }
    Ok(())
}

/// Iterate line offsets/lengths by splitting on `delim` (default `"\n"`).
/// Returns `(offset, length)` pairs so callers can read lines zero-copy.
pub fn lines_off(h: &MmapHandle, delim: &[u8]) -> Vec<(usize, usize)> {
    let data = h.as_slice();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i + delim.len() <= data.len() {
        if &data[i..i + delim.len()] == delim {
            out.push((start, i - start));
            start = i + delim.len();
            i = start;
        } else {
            i += 1;
        }
    }
    if start < data.len() {
        out.push((start, data.len() - start));
    }
    out
}

/// Materialise lines as owned `String`s (a copy per line is unavoidable for
/// safe iteration). Splits on `delim` (default `"\n"`).
pub fn lines(h: &MmapHandle, delim: &[u8]) -> Vec<String> {
    lines_off(h, delim)
        .into_iter()
        .map(|(off, len)| String::from_utf8_lossy(&h.as_slice()[off..off + len]).into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn mmap_open_slice_find_lines() {
        let dir = std::env::temp_dir();
        let path = dir.join("rak_mmap_test.bin");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"PNG\x89\nline2\nline3").unwrap();
        }
        let h = open(path.to_str().unwrap(), "r").unwrap();
        assert_eq!(size(&h), 16);
        assert_eq!(find(&h, b"line2"), Some(5));
        let ls = lines(&h, b"\n");
        assert_eq!(ls[0], String::from_utf8_lossy(b"PNG\x89"));
        assert_eq!(ls[1], "line2");
        assert_eq!(ls[2], "line3");
        let offs = lines_off(&h, b"\n");
        assert_eq!(offs[0], (0, 4));
        assert_eq!(offs[1], (5, 5));
        let _ = std::fs::remove_file(&path);
    }

    /// A writable mapping round-trips a byte back to disk.
    ///
    /// `flush` is not called: on both platforms the mapping is the file, and a
    /// fresh read of the file observes the write. Flushing explicitly would make
    /// the test depend on a syscall the editor does not need to make.
    #[test]
    fn mmap_write_round_trips_to_disk() {
        let path = std::env::temp_dir().join("rak_mmap_write_test.bin");
        std::fs::write(&path, b"\x00\x01\x02\x03\x04").unwrap();

        {
            let h = open(path.to_str().unwrap(), "rw").unwrap();
            write_byte(&h, 1, 0xFF).unwrap();
            write_bytes_at(&h, 3, b"\xAA\xBB").unwrap();
            // And through the read path, so a write is not invisible to `as_slice`.
            assert_eq!(h.as_slice()[1], 0xFF);
            assert_eq!(&h.as_slice()[3..5], b"\xAA\xBB");
        }

        assert_eq!(std::fs::read(&path).unwrap(), b"\x00\xFF\x02\xAA\xBB");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mmap_write_refuses_a_read_only_mapping() {
        let path = std::env::temp_dir().join("rak_mmap_ro_test.bin");
        std::fs::write(&path, b"\x00\x01").unwrap();
        let h = open(path.to_str().unwrap(), "r").unwrap();
        let err = write_byte(&h, 0, 0xFF).unwrap_err();
        assert!(err.contains("read-only"), "got: {}", err);
        // Nothing was written on the way to refusing.
        assert_eq!(std::fs::read(&path).unwrap(), b"\x00\x01");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mmap_write_out_of_range_changes_nothing() {
        let path = std::env::temp_dir().join("rak_mmap_range_test.bin");
        std::fs::write(&path, b"\x00\x01").unwrap();
        {
            let h = open(path.to_str().unwrap(), "rw").unwrap();
            assert!(write_byte(&h, 2, 0xFF).is_err());
            assert!(write_bytes_at(&h, 1, b"\xFF\xFF").is_err());
            // The rejected multi-byte write must not have applied its first byte.
            assert_eq!(h.as_slice()[1], 0x01);
        }
        assert_eq!(std::fs::read(&path).unwrap(), b"\x00\x01");
        let _ = std::fs::remove_file(&path);
    }
}
