//! Streaming / data-processing helpers for large datasets: gzip/zlib, zip
//! archives, CSV and JSONL parsers. The stream/lazy parts live in the
//! interpreter's `Value::Stream`; this module provides the per-item primitives
//! (compression + row parsing) that streams wrap.

use std::io::{Read, Write};

/// Gzip-compress `data`, returning the gzip bytes.
pub fn gzip_compress(data: &[u8], level: u32) -> anyhow::Result<Vec<u8>> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    let mut enc = GzEncoder::new(Vec::new(), Compression::new(level.min(9)));
    enc.write_all(data)?;
    Ok(enc.finish()?)
}

/// Default ceiling on decompression output: 100 MiB.
///
/// A deflate stream can expand by a factor of a thousand, so a few dozen bytes
/// arriving from somewhere untrusted can otherwise exhaust memory. This is the
/// limit that matters: `gunzip` and `inflate` reach this function, so a bound here
/// is one a Rak program actually gets.
pub const MAX_DECOMPRESS: usize = 100 * 1024 * 1024;

/// Decompress gzip bytes, bounded by `limit`.
///
/// The bound is enforced *while* reading, because the allocation is the damage:
/// checking the length once the buffer exists would be measuring a heap that is
/// already too big. `Read::take` stops the reader first, and hitting the limit
/// shows up as a short read rather than as success.
pub fn gzip_decompress_limited(data: &[u8], limit: usize) -> anyhow::Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    let mut dec = GzDecoder::new(data);
    let mut out = Vec::new();
    // `saturating_add`, so an explicit `usize::MAX` means "unbounded" rather than
    // overflowing to a one-byte limit.
    let cap = (limit as u64).saturating_add(1);
    std::io::Read::take(&mut dec, cap).read_to_end(&mut out)?;
    if out.len() > limit {
        anyhow::bail!(
            "output exceeds the {limit}-byte limit; this looks like a decompression \
             bomb rather than a {}-byte input",
            data.len()
        );
    }
    Ok(out)
}

/// Decompress gzip bytes, bounded by [`MAX_DECOMPRESS`].
pub fn gzip_decompress(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    gzip_decompress_limited(data, MAX_DECOMPRESS)
}

/// Zip-compress (deflate) `data`, returning raw DEFLATE bytes (no zip container).
pub fn deflate_compress(data: &[u8], level: u32) -> anyhow::Result<Vec<u8>> {
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::new(level.min(9)));
    enc.write_all(data)?;
    Ok(enc.finish()?)
}

/// Decompress raw DEFLATE bytes, bounded by `limit`.
///
/// `inflate` reaches this, so the same bomb applies: raw DEFLATE is what a zip
/// entry holds, and a crafted entry is the usual delivery.
pub fn deflate_decompress_limited(data: &[u8], limit: usize) -> anyhow::Result<Vec<u8>> {
    use flate2::read::DeflateDecoder;
    let mut dec = DeflateDecoder::new(data);
    let mut out = Vec::new();
    let cap = (limit as u64).saturating_add(1);
    std::io::Read::take(&mut dec, cap).read_to_end(&mut out)?;
    if out.len() > limit {
        anyhow::bail!(
            "output exceeds the {limit}-byte limit; this looks like a decompression \
             bomb rather than a {}-byte input",
            data.len()
        );
    }
    Ok(out)
}

/// Decompress raw DEFLATE bytes, bounded by [`MAX_DECOMPRESS`].
pub fn deflate_decompress(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    deflate_decompress_limited(data, MAX_DECOMPRESS)
}

/// Build a ZIP archive from `files: Vec<(name, bytes)>`.
pub fn zip_archive(files: Vec<(String, Vec<u8>)>) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        for (name, bytes) in files {
            let opts = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            w.start_file(name, opts)?;
            w.write_all(&bytes)?;
        }
        w.finish()?;
    }
    Ok(buf)
}

/// List the entries (names) in a ZIP archive.
pub fn zip_list(data: &[u8]) -> anyhow::Result<Vec<String>> {
    let reader = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader)?;
    let names = (0..archive.len())
        .map(|i| {
            archive
                .by_index(i)
                .map(|f| f.name().to_string())
                .unwrap_or_default()
        })
        .collect();
    Ok(names)
}

/// Extract a named file from a ZIP archive.
pub fn zip_extract(data: &[u8], name: &str) -> anyhow::Result<Vec<u8>> {
    let reader = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader)?;
    let mut file = archive.by_name(name)?;
    let mut out = Vec::new();
    file.read_to_end(&mut out)?;
    Ok(out)
}

