//! Memory-mapped files & zero-copy I/O.
//!
//! Wraps `memmap2` to map a file into memory so large PCAP / log files can be
//! inspected without loading them into RAM. Slices are returned as
//! `MmapHandle`-backed views (the Rak `Value::MmapSlice` keeps the mapping
//! alive), so indexing and pattern matching read directly from the mapped
//! pages with no copy.

use memmap2::{Mmap, MmapOptions};
use std::fs::File;
use std::sync::Arc;

/// A mapped file. `Ro` is a read-only mapping; `Rw` is a copy-on-write / writable
/// mapping. The `File` is kept alive for the mapping's lifetime.
pub struct MmapHandle {
    pub inner: MmapInner,
    pub _file: File,
}

pub enum MmapInner {
    Ro(Mmap),
    Rw(memmap2::MmapMut),
}

impl MmapHandle {
    /// Borrow the mapped bytes.
    pub fn as_slice(&self) -> &[u8] {
        match &self.inner {
            MmapInner::Ro(m) => m.as_ref(),
            MmapInner::Rw(m) => m.as_ref(),
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
        other => return Err(format!("mmap_open: unknown mode '{}', use \"r\" or \"rw\"", other)),
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
        MmapInner::Rw(m)
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
    h.as_slice()
        .windows(needle.len())
        .position(|w| w == needle)
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
}
