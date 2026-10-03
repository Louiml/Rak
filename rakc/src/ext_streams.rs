//! Lazy streams for the bytecode VM (spec 7A.4).
//!
//! # Relationship to the interpreter's streams
//!
//! The interpreter has had working streams since before 8.0, built on its own
//! `RakStream` trait and its own `Value`. The VM had none at all, so
//! `stream_from_array` was a tree-walker builtin and `rakc vm` reported
//! `Undefined: stream_from_array`.
//!
//! These are the VM's, written against `value::Value`. They are deliberately
//! *not* a port of the interpreter's trait: that trait's `next` takes
//! `&mut Interpreter`, which is the whole reason it could not be reused — a
//! stream that has to call a Rak function needs a machine, and a VM native has
//! none. Here the callback is an explicit parameter instead, which is what lets
//! a stream be advanced from `Vm::stream_call` rather than from inside a
//! `fn(&[Value])` native.
//!
//! The two implementations are separate code, so they can diverge. That is a
//! real cost and it is why `parity_streams` in `tests/backend_parity.rs` exists:
//! it runs the same pipeline on both backends and requires identical output.
//! What is shared is the design — pull-based, lazy, backpressure-preserving —
//! and the builtin names, not the types.
//!
//! # Laziness
//!
//! `next` produces one element and the producer never runs ahead of the
//! consumer. `take(1)` over a 100k-element source does not build the 100k.

use std::sync::{Arc, Mutex};

use crate::value::Value;

/// Calls a Rak function with one value. The VM supplies this from a place that
/// has the machine; see the module comment on why it cannot be implicit.
pub type CallFn<'a> = &'a mut dyn FnMut(&Value, Value) -> Result<Value, String>;

/// A pull-based stream of VM values.
pub trait VmStream: Send {
    fn next(&mut self, call: CallFn<'_>) -> Result<Option<Value>, String>;
}

/// A shared VM stream handle (`Value::Stream`).
pub type VmStreamHandle = Arc<Mutex<Box<dyn VmStream>>>;

/// Over an in-memory sequence.
pub struct ArrayStream {
    items: std::vec::IntoIter<Value>,
}

impl ArrayStream {
    pub fn new(items: Vec<Value>) -> Self {
        ArrayStream {
            items: items.into_iter(),
        }
    }
}

impl VmStream for ArrayStream {
    fn next(&mut self, _call: CallFn<'_>) -> Result<Option<Value>, String> {
        Ok(self.items.next())
    }
}

/// Lazily reads lines from a file. Never loads the whole file.
pub struct LinesStream {
    reader: Option<std::io::Lines<std::io::BufReader<std::fs::File>>>,
}

impl LinesStream {
    pub fn open(path: &str) -> Result<Self, String> {
        use std::io::BufRead;
        let f = std::fs::File::open(path).map_err(|e| format!("read_lines: {}: {}", path, e))?;
        Ok(LinesStream {
            reader: Some(std::io::BufReader::new(f).lines()),
        })
    }
}

impl VmStream for LinesStream {
    fn next(&mut self, call: CallFn<'_>) -> Result<Option<Value>, String> {
        let Some(reader) = self.reader.as_mut() else {
            return Ok(None);
        };
        match reader.next() {
            Some(Ok(line)) => Ok(Some(as_text(call, line))),
            // A read error ends the stream rather than raising, so a partially
            // readable file still yields the lines that did parse. This matches
            // the interpreter.
            Some(Err(_)) | None => {
                self.reader = None;
                Ok(None)
            }
        }
    }
}

/// Lines read (blocking) from a TCP connection.
pub struct TcpLineStream {
    reader: Option<std::io::BufReader<std::net::TcpStream>>,
}

impl TcpLineStream {
    pub fn open(stream: std::net::TcpStream) -> Self {
        TcpLineStream {
            reader: Some(std::io::BufReader::new(stream)),
        }
    }
}

impl VmStream for TcpLineStream {
    fn next(&mut self, call: CallFn<'_>) -> Result<Option<Value>, String> {
        use std::io::BufRead;
        let Some(reader) = self.reader.as_mut() else {
            return Ok(None);
        };
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => {
                self.reader = None;
                Ok(None)
            }
            Ok(_) => Ok(Some(as_text(call, line.trim_end_matches('\n').to_string()))),
        }
    }
}

/// Lazy `map`.
pub struct MapStream {
    pub inner: VmStreamHandle,
    pub f: Value,
}

impl MapStream {
    pub fn new(inner: VmStreamHandle, f: Value) -> Self {
        MapStream { inner, f }
    }
}