/// Parse one CSV line (RFC-4180-ish: handles quoted fields with commas) into a
/// `Vec<String>`.
pub fn parse_csv_line(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == delimiter {
            fields.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    fields.push(cur);
    fields
}

/// Parse one JSONL line into a `serde_json::Value`.
pub fn parse_jsonl_line(line: &str) -> anyhow::Result<serde_json::Value> {
    serde_json::from_str(line.trim()).map_err(|e| anyhow::anyhow!("{}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gzip_round_trip() {
        let data = b"the quick brown fox jumps over the lazy dog".repeat(100);
        let gz = gzip_compress(&data, 6).unwrap();
        let back = gzip_decompress(&gz).unwrap();
        assert_eq!(back, data);
        assert!(gz.len() < data.len());
    }

    #[test]
    fn parse_csv_quoted() {
        assert_eq!(parse_csv_line("a,\"b,c\",d", ','), vec!["a", "b,c", "d"]);
        assert_eq!(parse_csv_line("1;2;3", ';'), vec!["1", "2", "3"]);
    }

    #[test]
    fn zip_round_trip() {
        let zip = zip_archive(vec![
            ("a.txt".to_string(), b"hello".to_vec()),
            ("b.txt".to_string(), b"world".to_vec()),
        ])
        .unwrap();
        assert_eq!(zip_list(&zip).unwrap(), vec!["a.txt", "b.txt"]);
        assert_eq!(zip_extract(&zip, "a.txt").unwrap(), b"hello");
    }

    /// The reachable path. `archive.rs` has a near-identical `gzip_decompress`
    /// that no builtin calls, so a limit there looks like a fix and protects
    /// nothing; `gunzip` and `inflate` come through this module.
    #[test]
    fn gunzip_is_bounded() {
        let bomb = gzip_compress(&vec![0u8; 8 * 1024 * 1024], 6).expect("compress");
        assert!(
            bomb.len() < 64 * 1024,
            "bomb should be small, was {}",
            bomb.len()
        );
        assert_eq!(
            gzip_decompress_limited(&bomb, 32 * 1024 * 1024)
                .expect("under the limit")
                .len(),
            8 * 1024 * 1024
        );
        let err = gzip_decompress_limited(&bomb, 128 * 1024).expect_err("over the limit");
        let msg = err.to_string();
        assert!(msg.contains("limit"), "{}", msg);
        assert!(msg.contains("bomb"), "should name the cause: {}", msg);
        // The size is in the message; asserting the digits rather than the word
        // avoids depending on "8174-byte" vs "8174 bytes".
        assert!(
            msg.contains(&bomb.len().to_string()),
            "should say how big the input was: {}",
            msg
        );
    }

    #[test]
    fn inflate_is_bounded() {
        let bomb = deflate_compress(&vec![0u8; 8 * 1024 * 1024], 6).expect("compress");
        assert!(bomb.len() < 64 * 1024);
        assert!(deflate_decompress_limited(&bomb, 128 * 1024).is_err());
        assert_eq!(
            deflate_decompress_limited(&bomb, 32 * 1024 * 1024)
                .expect("under the limit")
                .len(),
            8 * 1024 * 1024
        );
    }

    #[test]
    fn a_bomb_is_refused_by_the_default_path_too() {
        // Not just the `_limited` variant: `gunzip(x)` must be safe.
        let bomb = gzip_compress(&vec![0u8; 4 * 1024 * 1024], 6).expect("compress");
        // 4 MiB is under the 100 MiB default, so this one is fine -- the point is
        // that the default path is the bounded one.
        assert!(gzip_decompress(&bomb).is_ok());
        // And an oversized one is refused rather than attempted.
        let huge = gzip_compress(&vec![7u8; 200 * 1024 * 1024], 9);
        // Compressing 200 MiB may itself fail on a constrained runner; only assert
        // when it succeeded, so this test cannot fail for an unrelated reason.
        if let Ok(huge) = huge {
            assert!(
                gzip_decompress(&huge).is_err(),
                "a 200 MiB bomb must be refused"
            );
        }
    }

    #[test]
    fn a_limit_of_max_means_unbounded() {
        let data = gzip_compress(b"x", 6).expect("compress");
        assert_eq!(
            gzip_decompress_limited(&data, usize::MAX).expect("unbounded"),
            b"x"
        );
    }

    #[test]
    fn corrupt_input_errors_rather_than_panicking() {
        assert!(gzip_decompress(&[1, 2, 3, 4]).is_err());
        assert!(gzip_decompress(&[]).is_err());
        assert!(deflate_decompress(&[1, 2, 3, 4]).is_err());
    }
}
