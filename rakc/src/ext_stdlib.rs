//! The stdlib builtins that the tree-walking interpreter has always had and the
//! bytecode VM did not.
//!
//! Rak has two backends. Through v8.0.0 the gap between them was invisible
//! because nothing compared them: the unit tests exercised each backend in
//! isolation, so a builtin that existed on only one of them was not a test
//! failure, it was simply a language that differed depending on how you invoked
//! it. `rakc run` could do `abs` and `sort`; `rakc vm` could not.
//!
//! Everything here is a thin wrapper over `rak_stdlib`, matching the
//! interpreter's `eval_builtin` arm for the same name. The rule for adding to
//! this file is that the function must be pure with respect to the VM: a native
//! receives `&[Value]` and no access to the machine, so it cannot call a Rak
//! function, suspend, or resume. Builtins that need any of those do not belong
//! here — they need the interception in `Vm::call_value`, where the VM is
//! reachable, or a real coroutine model. See `docs/V8-BACKEND-PARITY.md` for
//! which builtins fall in which bucket.

use crate::value::Value;
use std::collections::HashMap;
use std::sync::Arc;

type Args = [Value];
type R = Result<Value, String>;

// ---------------------------------------------------------------------------
// Conversions
//
// These mirror `Interpreter::val_to_string` / `val_to_bytes` exactly. Parity
// here is not a style preference: a builtin that renders `nil` as `"nil"` on
// one backend and `""` on the other is a portability bug in every program that
// prints a maybe-absent value.
// ---------------------------------------------------------------------------

fn to_str(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.to_string(),
        Some(Value::Bytes(b)) => String::from_utf8_lossy(b).to_string(),
        Some(Value::Mmap(h)) => String::from_utf8_lossy(h.as_slice()).to_string(),
        Some(Value::MmapSlice(h, off, n)) => {
            String::from_utf8_lossy(&h.as_slice()[*off..off + n]).to_string()
        }
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

/// Coerce a value to bytes, mirroring `Interpreter::val_to_bytes`.
///
/// An array or tuple of byte values becomes *those bytes*, not the text of the
/// array. Mirroring matters here rather than being tidiness: on one backend
/// `bytes([0x89, 0x50])` produced `[137, 80]` as text and on the other a two-byte
/// buffer, so every caller had to know which backend it was on.
fn to_bytes(v: Option<&Value>) -> Vec<u8> {
    match v {
        Some(Value::Bytes(b)) => b.to_vec(),
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        Some(Value::Hex(h)) => h.to_le_bytes().to_vec(),
        Some(Value::Array(a)) => {
            let a: &[Value] = a.as_slice();
            let mut out = Vec::with_capacity(a.len());
            for item in a.iter() {
                match item {
                    Value::Char(c) => {
                        let mut buf = [0u8; 4];
                        out.extend_from_slice((*c).encode_utf8(&mut buf).as_bytes());
                    }
                    Value::Bytes(b) => out.extend_from_slice(b),
                    other => match other.as_i64() {
                        Some(i) if (0..=255).contains(&i) => out.push(i as u8),
                        // Anything else is the caller's mistake; the interpreter
                        // reports the offending element by name and the VM cannot
                        // from here, so it falls back to the text of the value and
                        // the length differs visibly.
                        _ => out.extend_from_slice(other.to_string().as_bytes()),
                    },
                }
            }
            out
        }
        Some(Value::Mmap(h)) => h.as_slice().to_vec(),
        Some(Value::MmapSlice(h, off, n)) => h.as_slice()[*off..off + n].to_vec(),
        Some(v) => match v.as_i64() {
            Some(i) => i.to_le_bytes().to_vec(),
            None => v.to_string().into_bytes(),
        },
        None => vec![],
    }
}

/// A non-negative byte offset from an argument, accepting `Int`, `Hex` and
/// `Float` - `0`, `0x10` and `16.0` all have to mean the same offset.
fn to_offset(v: Option<&Value>) -> Result<usize, String> {
    match v {
        Some(Value::F64(f)) if *f >= 0.0 && f.fract() == 0.0 => Ok(*f as usize),
        Some(other) => match other.as_i64() {
            Some(i) if i >= 0 => Ok(i as usize),
            _ => Err(format!(
                "expected a non-negative integer offset, got {}",
                other
            )),
        },
        None => Err("expected an offset argument".to_string()),
    }
}

/// One byte from an argument: `0xFF`, `255` and `'A'` are the same byte.
pub fn to_byte(v: Option<&Value>) -> Result<u8, String> {
    match v {
        Some(Value::Char(c)) => {
            let mut buf = [0u8; 4];
            Ok(c.encode_utf8(&mut buf).as_bytes()[0])
        }
        Some(other) => match other.as_i64() {
            Some(i) if (0..=255).contains(&i) => Ok(i as u8),
            _ => Err(format!(
                "expected a byte value (0..255 or a char), got {}",
                other
            )),
        },
        None => Err("expected a byte argument".to_string()),
    }
}

fn s(v: &str) -> Value {
    Value::String(Arc::from(v))
}

fn s_owned(v: String) -> Value {
    Value::String(Arc::from(v.as_str()))
}

fn b(v: Vec<u8>) -> Value {
    Value::Bytes(Arc::from(v.as_slice()))
}

fn arr(v: Vec<Value>) -> Value {
    Value::Array(Arc::from(v))
}

fn strings(v: Vec<String>) -> Value {
    arr(v.into_iter().map(|x| s_owned(x)).collect())
}

fn str_map(v: HashMap<String, String>) -> Value {
    Value::Map(Arc::new(
        v.into_iter().map(|(k, x)| (k, s_owned(x))).collect(),
    ))
}

/// `Option<String>` as `string | nil`, the shape every `html_*` accessor
/// returns on the interpreter.
fn opt_str(v: Option<String>) -> Value {
    match v {
        Some(x) => s_owned(x),
        None => Value::Nil,
    }
}

fn arg<'a>(args: &'a Args, i: usize) -> Option<&'a Value> {
    args.get(i)
}

// ---------------------------------------------------------------------------
// Math
// ---------------------------------------------------------------------------

fn vm_abs(args: &Args) -> R {
    Ok(Value::I64(
        args.first().and_then(|v| v.as_i64()).unwrap_or(0).abs(),
    ))
}

fn vm_sqrt(args: &Args) -> R {
    Ok(Value::F64(
        args.first().and_then(|v| v.as_f64()).unwrap_or(0.0).sqrt(),
    ))
}

