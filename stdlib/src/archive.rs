//! Archive builtins for Rak: gzip (deflate) and zip read/write.
//!
//! Uses the existing `flate2` and `zip` crate dependencies. `zip_write`
//! is gated by the sandbox under the `fs_write` capability.

use std::io::Read;
use std::io::Write;

/// gzip-compress bytes (RFC 1952 container + deflate).
pub fn gzip_compress(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data).expect("gzip encode write");
    enc.finish().expect("gzip finish")
}

/// gzip-decompress bytes. Errors on truncated/corrupt streams.
pub fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut dec = flate2::read::GzDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)
        .map_err(|e| format!("gzip_decompress: {}", e))?;
    Ok(out)
}

/// One zip entry: (name, size, compressed_size).
pub fn zip_list(path: &str) -> Result<Vec<(String, u64, u64)>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("zip_list: cannot open '{}': {}", path, e))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("zip_list: bad zip '{}': {}", path, e))?;
    let mut out = Vec::new();
    for i in 0..z.len() {
        let f = z.by_index(i).map_err(|e| format!("zip_list: {}", e))?;
        out.push((f.name().to_string(), f.size(), f.compressed_size()));
    }
    Ok(out)
}

/// Read one entry's bytes out of a zip archive.
pub fn zip_read(path: &str, name: &str) -> Result<Vec<u8>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("zip_read: cannot open '{}': {}", path, e))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("zip_read: bad zip '{}': {}", path, e))?;
    let mut entry = z
        .by_name(name)
        .map_err(|e| format!("zip_read: no entry '{}' in '{}': {}", name, path, e))?;
    let mut out = Vec::new();
    entry
        .read_to_end(&mut out)
        .map_err(|e| format!("zip_read: {}", e))?;
    Ok(out)
}

/// Create (or overwrite) a zip with the given `(name, bytes)` entries.
/// Returns the total number of bytes written to disk.
pub fn zip_write(path: &str, entries: &[(String, Vec<u8>)]) -> Result<usize, String> {
    let f = std::fs::File::create(path).map_err(|e| format!("zip_write: cannot create '{}': {}", path, e))?;
    let mut w = zip::ZipWriter::new(f);
    let opts: zip::write::FileOptions = Default::default();
    for (name, data) in entries {
        w.start_file(name.as_str(), opts)
            .map_err(|e| format!("zip_write: entry '{}': {}", name, e))?;
        w.write_all(data)
            .map_err(|e| format!("zip_write: writing '{}': {}", name, e))?;
    }
    let file = w
        .finish()
        .map_err(|e| format!("zip_write: finish: {}", e))?;
    let written = file
        .metadata()
        .map(|m| m.len() as usize)
        .map_err(|e| format!("zip_write: stat: {}", e))?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gzip_roundtrip() {
        let data = b"the quick brown fox \x00\x01 jumps over 0x4D\xFF bytes".repeat(50);
        let packed = gzip_compress(&data);
        assert!(packed.len() < data.len()); // repetitive data compresses
        let unpacked = gzip_decompress(&packed).unwrap();
        assert_eq!(unpacked, data);
        assert!(gzip_decompress(b"not-gzip").is_err());
    }

    #[test]
    fn zip_roundtrip() {
        let dir = std::env::temp_dir().join("rak_zip_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.zip").to_string_lossy().to_string();
        let _ = std::fs::remove_file(&path);
        let written = zip_write(
            &path,
            &[
                ("a.txt".to_string(), b"hello".to_vec()),
                ("b.bin".to_string(), vec![0u8, 1, 2, 0xFF]),
            ],
        )
        .unwrap();
        assert!(written > 0);
        let list = zip_list(&path).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].0, "a.txt");
        assert_eq!(zip_read(&path, "a.txt").unwrap(), b"hello".to_vec());
        assert_eq!(zip_read(&path, "b.bin").unwrap(), vec![0u8, 1, 2, 0xFF]);
        assert!(zip_read(&path, "nope").is_err());
        let _ = std::fs::remove_file(&path);
    }
}