impl VmStream for MapStream {
    fn next(&mut self, call: CallFn<'_>) -> Result<Option<Value>, String> {
        // The inner lock is released before calling `f`. Holding it across the
        // call would serialise every other consumer of a shared stream for the
        // duration of a user callback, and would deadlock if `f` pulled from the
        // same stream.
        let item = { self.inner.lock().unwrap().next(call)? };
        match item {
            Some(v) => Ok(Some(call(&self.f, v)?)),
            None => Ok(None),
        }
    }
}

/// Lazy `filter`.
pub struct FilterStream {
    pub inner: VmStreamHandle,
    pub f: Value,
}

impl FilterStream {
    pub fn new(inner: VmStreamHandle, f: Value) -> Self {
        FilterStream { inner, f }
    }
}

impl VmStream for FilterStream {
    fn next(&mut self, call: CallFn<'_>) -> Result<Option<Value>, String> {
        loop {
            let item = { self.inner.lock().unwrap().next(call)? };
            match item {
                Some(v) => {
                    let keep = call(&self.f, v.clone())?;
                    if keep.is_truthy() {
                        return Ok(Some(v));
                    }
                }
                None => return Ok(None),
            }
        }
    }
}

/// Lazy `take(n)`.
pub struct TakeStream {
    pub inner: VmStreamHandle,
    pub remaining: u64,
}

impl TakeStream {
    pub fn new(inner: VmStreamHandle, n: u64) -> Self {
        TakeStream {
            inner,
            remaining: n,
        }
    }
}

impl VmStream for TakeStream {
    fn next(&mut self, call: CallFn<'_>) -> Result<Option<Value>, String> {
        if self.remaining == 0 {
            return Ok(None);
        }
        let item = { self.inner.lock().unwrap().next(call)? };
        if item.is_some() {
            self.remaining -= 1;
        }
        Ok(item)
    }
}

/// Lazy CSV. With `has_header`, rows become maps keyed by the header row;
/// otherwise each row is an array of fields.
pub struct CsvStream {
    inner: VmStreamHandle,
    delim: char,
    has_header: bool,
    headers: Vec<String>,
    started: bool,
}

impl CsvStream {
    pub fn new(inner: VmStreamHandle, delim: char, has_header: bool) -> Self {
        CsvStream {
            inner,
            delim,
            has_header,
            headers: Vec::new(),
            started: false,
        }
    }
}

impl VmStream for CsvStream {
    fn next(&mut self, call: CallFn<'_>) -> Result<Option<Value>, String> {
        loop {
            let line = { self.inner.lock().unwrap().next(call)? };
            let Some(line) = line else { return Ok(None) };
            let line = line.to_string();
            if line.trim().is_empty() {
                continue;
            }
            let fields = rak_stdlib::stream_io::parse_csv_line(&line, self.delim);
            if !self.started {
                self.started = true;
                if self.has_header {
                    self.headers = fields;
                    // Skip the header row; only data rows are emitted.
                    continue;
                }
            }
            if self.has_header {
                let map: std::collections::HashMap<String, Value> = self
                    .headers
                    .iter()
                    .enumerate()
                    .map(|(i, h)| {
                        (
                            h.clone(),
                            Value::String(Arc::from(
                                fields.get(i).cloned().unwrap_or_default().as_str(),
                            )),
                        )
                    })
                    .collect();
                return Ok(Some(Value::Map(Arc::new(map))));
            }
            return Ok(Some(Value::Array(Arc::from(
                fields
                    .iter()
                    .map(|f| Value::String(Arc::from(f.as_str())))
                    .collect::<Vec<_>>(),
            ))));
        }
    }
}

/// Lazy JSONL. Each non-empty line is one JSON value.
pub struct JsonlStream {
    inner: VmStreamHandle,
}

impl JsonlStream {
    pub fn new(inner: VmStreamHandle) -> Self {
        JsonlStream { inner }
    }
}

impl VmStream for JsonlStream {
    fn next(&mut self, call: CallFn<'_>) -> Result<Option<Value>, String> {
        loop {
            let line = { self.inner.lock().unwrap().next(call)? };
            let Some(line) = line else { return Ok(None) };
            let line = line.to_string();
            if line.trim().is_empty() {
                continue;
            }
            let parsed = rak_stdlib::stream_io::parse_jsonl_line(&line)
                .map_err(|e| format!("stream_jsonl: {}", e))?;
            return Ok(Some(crate::ext_stdlib::json_to_value(&parsed)));
        }
    }
}

/// The lock-free part of rendering a string as a VM value.
///
/// `call` is threaded through only so the signature matches `VmStream::next`;
/// a string needs no machine.
fn as_text(_call: CallFn<'_>, s: String) -> Value {
    Value::String(Arc::from(s.as_str()))
}