fn vm_pow(args: &Args) -> R {
    let base = args.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
    let exp = args.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0);
    Ok(Value::F64(base.powf(exp)))
}

fn vm_min(args: &Args) -> R {
    let nums: Vec<i64> = args.iter().filter_map(|v| v.as_i64()).collect();
    if nums.is_empty() {
        return Ok(Value::Nil);
    }
    Ok(Value::I64(*nums.iter().min().unwrap()))
}

fn vm_max(args: &Args) -> R {
    let nums: Vec<i64> = args.iter().filter_map(|v| v.as_i64()).collect();
    if nums.is_empty() {
        return Ok(Value::Nil);
    }
    Ok(Value::I64(*nums.iter().max().unwrap()))
}

fn vm_clamp(args: &Args) -> R {
    let v = args.first().and_then(|v| v.as_i64()).unwrap_or(0);
    let lo = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
    let hi = args.get(2).and_then(|v| v.as_i64()).unwrap_or(0);
    Ok(Value::I64(v.max(lo).min(hi)))
}

fn vm_ord(args: &Args) -> R {
    let s = to_str(args.first());
    Ok(Value::I64(s.chars().next().map(|c| c as i64).unwrap_or(0)))
}

fn vm_chr(args: &Args) -> R {
    let n = args.first().and_then(|v| v.as_i64()).unwrap_or(0) as u32;
    Ok(s_owned(
        char::from_u32(n).map(|c| c.to_string()).unwrap_or_default(),
    ))
}

fn vm_sum(args: &Args) -> R {
    if let Some(Value::Array(a)) = args.first() {
        // A single non-integer element promotes the whole sum to float, which
        // is what the interpreter does; anything non-numeric is skipped.
        let mut total = 0i64;
        for v in a.iter() {
            if let Some(n) = v.as_i64() {
                total += n;
            } else if v.as_f64().is_some() {
                return Ok(Value::F64(a.iter().filter_map(|v| v.as_f64()).sum()));
            }
        }
        Ok(Value::I64(total))
    } else {
        Ok(Value::I64(0))
    }
}

// ---------------------------------------------------------------------------
// Strings
// ---------------------------------------------------------------------------

fn vm_to_string(args: &Args) -> R {
    Ok(s_owned(to_str(args.first())))
}

fn vm_split(args: &Args) -> R {
    let subject = to_str(args.first());
    let delim = to_str(args.get(1));
    Ok(strings(
        subject.split(&delim).map(|p| p.to_string()).collect(),
    ))
}

fn vm_join(args: &Args) -> R {
    let delim = to_str(args.first());
    match args.get(1) {
        Some(Value::Array(a)) => Ok(s_owned(
            a.iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(&delim),
        )),
        _ => Err("join() requires an array".to_string()),
    }
}

fn vm_replace(args: &Args) -> R {
    Ok(s_owned(
        to_str(args.first()).replace(&to_str(args.get(1)), &to_str(args.get(2))),
    ))
}

fn vm_repeat(args: &Args) -> R {
    let n = args.get(1).and_then(|v| v.as_i64()).unwrap_or(1).max(0) as usize;
    Ok(s_owned(to_str(args.first()).repeat(n)))
}

fn vm_starts_with(args: &Args) -> R {
    Ok(Value::Bool(
        to_str(args.first()).starts_with(&to_str(args.get(1))),
    ))
}

fn vm_ends_with(args: &Args) -> R {
    Ok(Value::Bool(
        to_str(args.first()).ends_with(&to_str(args.get(1))),
    ))
}

fn vm_find(args: &Args) -> R {
    // Byte offset, -1 when absent — matching the interpreter, and matching
    // what a caller indexing with the result expects.
    Ok(Value::I64(
        to_str(args.first())
            .find(&to_str(args.get(1)))
            .map(|i| i as i64)
            .unwrap_or(-1),
    ))
}

fn vm_substr(args: &Args) -> R {
    let subject = to_str(args.first());
    let start = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
    let length = args.get(2).and_then(|v| v.as_i64()).unwrap_or(0);
    // Matches the interpreter, which rejects these rather than casting to `usize`.
    // The VM used `saturating_add` and so never aborted, but it returned the empty
    // string for a negative start -- a different wrong answer for the same input.
    // See the interpreter's `substr` for why an error is the right answer.
    if start < 0 {
        return Err(format!(
            "substr: start must not be negative (got {})",
            start
        ));
    }
    if length < 0 {
        return Err(format!(
            "substr: length must not be negative (got {})",
            length
        ));
    }
    let chars: Vec<char> = subject.chars().collect();
    let from = (start as usize).min(chars.len());
    let end = (start as usize)
        .saturating_add(length as usize)
        .min(chars.len())
        .max(from);
    Ok(s_owned(chars[from..end].iter().collect()))
}

fn vm_trim(args: &Args) -> R {
    Ok(s_owned(to_str(args.first()).trim().to_string()))
}

fn vm_trim_start(args: &Args) -> R {
    Ok(s_owned(to_str(args.first()).trim_start().to_string()))
}

fn vm_trim_end(args: &Args) -> R {
    Ok(s_owned(to_str(args.first()).trim_end().to_string()))
}

fn vm_rot13(args: &Args) -> R {
    Ok(s_owned(rak_stdlib::rot13(&to_str(args.first()))))
}

fn vm_url_encode(args: &Args) -> R {
    Ok(s_owned(rak_stdlib::url_encode(&to_str(args.first()))))
}

