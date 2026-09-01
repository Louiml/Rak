use std::fs;
use std::path::Path;

/// Read a file's contents as a string
pub fn read(path: &str) -> anyhow::Result<String> {
    fs::read_to_string(path).map_err(|e| anyhow::anyhow!("{}", e))
}

/// Read a file as raw bytes
pub fn read_bytes(path: &str) -> anyhow::Result<Vec<u8>> {
    fs::read(path).map_err(|e| anyhow::anyhow!("{}", e))
}

/// Write content to a file (overwrites)
pub fn write(path: &str, content: &str) -> anyhow::Result<()> {
    fs::write(path, content).map_err(|e| anyhow::anyhow!("{}", e))
}

/// Append content to a file
pub fn append(path: &str, content: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new().append(true).create(true).open(path)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

/// Check if a file or directory exists
pub fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// Get file size in bytes
pub fn size(path: &str) -> Option<u64> {
    fs::metadata(path).ok().map(|m| m.len())
}

/// List files in a directory (returns names)
pub fn list(path: &str) -> Vec<String> {
    match fs::read_dir(path) {
        Ok(entries) => entries.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect(),
        Err(_) => vec![],
    }
}

/// Delete a file
pub fn delete(path: &str) -> bool {
    fs::remove_file(path).is_ok()
}

/// Create a directory (and parents)
pub fn create_dir(path: &str) -> bool {
    fs::create_dir_all(path).is_ok()
}

/// Copy a file
pub fn copy(src: &str, dst: &str) -> bool {
    fs::copy(src, dst).is_ok()
}

/// Rename or move a file
pub fn rename(src: &str, dst: &str) -> bool {
    fs::rename(src, dst).is_ok()
}

/// Get file extension
pub fn ext(path: &str) -> Option<String> {
    Path::new(path).extension().map(|e| e.to_string_lossy().to_string())
}

/// Get filename without extension
pub fn basename(path: &str) -> String {
    Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

/// Get directory path of a file
pub fn dirname(path: &str) -> String {
    Path::new(path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
}

/// Create a temporary file and return its path
pub fn temp_file(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = format!("{}/{}{}", std::env::temp_dir().to_string_lossy(), prefix, timestamp);
    let _ = fs::write(&path, "");
    path
}