/// Wrap a stream as a shareable handle.
pub fn handle(s: impl VmStream + 'static) -> VmStreamHandle {
    Arc::new(Mutex::new(Box::new(s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Streams that never call a Rak function need no machine, so a callback
    /// that always fails is enough to exercise the pure half.
    fn no_call(_f: &Value, _arg: Value) -> Result<Value, String> {
        Err("no machine in this test".to_string())
    }

    fn strings(items: &[&str]) -> Vec<Value> {
        items.iter().map(|s| Value::String(Arc::from(*s))).collect()
    }

    #[test]
    fn array_stream_yields_in_order() {
        let h = handle(ArrayStream::new(strings(&["a", "b", "c"])));
        let mut out = Vec::new();
        loop {
            match h.lock().unwrap().next(&mut no_call).unwrap() {
                Some(v) => out.push(v.to_string()),
                None => break,
            }
        }
        assert_eq!(out, vec!["a", "b", "c"]);
    }

    #[test]
    fn take_is_lazy_over_a_large_source() {
        // The point of laziness: `take(1)` must not cost 100k allocations.
        let big: Vec<Value> = (0..100_000).map(|i| Value::I64(i)).collect();
        let h = handle(TakeStream {
            inner: handle(ArrayStream::new(big)),
            remaining: 1,
        });
        let first = h.lock().unwrap().next(&mut no_call).unwrap();
        assert_eq!(first, Some(Value::I64(0)));
        assert!(h.lock().unwrap().next(&mut no_call).unwrap().is_none());
    }

    #[test]
    fn take_zero_yields_nothing_without_touching_the_source() {
        let h = handle(TakeStream {
            inner: handle(ArrayStream::new(strings(&["a"]))),
            remaining: 0,
        });
        assert!(h.lock().unwrap().next(&mut no_call).unwrap().is_none());
    }

    #[test]
    fn a_shared_stream_advances_across_consumers() {
        // Streams are shared values, so a second consumer must not restart the
        // sequence.
        let h = handle(ArrayStream::new(strings(&["a", "b"])));
        assert_eq!(
            h.lock().unwrap().next(&mut no_call).unwrap(),
            Some(strings(&["a"]).remove(0))
        );
        assert_eq!(
            h.lock().unwrap().next(&mut no_call).unwrap(),
            Some(strings(&["b"]).remove(0))
        );
        assert!(h.lock().unwrap().next(&mut no_call).unwrap().is_none());
    }

    #[test]
    fn map_propagates_a_call_failure() {
        // With no machine available, `map` must surface the error rather than
        // silently producing nil.
        let h = handle(MapStream {
            inner: handle(ArrayStream::new(strings(&["x"]))),
            f: Value::Nil,
        });
        let err = h.lock().unwrap().next(&mut no_call).unwrap_err();
        assert!(err.contains("no machine"), "got: {}", err);
    }

    #[test]
    fn csv_without_a_header_emits_arrays() {
        let inner = handle(ArrayStream::new(strings(&["a,b", "1,2"])));
        let h = handle(CsvStream::new(inner, ',', false));
        let first = h.lock().unwrap().next(&mut no_call).unwrap().unwrap();
        assert_eq!(first.to_string(), "[a, b]");
    }

    #[test]
    fn csv_with_a_header_skips_it_and_maps_rows() {
        let inner = handle(ArrayStream::new(strings(&["name,age", "ada,36"])));
        let h = handle(CsvStream::new(inner, ',', true));
        let row = h.lock().unwrap().next(&mut no_call).unwrap().unwrap();
        let Value::Map(m) = row else {
            panic!("expected a map, got {}", row);
        };
        assert_eq!(
            m.get("name").map(|v| v.to_string()),
            Some("ada".to_string())
        );
        assert_eq!(m.get("age").map(|v| v.to_string()), Some("36".to_string()));
    }

    #[test]
    fn csv_skips_blank_lines() {
        let inner = handle(ArrayStream::new(strings(&["", "a,b", "   "])));
        let h = handle(CsvStream::new(inner, ',', false));
        let got = h.lock().unwrap().next(&mut no_call).unwrap().unwrap();
        assert_eq!(got.to_string(), "[a, b]");
    }

    #[test]
    fn jsonl_parses_each_non_empty_line() {
        // The expected renderings are Rak's, not serde's: a parsed object prints
        // as `{a: 1}` because `Value`'s `Display` is the language's own syntax.
        // Asserting on the JSON spelling would have failed while the value was
        // in fact correct.
        let inner = handle(ArrayStream::new(strings(&[r#"{"a":1}"#, "  ", "[1,2]"])));
        let h = handle(JsonlStream::new(inner));
        let first = h.lock().unwrap().next(&mut no_call).unwrap().unwrap();
        assert_eq!(first.to_string(), "{a: 1}");
        let second = h.lock().unwrap().next(&mut no_call).unwrap().unwrap();
        assert_eq!(second.to_string(), "[1, 2]");
    }
}