fn vm_url_decode(args: &Args) -> R {
    // `url_decode` returns `Option`, not `Result`: an invalid escape yields
    // `None` rather than a diagnostic.
    match rak_stdlib::url_decode(&to_str(args.first())) {
        Some(x) => Ok(s_owned(x)),
        None => Err("url_decode: malformed percent-encoding".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Arrays
// ---------------------------------------------------------------------------

fn vm_sort(args: &Args) -> R {
    match args.first() {
        Some(Value::Array(a)) => {
            // Sorted by rendered value, matching the interpreter. It is a
            // string sort, so it is total and stable across runs — unlike map
            // iteration, which is not.
            let mut items = a.to_vec();
            items.sort_by(|x, y| x.to_string().cmp(&y.to_string()));
            Ok(arr(items))
        }
        _ => Err("sort() requires an array".to_string()),
    }
}

fn vm_reverse(args: &Args) -> R {
    match args.first() {
        Some(Value::Array(a)) => {
            let mut items = a.to_vec();
            items.reverse();
            Ok(arr(items))
        }
        Some(Value::String(x)) => Ok(s_owned(x.chars().rev().collect())),
        _ => Err("reverse() requires array or string".to_string()),
    }
}

fn vm_push(args: &Args) -> R {
    match args.first() {
        Some(Value::Array(a)) => {
            let mut items = a.to_vec();
            if let Some(item) = args.get(1) {
                items.push(item.clone());
            }
            Ok(arr(items))
        }
        _ => Err("push() requires an array".to_string()),
    }
}

fn vm_from_hex(args: &Args) -> R {
    let raw = to_str(args.first());
    let raw = raw.trim_start_matches("0x").trim_start_matches("0X");
    let h = u64::from_str_radix(raw, 16).map_err(|_| "Invalid hex literal".to_string())?;
    // The width is in bits and only affects rendering. The interpreter prints
    // `0x{:X}` with no padding, so the VM has to size the field to the value's
    // natural width or the same program prints a different number on each
    // backend.
    let nibbles = format!("{:X}", h).len();
    Ok(Value::Hex(h))
}

// ---------------------------------------------------------------------------
// Codecs
// ---------------------------------------------------------------------------

fn vm_xor(args: &Args) -> R {
    Ok(b(rak_stdlib::xor_encrypt(
        &to_bytes(args.first()),
        &to_bytes(args.get(1)),
    )))
}

fn vm_hex_decode(args: &Args) -> R {
    match rak_stdlib::hex_decode(&to_str(args.first())) {
        Some(v) => Ok(b(v)),
        None => Err("hex_decode: input is not valid hexadecimal".to_string()),
    }
}

fn vm_base64_decode(args: &Args) -> R {
    match rak_stdlib::base64_decode(&to_str(args.first())) {
        Some(v) => Ok(b(v)),
        None => Err("base64_decode: input is not valid base64".to_string()),
    }
}

fn vm_gzip(args: &Args) -> R {
    let level = args.get(1).and_then(|v| v.as_u64()).unwrap_or(6) as u32;
    rak_stdlib::stream_io::gzip_compress(&to_bytes(args.first()), level)
        .map(b)
        .map_err(|e| format!("gzip: {}", e))
}

fn vm_gunzip(args: &Args) -> R {
    rak_stdlib::stream_io::gzip_decompress(&to_bytes(args.first()))
        .map(b)
        .map_err(|e| format!("gunzip: {}", e))
}

fn vm_deflate(args: &Args) -> R {
    let level = args.get(1).and_then(|v| v.as_u64()).unwrap_or(6) as u32;
    rak_stdlib::stream_io::deflate_compress(&to_bytes(args.first()), level)
        .map(b)
        .map_err(|e| format!("deflate: {}", e))
}

fn vm_inflate(args: &Args) -> R {
    rak_stdlib::stream_io::deflate_decompress(&to_bytes(args.first()))
        .map(b)
        .map_err(|e| format!("inflate: {}", e))
}

// ---------------------------------------------------------------------------
// Sets (spec 7A.11)
//
// The storage is shared with the interpreter (`crate::setrepr::SetRepr`) and
// keyed identically, so a set behaves the same on either backend: same
// membership rule, same insertion-order iteration.
// ---------------------------------------------------------------------------

type Set = Arc<std::sync::Mutex<crate::setrepr::SetRepr<Value>>>;

fn as_set(v: Option<&Value>, who: &str) -> Result<Set, String> {
    match v {
        Some(Value::Set(s)) => Ok(s.clone()),
        Some(other) => Err(format!(
            "{}: expected a set, got {}",
            who,
            other.type_name()
        )),
        None => Err(format!("{}: expected a set", who)),
    }
}

/// The elements of a set-or-array argument, so `set_of` and `set_has_all`
/// accept either.
fn set_items(v: Option<&Value>, who: &str) -> Result<Vec<Value>, String> {
    match v {
        Some(Value::Array(a)) => Ok(a.to_vec()),
        Some(Value::Set(s)) => Ok(s.lock().unwrap().to_vec()),
        Some(other) => Err(format!(
            "{}: expected an array or set, got {}",
            who,
            other.type_name()
        )),
        None => Err(format!("{}: expected an array or set", who)),
    }
}

fn vm_set_of(args: &Args) -> R {
    let items = match args.first() {
        Some(v) => set_items(Some(v), "set_of")?,
        None => Vec::new(),
    };
    Ok(Value::Set(Arc::new(std::sync::Mutex::new(
        crate::setrepr::SetRepr::from_iter_ordered(items),
    ))))
}

fn vm_set_add(args: &Args) -> R {
    // The set is behind an `Arc`, not an `Arc<Mutex<_>>`, so a mutating builtin
    // cannot be a plain `fn(&[Value])`: it needs to reach the VM to copy,
    // update and write back, exactly like the interpreter does. This one is
    // registered in `Vm::call_value`'s interception instead.
    Err("set_add: internal error, should be intercepted".to_string())
}

fn vm_set_has(args: &Args) -> R {
    let set = as_set(args.first(), "set_has")?;
    let item = args.get(1).cloned().unwrap_or(Value::Nil);
    let guard = set.lock().unwrap();
    Ok(Value::Bool(guard.contains(&item)))
}

fn vm_set_discard(args: &Args) -> R {
    let _ = as_set(args.first(), "set_discard")?;
    Err("set_discard: internal error, should be intercepted".to_string())
}

fn vm_set_len(args: &Args) -> R {
    let set = as_set(args.first(), "set_len")?;
    let n = set.lock().unwrap().len();
    Ok(Value::I64(n as i64))
}

fn vm_set_has_all(args: &Args) -> R {
    let set = as_set(args.first(), "set_has_all")?;
    let items = set_items(args.get(1), "set_has_all")?;
    let guard = set.lock().unwrap();
    Ok(Value::Bool(items.iter().all(|v| guard.contains(v))))
}

fn vm_set_union(args: &Args) -> R {
    let a = as_set(args.first(), "set_union")?;
    let b = as_set(args.get(1), "set_union")?;
    let out = a.lock().unwrap().union(&b.lock().unwrap());
    Ok(Value::Set(Arc::new(std::sync::Mutex::new(out))))
}

fn vm_set_intersect(args: &Args) -> R {
    let a = as_set(args.first(), "set_intersect")?;
    let b = as_set(args.get(1), "set_intersect")?;
    let out = a.lock().unwrap().intersect(&b.lock().unwrap());
    Ok(Value::Set(Arc::new(std::sync::Mutex::new(out))))
}

fn vm_set_diff(args: &Args) -> R {
    let a = as_set(args.first(), "set_diff")?;
    let b = as_set(args.get(1), "set_diff")?;
    let out = a.lock().unwrap().diff(&b.lock().unwrap());
    Ok(Value::Set(Arc::new(std::sync::Mutex::new(out))))
}

fn vm_set_to_array(args: &Args) -> R {
    let set = as_set(args.first(), "set_to_array")?;
    let items = set.lock().unwrap().to_vec();
    Ok(arr(items))
}

// ---------------------------------------------------------------------------
// Encrypted UDP (spec 7A.5)
// ---------------------------------------------------------------------------

type Udp = Arc<std::sync::Mutex<rak_stdlib::tunnel::UdpTransport>>;

fn as_udp(v: Option<&Value>, who: &str) -> Result<Udp, String> {
    match v {
        Some(Value::UdpTransport(t)) => Ok(t.clone()),
        Some(other) => Err(format!(
            "{}: expected a UDP transport, got {}",
            who,
            other.type_name()
        )),
        None => Err(format!("{}: expected a UDP transport", who)),
    }
}

fn vm_udp_bind(args: &Args) -> R {
    let addr = match args.first() {
        Some(v) => to_str(Some(v)),
        None => "127.0.0.1:0".to_string(),
    };
    let (transport, local) = rak_stdlib::tunnel::udp_bind(&addr).map_err(|e| e.to_string())?;
    Ok(Value::Tuple(Arc::from(vec![
        Value::UdpTransport(Arc::new(transport)),
        s_owned(local.to_string()),
    ])))
}

/// The salt, iteration count and key length `tunnel` uses.
///
/// Shared with the compiler's lowering of the `tunnel` statement so the two
/// cannot drift: a different salt on one backend would derive a different key
/// from the same passphrase, which is silent and would look like a
/// cryptography bug rather than a compilation difference.
pub(crate) const TUNNEL_SALT: &[u8] = b"rak:secure-elb:tunnel";
pub(crate) const TUNNEL_ITERS: i64 = 100_000;
pub(crate) const TUNNEL_KEY_LEN: i64 = 32;

fn vm_udp_send(args: &Args) -> R {
    let transport = as_udp(args.first(), "udp_send")?;
    let data = to_bytes(args.get(1));
    let target = to_str(args.get(2));
    rak_stdlib::tunnel::udp_send(&transport, &data, &target)
        .map(|n| Value::I64(n as i64))
        .map_err(|e| e.to_string())
}

fn vm_udp_recv(args: &Args) -> R {
    let transport = as_udp(args.first(), "udp_recv")?;
    let max = args.get(1).and_then(|v| v.as_u64()).unwrap_or(65535) as usize;
    let timeout = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0);
    // A timeout is not an error: it yields `nil`, so a Rak program can poll a
    // transport in a loop without a `try`.
    match rak_stdlib::tunnel::udp_recv(&transport, max, timeout) {
        Ok(Some((data, addr))) => Ok(Value::Tuple(Arc::from(vec![
            b(data),
            s_owned(addr.to_string()),
        ]))),
        Ok(None) => Ok(Value::Nil),
        Err(e) => Err(e.to_string()),
    }
}

fn vm_udp_local_addr(args: &Args) -> R {
    let transport = as_udp(args.first(), "udp_local_addr")?;
    Ok(s_owned(rak_stdlib::tunnel::udp_local_addr(&transport)))
}

fn vm_zip_archive(args: &Args) -> R {
    // Accepts a map of name -> bytes or an array of [name, bytes] pairs, the
    // same two shapes the interpreter accepts.
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    match args.first() {
        Some(Value::Map(m)) => {
            for (k, v) in m.iter() {
                files.push((k.clone(), to_bytes(Some(v))));
            }
        }
        Some(Value::Array(a)) => {
            for item in a.iter() {
                if let Value::Tuple(t) = item {
                    if let (Some(Value::String(name)), Some(bytes)) = (t.get(0), t.get(1)) {
                        files.push((name.to_string(), to_bytes(Some(bytes))));
                    }
                }
            }
        }
        Some(other) => {
            return Err(format!(
                "zip_archive: expected a map/array, got {}",
                other.type_name()
            ))
        }
        None => return Err("zip_archive: expected a map/array, got nil".to_string()),
    }
    rak_stdlib::stream_io::zip_archive(files)
        .map(b)
        .map_err(|e| format!("zip_archive: {}", e))
}

fn vm_zip_extract(args: &Args) -> R {
    rak_stdlib::stream_io::zip_extract(&to_bytes(args.first()), &to_str(args.get(1)))
        .map(b)
        .map_err(|e| format!("zip_extract: {}", e))
}

fn vm_parse_csv_line(args: &Args) -> R {
    let line = to_str(args.first());
    let sep = match args.get(1) {
        Some(v) => to_str(Some(v)),
        None => ",".to_string(),
    };
    let delim = sep.chars().next().unwrap_or(',');
    Ok(strings(rak_stdlib::stream_io::parse_csv_line(&line, delim)))
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

/// Convert a parsed JSON value into a VM value. Shared with `ext_streams`,
/// which needs it for `stream_jsonl`.
///
/// Takes a reference: `stream_jsonl` has a borrowed `serde_json::Value` from the
/// line parser, and cloning the whole tree to convert it would defeat the point
/// of streaming line by line.
pub(crate) fn json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(x) => Value::Bool(*x),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::I64(i)
            } else if let Some(f) = n.as_f64() {
                Value::F64(f)
            } else {
                Value::Nil
            }
        }
        serde_json::Value::String(x) => s_owned(x.clone()),
        serde_json::Value::Array(a) => arr(a.iter().map(json_to_value).collect()),
        serde_json::Value::Object(o) => Value::Map(Arc::new(
            o.iter()
                .map(|(k, v)| (k.clone(), json_to_value(v)))
                .collect(),
        )),
    }
}

