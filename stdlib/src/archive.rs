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

/// Default ceiling on decompression output: 100 MiB.
///
/// A gzip stream can expand by a factor of a thousand or more, so a 40-byte input
/// that arrives from somewhere untrusted can otherwise exhaust memory. 100 MiB is
/// far above any legitimate use -- it is a zip archive or a firmware image, not a
/// string -- and low enough that the failure is an error rather than an OOM kill.
pub const MAX_DECOMPRESS: usize = 100 * 1024 * 1024;

/// gzip-decompress bytes. Errors on truncated/corrupt streams.
///
/// `limit` bounds the output. It is enforced while reading rather than after,
/// because the allocation is what does the damage: checking the length once the
/// buffer already exists would be checking a heap that is already too big.
///
/// Passing `usize::MAX` disables the bound, which is the caller's choice to make
/// explicitly rather than by accident.
pub fn gzip_decompress_limited(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut dec = flate2::read::GzDecoder::new(data);
    let mut out = Vec::new();
    // `take` is the bound: it stops the reader before it can grow past the limit,
    // and hitting it shows up as a short read rather than as success.
    //
    // `saturating_add` rather than `+ 1`, so an explicit `usize::MAX` means
    // "unbounded" instead of overflowing in debug and wrapping to 0 in release --
    // which would have turned "no limit" into "a one-byte limit".
    let cap = (limit as u64).saturating_add(1);
    let read = std::io::Read::take(&mut dec, cap).read_to_end(&mut out);
    match read {
        Ok(_) => {
            if out.len() > limit {
                return Err(format!(
                    "gzip_decompress: output exceeds the {limit}-byte limit \
                     (this looks like a decompression bomb, not a gzip stream)"
                ));
            }
            Ok(out)
        }
        Err(e) => Err(format!("gzip_decompress: {}", e)),
    }
}

/// gzip-decompress bytes, bounded by [`MAX_DECOMPRESS`].
pub fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    gzip_decompress_limited(data, MAX_DECOMPRESS)
}

/// One zip entry: (name, size, compressed_size).
pub fn zip_list(path: &str) -> Result<Vec<(String, u64, u64)>, String> {
    let f = std::fs::File::open(path)
        .map_err(|e| format!("zip_list: cannot open '{}': {}", path, e))?;
    let mut z =
        zip::ZipArchive::new(f).map_err(|e| format!("zip_list: bad zip '{}': {}", path, e))?;
    let mut out = Vec::new();
    for i in 0..z.len() {
        let f = z.by_index(i).map_err(|e| format!("zip_list: {}", e))?;
        out.push((f.name().to_string(), f.size(), f.compressed_size()));
    }
    Ok(out)
}

/// Read one entry's bytes out of a zip archive.
pub fn zip_read(path: &str, name: &str) -> Result<Vec<u8>, String> {
    let f = std::fs::File::open(path)
        .map_err(|e| format!("zip_read: cannot open '{}': {}", path, e))?;
    let mut z =
        zip::ZipArchive::new(f).map_err(|e| format!("zip_read: bad zip '{}': {}", path, e))?;
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
    let f = std::fs::File::create(path)
        .map_err(|e| format!("zip_write: cannot create '{}': {}", path, e))?;
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

    /// The bomb case: a small input that decompresses to far more than the limit.
    ///
    /// Compressing 4 MiB of zeroes gives a few kilobytes, which is the whole shape
    /// of the attack -- a tiny payload, a huge allocation.
    #[test]
    fn gzip_decompress_refuses_to_exceed_its_limit() {
        let bomb = gzip_compress(&vec![0u8; 4 * 1024 * 1024]);
        assert!(
            bomb.len() < 32 * 1024,
            "the compressed form should be tiny, was {} bytes",
            bomb.len()
        );
        // Under the limit: fine.
        assert_eq!(
            gzip_decompress_limited(&bomb, 8 * 1024 * 1024)
                .unwrap()
                .len(),
            4 * 1024 * 1024
        );
        // Over the limit: refused, with a message that says why.
        let err = gzip_decompress_limited(&bomb, 64 * 1024).unwrap_err();
        assert!(err.contains("limit"), "{}", err);
        assert!(
            err.contains("bomb"),
            "the error should name the likely cause: {}",
            err
        );
    }

    #[test]
    fn gzip_decompress_under_the_limit_is_unaffected() {
        let data = gzip_compress(b"hello hello hello");
        assert_eq!(
            gzip_decompress_limited(&data, 1024).unwrap(),
            b"hello hello hello"
        );
    }

    #[test]
    fn gzip_decompress_of_corrupt_input_still_errors() {
        assert!(gzip_decompress(&[0xff, 0x00, 0x01]).is_err());
    }

    #[test]
    fn gzip_decompress_of_empty_input_errors_rather_than_panicking() {
        assert!(gzip_decompress(&[]).is_err());
    }

    #[test]
    fn an_explicitly_unbounded_limit_is_allowed() {
        let data = gzip_compress(b"x");
        assert_eq!(gzip_decompress_limited(&data, usize::MAX).unwrap(), b"x");
    }

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
