//! Streaming / data-processing helpers for large datasets: gzip/zlib, zip
//! archives, CSV and JSONL parsers. The stream/lazy parts live in the
//! interpreter's `Value::Stream`; this module provides the per-item primitives
//! (compression + row parsing) that streams wrap.

use std::io::{Read, Write};

/// Gzip-compress `data`, returning the gzip bytes.
pub fn gzip_compress(data: &[u8], level: u32) -> anyhow::Result<Vec<u8>> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    let mut enc = GzEncoder::new(Vec::new(), Compression::new(level.min(9)));
    enc.write_all(data)?;
    Ok(enc.finish()?)
}

/// Decompress gzip bytes.
pub fn gzip_decompress(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    let mut dec = GzDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)?;
    Ok(out)
}

/// Zip-compress (deflate) `data`, returning raw DEFLATE bytes (no zip container).
pub fn deflate_compress(data: &[u8], level: u32) -> anyhow::Result<Vec<u8>> {
    use flate2::Compression;
    use flate2::write::DeflateEncoder;
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::new(level.min(9)));
    enc.write_all(data)?;
    Ok(enc.finish()?)
}

/// Decompress raw DEFLATE bytes.
pub fn deflate_decompress(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    use flate2::read::DeflateDecoder;
    let mut dec = DeflateDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)?;
    Ok(out)
}

/// Build a ZIP archive from `files: Vec<(name, bytes)>`.
pub fn zip_archive(files: Vec<(String, Vec<u8>)>) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        for (name, bytes) in files {
            let opts = zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
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
        .map(|i| archive.by_index(i).map(|f| f.name().to_string()).unwrap_or_default())
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
        let zip = zip_archive(vec![("a.txt".to_string(), b"hello".to_vec()), ("b.txt".to_string(), b"world".to_vec())]).unwrap();
        assert_eq!(zip_list(&zip).unwrap(), vec!["a.txt", "b.txt"]);
        assert_eq!(zip_extract(&zip, "a.txt").unwrap(), b"hello");
    }
}