fn value_to_json(v: &Value) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        Value::Nil => J::Null,
        Value::Bool(x) => J::Bool(*x),
        Value::I64(i) => serde_json::json!(*i),
        Value::I8(i) => serde_json::json!(*i),
        Value::I16(i) => serde_json::json!(*i),
        Value::I32(i) => serde_json::json!(*i),
        Value::U8(i) => serde_json::json!(*i),
        Value::U16(i) => serde_json::json!(*i),
        Value::U32(i) => serde_json::json!(*i),
        Value::U64(i) => serde_json::json!(*i),
        Value::Hex(h) => serde_json::json!(*h),
        Value::F32(f) => serde_json::json!(*f),
        Value::F64(f) => serde_json::json!(*f),
        Value::String(x) => J::String(x.to_string()),
        Value::Bytes(x) => J::String(String::from_utf8_lossy(x).to_string()),
        Value::Array(a) => J::Array(a.iter().map(value_to_json).collect()),
        Value::Tuple(t) => J::Array(t.iter().map(value_to_json).collect()),
        Value::Map(m) => {
            let mut o = serde_json::Map::new();
            for (k, val) in m.iter() {
                o.insert(k.clone(), value_to_json(val));
            }
            J::Object(o)
        }
        Value::Struct { fields, .. } => {
            let mut o = serde_json::Map::new();
            for (k, val) in fields.iter() {
                o.insert(k.clone(), value_to_json(val));
            }
            J::Object(o)
        }
        Value::Option(Some(v)) => value_to_json(v),
        Value::Option(None) => J::Null,
        Value::Result(Some(v), _) => value_to_json(v),
        Value::Result(_, Some(e)) => value_to_json(e),
        Value::Evidence { inner, .. } => value_to_json(inner),
        _ => J::Null,
    }
}

