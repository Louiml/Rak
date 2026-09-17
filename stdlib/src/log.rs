//! Structured, machine-readable (JSON-lines) logging.
//!
//! Each log call emits one JSON object per line to stdout (default) or an
//! append-only file (via `log_init`). Output is greppable with jq and feeds
//! forensic / OSINT pipelines. Basic log levels gate verbosity.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn from_str(s: &str) -> Level {
        match s.to_lowercase().as_str() {
            "debug" => Level::Debug,
            "warn" | "warning" => Level::Warn,
            "error" => Level::Error,
            _ => Level::Info,
        }
    }
    fn as_str(&self) -> &'static str {
        match self {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

// A JSON field value. Kept intentionally small — the builtin layer maps Rak
// values into this before serialising.
#[derive(Debug, Clone)]
pub enum Json {
    Str(String),
    Num(f64),
    Bool(bool),
    Nil,
    Arr(Vec<Json>),
    Obj(HashMap<String, Json>),
}

fn escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

impl Json {
    /// Compact JSON serialisation (no pretty-printing, stable key order not
    /// guaranteed — HashMap iteration order).
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }
    fn write(&self, out: &mut String) {
        match self {
            Json::Str(s) => escape(s, out),
            Json::Num(n) => {
                if n.fract() == 0.0 && n.is_finite() {
                    out.push_str(&format!("{}", *n as i64));
                } else {
                    out.push_str(&format!("{}", n));
                }
            }
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Nil => out.push_str("null"),
            Json::Arr(items) => {
                out.push('[');
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    it.write(out);
                }
                out.push(']');
            }
            Json::Obj(map) => {
                out.push('{');
                let mut first = true;
                for (k, v) in map {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    escape(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

thread_local! {
    static LEVEL: RefCell<Level> = RefCell::new(Level::Info);
}

// File destination, shared across threads via a global mutex. `None` means sink
// to stdout. Initialised lazily by `log_init`.
static FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

fn now_rfc3339() -> String {
    // RFC3339-ish timestamp. We avoid pulling a date crate: emit epoch
    // milliseconds alongside a simple UTC string via `date` is overkill, so we
    // just expose epoch_ms (unambiguous, jq-friendly).
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis().to_string()).unwrap_or_else(|_| "0".to_string())
}

/// Set the minimum log level ("debug" | "info" | "warn" | "error").
pub fn set_level(s: &str) {
    LEVEL.with(|l| *l.borrow_mut() = Level::from_str(s));
}

/// Open an append-only log file. Passing "" resets back to stdout.
pub fn init_file(path: &str) -> Result<(), String> {
    let mut guard = FILE.lock().map_err(|e| format!("log: lock: {}", e))?;
    if path.is_empty() {
        *guard = None;
    } else {
        let f = OpenOptions::new().create(true).append(true).open(path).map_err(|e| format!("log_init: {}: {}", path, e))?;
        *guard = Some(f);
    }
    Ok(())
}

/// Emit a structured record. `key` is the log subject; `fields` is an object
/// of key/value pairs. Returns Ok(()) always (best-effort IO).
pub fn log(level: Level, key: &str, fields: &HashMap<String, Json>) -> Result<(), String> {
    let enabled = LEVEL.with(|l| level >= *l.borrow());
    if !enabled {
        return Ok(());
    }
    let mut obj = HashMap::new();
    obj.insert("ts".to_string(), Json::Str(now_rfc3339()));
    obj.insert("level".to_string(), Json::Str(level.as_str().to_string()));
    obj.insert("evt".to_string(), Json::Str(key.to_string()));
    if !fields.is_empty() {
        obj.insert("fields".to_string(), Json::Obj(fields.clone()));
    }
    let line = Json::Obj(obj).to_json() + "\n";
    let mut guard = FILE.lock().map_err(|e| format!("log: lock: {}", e))?;
    if let Some(f) = guard.as_mut() {
        f.write_all(line.as_bytes()).map_err(|e| format!("log: write: {}", e))?;
        let _ = f.flush();
    } else {
        let mut out = std::io::stdout();
        out.write_all(line.as_bytes()).map_err(|e| format!("log: write: {}", e))?;
        let _ = out.flush();
    }
    Ok(())
}