fn vm_json_parse(args: &Args) -> R {
    rak_stdlib::js::json_parse(&to_str(args.first()))
        .map(|j| json_to_value(&j))
        .map_err(|e| e.to_string())
}

fn vm_json_stringify(args: &Args) -> R {
    let v = args.first().cloned().unwrap_or(Value::Nil);
    Ok(s_owned(value_to_json(&v).to_string()))
}

fn vm_json_get(args: &Args) -> R {
    rak_stdlib::js::json_get(&to_str(args.first()), &to_str(args.get(1)))
        .map(|x| s_owned(x))
        .map_err(|e| e.to_string())
}

fn vm_json_path(args: &Args) -> R {
    rak_stdlib::js::json_path(&to_str(args.first()), &to_str(args.get(1)))
        .map(|x| s_owned(x))
        .map_err(|e| e.to_string())
}

fn vm_json_keys(args: &Args) -> R {
    rak_stdlib::js::json_keys(&to_str(args.first()))
        .map(strings)
        .map_err(|e| e.to_string())
}

fn vm_json_len(args: &Args) -> R {
    rak_stdlib::js::json_len(&to_str(args.first()))
        .map(|n| Value::I64(n as i64))
        .map_err(|e| e.to_string())
}

fn vm_json_find_all(args: &Args) -> R {
    Ok(strings(rak_stdlib::js::json_find_all(
        &to_str(args.first()),
        &to_str(args.get(1)),
    )))
}

// ---------------------------------------------------------------------------
// HTML
// ---------------------------------------------------------------------------

fn vm_html_title(args: &Args) -> R {
    Ok(opt_str(rak_stdlib::web::html_title(&to_str(args.first()))))
}

fn vm_html_select(args: &Args) -> R {
    Ok(opt_str(rak_stdlib::web::html_select(
        &to_str(args.first()),
        &to_str(args.get(1)),
    )))
}

fn vm_html_select_all(args: &Args) -> R {
    Ok(strings(rak_stdlib::web::html_select_all(
        &to_str(args.first()),
        &to_str(args.get(1)),
    )))
}

fn vm_html_attr(args: &Args) -> R {
    Ok(opt_str(rak_stdlib::web::html_attr(
        &to_str(args.first()),
        &to_str(args.get(1)),
        &to_str(args.get(2)),
    )))
}

fn vm_html_links(args: &Args) -> R {
    Ok(strings(rak_stdlib::web::html_links(&to_str(args.first()))))
}

fn vm_html_images(args: &Args) -> R {
    Ok(strings(rak_stdlib::web::html_images(&to_str(args.first()))))
}

fn vm_html_scripts(args: &Args) -> R {
    Ok(strings(rak_stdlib::web::html_scripts(&to_str(
        args.first(),
    ))))
}

fn vm_html_forms(args: &Args) -> R {
    Ok(arr(rak_stdlib::web::html_forms(&to_str(args.first()))
        .into_iter()
        .map(str_map)
        .collect()))
}

fn vm_html_inputs(args: &Args) -> R {
    Ok(arr(rak_stdlib::web::html_inputs(&to_str(args.first()))
        .into_iter()
        .map(str_map)
        .collect()))
}

fn vm_html_meta(args: &Args) -> R {
    Ok(opt_str(rak_stdlib::web::html_meta(
        &to_str(args.first()),
        &to_str(args.get(1)),
    )))
}

fn vm_html_count(args: &Args) -> R {
    Ok(Value::I64(
        rak_stdlib::web::html_count(&to_str(args.first()), &to_str(args.get(1))) as i64,
    ))
}

fn vm_html_headers(args: &Args) -> R {
    Ok(strings(rak_stdlib::web::html_headers(&to_str(
        args.first(),
    ))))
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

fn vm_read(args: &Args) -> R {
    std::fs::read_to_string(to_str(args.first()))
        .map(|c| s_owned(c))
        .map_err(|e| format!("Read error: {}", e))
}

fn vm_write(args: &Args) -> R {
    std::fs::write(to_str(args.first()), to_str(args.get(1)))
        .map(|_| Value::Bool(true))
        .map_err(|e| format!("Write error: {}", e))
}

fn vm_file_read(args: &Args) -> R {
    rak_stdlib::file::read(&to_str(args.first()))
        .map(|c| s_owned(c))
        .map_err(|e| e.to_string())
}

fn vm_file_write(args: &Args) -> R {
    rak_stdlib::file::write(&to_str(args.first()), &to_str(args.get(1)))
        .map(|_| Value::Bool(true))
        .map_err(|e| e.to_string())
}

/// Read a whole file as raw bytes.
///
/// `vm_file_read` is `fs::read_to_string`, which *fails* on any file containing a
/// byte sequence that is not valid UTF-8 - so between them there was no way to
/// open a binary file on either backend.
/// Write one byte through a writable mapping.
///
/// Unlike `set_add`, this needs no `&mut Vm`: the handle already owns the mapping
/// behind an `Arc`, so the write is an ordinary native. The mapping is the file,
/// so there is nothing to flush and no second handle to keep in step - which is
/// the reason to write through the mapping rather than seek-and-write a path.
fn vm_mmap_write(args: &Args) -> R {
    let (handle, base) = match args.first() {
        Some(Value::Mmap(h)) => (h.clone(), 0usize),
        // A slice is a view, so an offset relative to it has to be rebased onto
        // the mapping before use.
        Some(Value::MmapSlice(h, off, _)) => (h.clone(), *off),
        Some(other) => {
            return Err(format!(
                "mmap_write: first argument must be a mapping from mmap_open, got {}",
                other.type_name()
            ))
        }
        None => return Err("mmap_write: expected a mapping".to_string()),
    };
    let off = base + to_offset(args.get(1))?;
    let byte = to_byte(args.get(2))?;
    rak_stdlib::mmap::write_byte(&handle, off, byte).map(|_| Value::Bool(true))
}

/// `bytes(x)` - coerce to a byte buffer.
///
/// Closed on the VM in this change. `to_bytes` now builds a real buffer from an
/// array of byte values rather than the array's text, which is what makes this
/// worth having on both sides: `bytes([0x89, 0x50])` was the 9-byte string
/// `[137, 80]` on the VM and a 2-byte buffer on the interpreter, so a program
/// reading a binary file could not hand the result anywhere on the VM.
fn vm_bytes(args: &Args) -> R {
    Ok(b(to_bytes(args.first())))
}

fn vm_file_read_bytes(args: &Args) -> R {
    rak_stdlib::file::read_bytes(&to_str(args.first()))
        .map(b)
        .map_err(|e| e.to_string())
}

fn vm_file_write_bytes(args: &Args) -> R {
    rak_stdlib::file::write_bytes(&to_str(args.first()), &to_bytes(args.get(1)))
        .map(|_| Value::Bool(true))
        .map_err(|e| e.to_string())
}

fn vm_file_append_bytes(args: &Args) -> R {
    rak_stdlib::file::append_bytes(&to_str(args.first()), &to_bytes(args.get(1)))
        .map(|_| Value::Bool(true))
        .map_err(|e| e.to_string())
}

fn vm_file_append(args: &Args) -> R {
    rak_stdlib::file::append(&to_str(args.first()), &to_str(args.get(1)))
        .map(|_| Value::Bool(true))
        .map_err(|e| e.to_string())
}

fn vm_file_exists(args: &Args) -> R {
    Ok(Value::Bool(rak_stdlib::file::exists(&to_str(args.first()))))
}

fn vm_file_size(args: &Args) -> R {
    Ok(Value::I64(
        rak_stdlib::file::size(&to_str(args.first())).unwrap_or(0) as i64,
    ))
}

fn vm_file_list(args: &Args) -> R {
    Ok(strings(rak_stdlib::file::list(&to_str(args.first()))))
}

fn vm_file_delete(args: &Args) -> R {
    Ok(Value::Bool(rak_stdlib::file::delete(&to_str(args.first()))))
}

fn vm_file_mkdir(args: &Args) -> R {
    Ok(Value::Bool(rak_stdlib::file::create_dir(&to_str(
        args.first(),
    ))))
}

fn vm_file_copy(args: &Args) -> R {
    Ok(Value::Bool(rak_stdlib::file::copy(
        &to_str(args.first()),
        &to_str(args.get(1)),
    )))
}

fn vm_file_rename(args: &Args) -> R {
    Ok(Value::Bool(rak_stdlib::file::rename(
        &to_str(args.first()),
        &to_str(args.get(1)),
    )))
}

fn vm_file_ext(args: &Args) -> R {
    Ok(s_owned(
        rak_stdlib::file::ext(&to_str(args.first())).unwrap_or_default(),
    ))
}

fn vm_file_basename(args: &Args) -> R {
    Ok(s_owned(rak_stdlib::file::basename(&to_str(args.first()))))
}

fn vm_file_dirname(args: &Args) -> R {
    Ok(s_owned(rak_stdlib::file::dirname(&to_str(args.first()))))
}

// ---------------------------------------------------------------------------
// Process and environment
// ---------------------------------------------------------------------------

fn vm_print(args: &Args) -> R {
    println!("{}", to_str(args.first()));
    use std::io::Write;
    // The interpreter flushes explicitly so that `print` interleaves correctly
    // with `dump` output when both are redirected to the same pipe.
    std::io::stdout().flush().ok();
    Ok(Value::Nil)
}

fn vm_eprint(args: &Args) -> R {
    eprintln!("{}", to_str(args.first()));
    Ok(Value::Nil)
}

fn vm_dbg(args: &Args) -> R {
    eprintln!("[dbg] {}", to_str(args.first()));
    Ok(Value::Nil)
}

fn vm_args(_args: &Args) -> R {
    Ok(strings(std::env::args().skip(1).collect::<Vec<_>>()))
}

fn vm_env_get(args: &Args) -> R {
    Ok(std::env::var(to_str(args.first()))
        .map(|x| s_owned(x))
        .unwrap_or(Value::Nil))
}

fn vm_env_set(args: &Args) -> R {
    // `set_var` is infallible today but is `unsafe` in edition 2024 for
    // process-safety reasons; this crate is on 2021, where it is safe. The
    // check below is kept so the intent is explicit if that ever changes.
    std::env::set_var(to_str(args.first()), to_str(args.get(1)));
    Ok(Value::Bool(true))
}

fn vm_stdin_read_line(_args: &Args) -> R {
    use std::io::Read;
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => Ok(Value::Nil),
        Ok(_) => Ok(s_owned(line.trim_end_matches('\n').to_string())),
        Err(e) => Err(format!("stdin_read_line: {}", e)),
    }
}

fn vm_stdin_read_all(_args: &Args) -> R {
    use std::io::Read;
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    Ok(s_owned(buf))
}

/// `parse_args(spec, argv) -> map`. `spec` maps a flag name to `"bool"`,
/// `"string"` or `"int"`; unlisted flags are treated as booleans. Positional
/// arguments land under the empty-string key.
fn vm_parse_args(args: &Args) -> R {
    let spec = match arg(args, 0) {
        Some(Value::Map(m)) => m.clone(),
        _ => return Err("parse_args: expected a spec map".to_string()),
    };
    let argv: Vec<String> = match arg(args, 1) {
        Some(Value::Array(a)) => a.iter().map(|v| v.to_string()).collect(),
        _ => std::env::args().skip(1).collect(),
    };

    let mut out: HashMap<String, Value> = HashMap::new();
    let mut positionals: Vec<Value> = Vec::new();
    let mut i = 0usize;
    while i < argv.len() {
        let a = argv[i].clone();
        if a == "--help" || a == "-h" {
            out.insert("help".to_string(), Value::Bool(true));
            i += 1;
            continue;
        }
        if let Some(body) = a.strip_prefix("--") {
            let (key, inline) = match body.find('=') {
                Some(pos) => (body[..pos].to_string(), Some(body[pos + 1..].to_string())),
                None => (body.to_string(), None),
            };
            let kind = spec
                .get(&key)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "bool".to_string());
            if inline.is_some() || kind == "bool" {
                out.insert(
                    key,
                    match inline {
                        Some(v) => s_owned(v),
                        None => Value::Bool(true),
                    },
                );
                i += 1;
                continue;
            }
            if i + 1 < argv.len() {
                let raw = argv[i + 1].clone();
                let val = if kind == "int" {
                    Value::I64(raw.parse::<i64>().unwrap_or(0))
                } else {
                    s_owned(raw)
                };
                out.insert(key, val);
                i += 2;
                continue;
            }
            out.insert(key, Value::Bool(true));
            i += 1;
            continue;
        }
        if a.starts_with('-') && a.len() > 1 {
            out.insert(a[1..].to_string(), Value::Bool(true));
            i += 1;
            continue;
        }
        positionals.push(s_owned(a));
        i += 1;
    }
    out.insert(String::new(), arr(positionals));
    Ok(Value::Map(Arc::new(out)))
}

// ---------------------------------------------------------------------------
// Assertions
// ---------------------------------------------------------------------------

fn vm_assert_eq(args: &Args) -> R {
    let a = args.first().cloned().unwrap_or(Value::Nil);
    let c = args.get(1).cloned().unwrap_or(Value::Nil);
    if a != c {
        return Err(format!("assertion failed: assert_eq({}, {})", a, c));
    }
    Ok(Value::Bool(true))
}

fn vm_assert_ne(args: &Args) -> R {
    let a = args.first().cloned().unwrap_or(Value::Nil);
    let c = args.get(1).cloned().unwrap_or(Value::Nil);
    if a == c {
        return Err(format!("assertion failed: assert_ne({}, {})", a, c));
    }
    Ok(Value::Bool(true))
}

fn vm_assert_true(args: &Args) -> R {
    let a = args.first().cloned().unwrap_or(Value::Nil);
    if !a.is_truthy() {
        return Err("assertion failed: assert_true()".to_string());
    }
    Ok(Value::Bool(true))
}

fn vm_assert_false(args: &Args) -> R {
    let a = args.first().cloned().unwrap_or(Value::Nil);
    if a.is_truthy() {
        return Err("assertion failed: assert_false()".to_string());
    }
    Ok(Value::Bool(true))
}

fn vm_panic(args: &Args) -> R {
    let msg = args
        .first()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "panic".to_string());
    Err(format!("panic: {}", msg))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// The native table registered into `Vm::register_natives`.
///
/// Kept as a table rather than inline `insert_native` calls so that this file
/// owns the whole family and `vm.rs` does not grow by a thousand lines. The
/// signature is the restrictive one — `fn(&[Value])`, no `&mut Vm` — which is
/// precisely the constraint that keeps this set to builtins that do not need to
/// call back into Rak.
pub fn vm_natives() -> Vec<(&'static str, fn(&[Value]) -> Result<Value, String>)> {
    vec![
        // math
        ("abs", vm_abs),
        ("sqrt", vm_sqrt),
        ("pow", vm_pow),
        ("min", vm_min),
        ("max", vm_max),
        ("clamp", vm_clamp),
        ("ord", vm_ord),
        ("chr", vm_chr),
        ("sum", vm_sum),
        // strings
        ("to_string", vm_to_string),
        ("split", vm_split),
        ("join", vm_join),
        ("replace", vm_replace),
        ("repeat", vm_repeat),
        ("starts_with", vm_starts_with),
        ("ends_with", vm_ends_with),
        ("find", vm_find),
        ("substr", vm_substr),
        ("trim", vm_trim),
        ("trim_start", vm_trim_start),
        ("trim_end", vm_trim_end),
        ("rot13", vm_rot13),
        ("url_encode", vm_url_encode),
        ("url_decode", vm_url_decode),
        // arrays
        ("sort", vm_sort),
        ("reverse", vm_reverse),
        ("push", vm_push),
        ("from_hex", vm_from_hex),
        // codecs
        ("xor", vm_xor),
        ("hex_decode", vm_hex_decode),
        ("base64_decode", vm_base64_decode),
        ("gzip", vm_gzip),
        ("gunzip", vm_gunzip),
        ("deflate", vm_deflate),
        ("inflate", vm_inflate),
        ("zip_archive", vm_zip_archive),
        ("zip_extract", vm_zip_extract),
        ("parse_csv_line", vm_parse_csv_line),
        // json
        ("json_parse", vm_json_parse),
        ("json_stringify", vm_json_stringify),
        ("json_get", vm_json_get),
        ("json_path", vm_json_path),
        ("json_keys", vm_json_keys),
        ("json_len", vm_json_len),
        ("json_find_all", vm_json_find_all),
        // html
        ("html_title", vm_html_title),
        ("html_select", vm_html_select),
        ("html_select_all", vm_html_select_all),
        ("html_attr", vm_html_attr),
        ("html_links", vm_html_links),
        ("html_images", vm_html_images),
        ("html_scripts", vm_html_scripts),
        ("html_forms", vm_html_forms),
        ("html_inputs", vm_html_inputs),
        ("html_meta", vm_html_meta),
        ("html_count", vm_html_count),
        ("html_headers", vm_html_headers),
        // files
        ("read", vm_read),
        ("write", vm_write),
        ("file_read", vm_file_read),
        ("file_write", vm_file_write),
        ("file_read_bytes", vm_file_read_bytes),
        ("mmap_write", vm_mmap_write),
        ("bytes", vm_bytes),
        ("file_write_bytes", vm_file_write_bytes),
        ("file_append_bytes", vm_file_append_bytes),
        ("file_append", vm_file_append),
        ("file_exists", vm_file_exists),
        ("file_size", vm_file_size),
        ("file_list", vm_file_list),
        ("file_delete", vm_file_delete),
        ("file_mkdir", vm_file_mkdir),
        ("file_copy", vm_file_copy),
        ("file_rename", vm_file_rename),
        ("file_ext", vm_file_ext),
        ("file_basename", vm_file_basename),
        ("file_dirname", vm_file_dirname),
        // process and environment
        ("print", vm_print),
        ("eprint", vm_eprint),
        ("dbg", vm_dbg),
        ("args", vm_args),
        ("env_get", vm_env_get),
        ("env_set", vm_env_set),
        ("stdin_read_line", vm_stdin_read_line),
        ("stdin_read_all", vm_stdin_read_all),
        ("parse_args", vm_parse_args),
        // assertions
        ("assert_eq", vm_assert_eq),
        ("assert_ne", vm_assert_ne),
        ("assert_true", vm_assert_true),
        ("assert_false", vm_assert_false),
        ("panic", vm_panic),
        // sets
        ("set_of", vm_set_of),
        ("set_has", vm_set_has),
        ("set_len", vm_set_len),
        ("set_has_all", vm_set_has_all),
        ("set_union", vm_set_union),
        ("set_intersect", vm_set_intersect),
        ("set_diff", vm_set_diff),
        ("set_to_array", vm_set_to_array),
        // `set_add` and `set_discard` are registered too, but only as
        // placeholders: the table's `fn(&[Value])` signature cannot mutate a
        // set, so they are intercepted in `Vm::call_value`. Registering them
        // keeps the name visible to the parity gate and to the LSP.
        ("set_add", vm_set_add),
        ("set_discard", vm_set_discard),
        // encrypted UDP
        ("udp_bind", vm_udp_bind),
        ("udp_send", vm_udp_send),
        ("udp_recv", vm_udp_recv),
        ("udp_local_addr", vm_udp_local_addr),
        // streams
        ("stream_from_array", vm_stream_from_array),
        ("stream_map", vm_stream_map),
        ("filter", vm_stream_filter),
        ("take", vm_stream_take),
        ("read_lines", vm_read_lines),
        ("tcp_stream", vm_tcp_stream),
        ("stream_csv", vm_stream_csv),
        ("stream_jsonl", vm_stream_jsonl),
        // `stream_next` and `collect` are intercepted in `Vm::call_value`:
        // both pull elements, and pulling past an element produced by `map` or
        // `filter` means calling a Rak function, which a `fn(&[Value])` native
        // cannot do. They are registered here as placeholders so the parity gate
        // and the LSP can see the names.
        ("stream_next", vm_stream_next_unreachable),
        ("collect", vm_stream_next_unreachable),
    ]
}

// ---------------------------------------------------------------------------
// Streams (spec 7A.4)
// ---------------------------------------------------------------------------

use crate::ext_streams as st;

/// Unwrap a stream argument.
///
/// The error text deliberately omits the builtin's name: `Vm::call_value`
/// already prefixes `"<name>: "`, and including it here produced
/// `filter: filter: expected a stream`. The interpreter's arms are written the
/// same way — with the name on some and not others — so the message here is
/// matched to what the interpreter produces for the same mistake.
fn as_stream(v: Option<&Value>, _who: &str) -> Result<st::VmStreamHandle, String> {
    match v {
        Some(Value::Stream(s)) => Ok(s.clone()),
        Some(other) => Err(format!("expected a stream, got {}", other.type_name())),
        None => Err("expected a stream".to_string()),
    }
}

fn fn_value(v: Option<&Value>, _who: &str) -> Result<Value, String> {
    match v {
        Some(f) => Ok(f.clone()),
        None => Err("expected a function argument".to_string()),
    }
}

fn stream_value(s: impl st::VmStream + 'static) -> Value {
    Value::Stream(st::handle(s))
}

fn vm_stream_from_array(args: &Args) -> R {
    let items = match args.first() {
        Some(Value::Array(a)) => a.to_vec(),
        Some(other) => return Err(format!("expected an array, got {}", other.type_name())),
        None => return Err("expected an array".to_string()),
    };
    Ok(stream_value(st::ArrayStream::new(items)))
}

fn vm_stream_map(args: &Args) -> R {
    let inner = as_stream(args.first(), "stream_map")?;
    let f = fn_value(args.get(1), "stream_map")?;
    Ok(stream_value(st::MapStream::new(inner, f)))
}

fn vm_stream_filter(args: &Args) -> R {
    let inner = as_stream(args.first(), "filter")?;
    let f = fn_value(args.get(1), "filter")?;
    Ok(stream_value(st::FilterStream::new(inner, f)))
}

fn vm_stream_take(args: &Args) -> R {
    let inner = as_stream(args.first(), "take")?;
    let n = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0);
    Ok(stream_value(st::TakeStream::new(inner, n)))
}

fn vm_read_lines(args: &Args) -> R {
    st::LinesStream::open(&to_str(args.first()))
        .map(stream_value)
        .map_err(|e| e)
}

fn vm_tcp_stream(args: &Args) -> R {
    let addr = to_str(args.first());
    use std::net::TcpStream;
    let s = TcpStream::connect(&addr).map_err(|e| format!("{}: {}", addr, e))?;
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    Ok(stream_value(st::TcpLineStream::open(s)))
}

fn vm_stream_csv(args: &Args) -> R {
    // The interpreter takes an options string and infers the dialect from it:
    // a `;` anywhere means semicolon-separated, `header` means the first row is
    // a header. Kept identical so the same program parses the same file on both
    // backends.
    let path = to_str(args.first());
    let opts = to_str(args.get(1));
    let delim = if opts.contains(';') { ';' } else { ',' };
    let has_header = opts.contains("header");
    let inner = st::LinesStream::open(&path).map_err(|e| e)?;
    Ok(stream_value(st::CsvStream::new(
        st::handle(inner),
        delim,
        has_header,
    )))
}

fn vm_stream_jsonl(args: &Args) -> R {
    let inner = st::LinesStream::open(&to_str(args.first())).map_err(|e| e)?;
    Ok(stream_value(st::JsonlStream::new(st::handle(inner))))
}

/// Placeholder for a stream builtin that must be intercepted in
/// `Vm::call_value`.
///
/// Reaching one of these means the interception is missing, which would
/// otherwise look like a silent `nil` rather than a bug — so it says so.
fn vm_stream_next_unreachable(_args: &Args) -> R {
    Err("stream: internal error, this builtin must be driven with VM access".to_string())
}
