//! Extended stdlib batteries: time/datetime, randomness, CSV/YAML, archives.
//!
//! Glue between `rak-stdlib`'s pure Rust modules (`timekit`, `randkit`,
//! `datafmt`, `archive`) and Rak values. Hooked into both backends:
//! - interpreter: `Interpreter::eval_builtin` tries [`try_interp`] first;
//! - VM: `Vm::register_natives` registers every entry from [`vm_natives`].
//!
//! Fallible builtins return `Ok(...)`/`Err(msg)` as Rak `Result` values on
//! both backends.

use crate::interpreter::Value as IV;
use crate::value::Value as VV;

// ------------------------------------------------------------- helpers ----

fn iv_int(i: i64) -> IV {
    IV::Int(i)
}
fn iv_ok(v: IV) -> IV {
    IV::Result(Some(Box::new(v)), None)
}
fn iv_err(msg: impl Into<String>) -> IV {
    IV::Result(None, Some(Box::new(IV::String(msg.into()))))
}
fn iv_err_res(msg: impl Into<String>) -> crate::Result<IV> {
    Ok(iv_err(msg))
}

fn vv_ok(v: VV) -> VV {
    VV::Result(Some(Box::new(v)), None)
}
fn vv_err(msg: impl Into<String>) -> VV {
    VV::Result(None, Some(Box::new(VV::String(std::sync::Arc::from(msg.into().as_str())))))
}

fn arg_count(name: &str, args: &[IV], min: usize, max: usize) -> Option<crate::Result<IV>> {
    if args.len() < min || args.len() > max {
        return Some(iv_err_res(format!(
            "{}() expects {} argument(s), got {}",
            name,
            if min == max { format!("{}", min) } else { format!("{}..{}", min, max) },
            args.len()
        )));
    }
    None
}

fn as_str<'a>(v: Option<&'a IV>, what: &str) -> Result<&'a str, crate::RakError> {
    match v {
        Some(IV::String(s)) => Ok(s),
        Some(IV::Bytes(b)) => std::str::from_utf8(b)
            .map_err(|_| crate::RakError::Runtime(format!("{}: invalid utf-8 bytes", what))),
        _ => Err(crate::RakError::Runtime(format!("{} expects a string", what))),
    }
}

fn as_i64(v: Option<&IV>, what: &str) -> Result<i64, crate::RakError> {
    match v {
        Some(IV::Int(i)) => Ok(*i),
        Some(IV::Float(f)) => Ok(*f as i64),
        Some(IV::Hex(h)) => Ok(*h as i64),
        _ => Err(crate::RakError::Runtime(format!("{} expects an int", what))),
    }
}

fn as_bytes(v: Option<&IV>, what: &str) -> Result<Vec<u8>, crate::RakError> {
    match v {
        Some(IV::Bytes(b)) => Ok(b.clone()),
        Some(IV::String(s)) => Ok(s.as_bytes().to_vec()),
        _ => Err(crate::RakError::Runtime(format!("{} expects string or bytes", what))),
    }
}

// ---------------------------------------------------- interpreter glue ----

fn time_interp(name: &str, args: &[IV]) -> Option<crate::Result<IV>> {
    use rak_stdlib::timekit;
    match name {
        "time_now" => Some(Ok(iv_int(timekit::now_unix()))),
        "time_now_millis" => Some(Ok(iv_int(timekit::now_millis()))),
        "time_fmt" => {
            if let Some(e) = arg_count(name, args, 2, 2) { return Some(e); }
            Some((|| {
                let ts = as_i64(args.first(), "time_fmt(ts, fmt)")?;
                let f = as_str(args.get(1), "time_fmt(ts, fmt)")?;
                Ok(IV::String(timekit::fmt(ts, f)))
            })())
        }
        "time_parse" => {
            if let Some(e) = arg_count(name, args, 1, 1) { return Some(e); }
            Some((|| {
                let s = as_str(args.first(), "time_parse(s)")?;
                Ok(match timekit::parse(s) {
                    Ok(ts) => iv_ok(iv_int(ts)),
                    Err(e) => iv_err(e),
                })
            })())
        }
        "time_parts" => {
            if let Some(e) = arg_count(name, args, 1, 1) { return Some(e); }
            Some((|| {
                let ts = as_i64(args.first(), "time_parts(ts)")?;
                let c = timekit::tm_from_unix(ts);
                let mut m = std::collections::HashMap::new();
                m.insert("year".to_string(), iv_int(c.year));
                m.insert("month".to_string(), iv_int(c.month));
                m.insert("day".to_string(), iv_int(c.day));
                m.insert("hour".to_string(), iv_int(c.hour));
                m.insert("minute".to_string(), iv_int(c.minute));
                m.insert("second".to_string(), iv_int(c.second));
                m.insert("weekday".to_string(), iv_int(c.weekday));
                m.insert("yday".to_string(), iv_int(c.yday));
                Ok(IV::Map(m))
            })())
        }
        "time_add" => {
            if let Some(e) = arg_count(name, args, 2, 2) { return Some(e); }
            Some((|| {
                let ts = as_i64(args.first(), "time_add(ts, secs)")?;
                let secs = as_i64(args.get(1), "time_add(ts, secs)")?;
                Ok(iv_int(ts + secs))
            })())
        }
        "time_diff" => {
            if let Some(e) = arg_count(name, args, 2, 2) { return Some(e); }
            Some((|| {
                let a = as_i64(args.first(), "time_diff(a, b)")?;
                let b = as_i64(args.get(1), "time_diff(a, b)")?;
                Ok(iv_int(a - b))
            })())
        }
        "date_today" => Some(Ok(IV::String(timekit::fmt(timekit::now_unix(), "%Y-%m-%d")))),
        _ => None,
    }
}


fn rand_interp(name: &str, args: &[IV]) -> Option<crate::Result<IV>> {
    use rak_stdlib::randkit;
    match name {
        "rand_seed" => {
            if let Some(e) = arg_count(name, args, 1, 1) { return Some(e); }
            Some((|| {
                let n = as_i64(args.first(), "rand_seed(n)")?;
                randkit::reseed(n);
                Ok(IV::Nil)
            })())
        }
        "rand_int" => {
            if let Some(e) = arg_count(name, args, 1, 2) { return Some(e); }
            Some((|| {
                let (lo, hi) = if args.len() == 2 {
                    (as_i64(args.first(), "rand_int(lo, hi)")?, as_i64(args.get(1), "rand_int(lo, hi)")?)
                } else {
                    (0, as_i64(args.first(), "rand_int(hi)")?)
                };
                // An empty range is a programming error — raise (like
                // division by zero), don't return an Err result.
                match randkit::gen_int(lo, hi) {
                    Ok(v) => Ok(iv_int(v)),
                    Err(e) => Err(crate::RakError::Runtime(e)),
                }
            })())
        }
        "rand_float" => Some(Ok(IV::Float(randkit::gen_float()))),
        "rand_bytes" => {
            if let Some(e) = arg_count(name, args, 1, 1) { return Some(e); }
            Some((|| {
                let n = as_i64(args.first(), "rand_bytes(n)")?.max(0) as usize;
                Ok(IV::Bytes(randkit::gen_bytes(n)))
            })())
        }
        "rand_hex" => {
            if let Some(e) = arg_count(name, args, 1, 1) { return Some(e); }
            Some((|| {
                let n = as_i64(args.first(), "rand_hex(n)")?.max(0) as usize;
                Ok(IV::String(randkit::gen_hex(n)))
            })())
        }
        "rand_choice" => {
            if let Some(e) = arg_count(name, args, 1, 1) { return Some(e); }
            Some((|| {
                let arr = match args.first() {
                    Some(IV::Array(a)) => a.clone(),
                    _ => return Err(crate::RakError::Runtime("rand_choice(arr) requires an array".to_string())),
                };
                Ok(match randkit::gen_index(arr.len()) {
                    Some(i) => iv_ok(arr[i].clone()),
                    None => iv_err("rand_choice: empty array"),
                })
            })())
        }
        "rand_shuffle" => {
            if let Some(e) = arg_count(name, args, 1, 1) { return Some(e); }
            Some((|| {
                let mut arr = match args.first() {
                    Some(IV::Array(a)) => a.clone(),
                    _ => return Err(crate::RakError::Runtime("rand_shuffle(arr) requires an array".to_string())),
                };
                randkit::gen_shuffle(&mut arr);
                Ok(IV::Array(arr))
            })())
        }
        _ => None,
    }
}

/// Convert a `YamlV` tree into interpreter values.
fn yamlv_to_iv(v: &rak_stdlib::datafmt::YamlV) -> IV {
    use rak_stdlib::datafmt::YamlV as Y;
    match v {
        Y::Null => IV::Nil,
        Y::Bool(b) => IV::Bool(*b),
        Y::Int(i) => iv_int(*i),
        Y::Float(f) => IV::Float(*f),
        Y::Str(s) => IV::String(s.clone()),
        Y::List(items) => IV::Array(items.iter().map(yamlv_to_iv).collect()),
        Y::Map(entries) => {
            let mut m = std::collections::HashMap::new();
            for (k, v) in entries {
                m.insert(k.clone(), yamlv_to_iv(v));
            }
            IV::Map(m)
        }
    }
}

/// Stringify one interpreter value for CSV cells.
fn iv_cell(v: &IV) -> String {
    match v {
        IV::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn data_archive_interp(name: &str, args: &[IV]) -> Option<crate::Result<IV>> {
    use rak_stdlib::{archive, datafmt};
    match name {
        "csv_parse" => {
            if let Some(e) = arg_count(name, args, 1, 2) { return Some(e); }
            Some((|| {
                let text = as_str(args.first(), "csv_parse(text)")?.to_string();
                let header = match args.get(1) {
                    Some(IV::Map(opts)) => matches!(opts.get("header"), Some(IV::Bool(false))),
                    _ => false,
                };
                // Malformed CSV raises (matches `json_parse` behavior).
                let rows = datafmt::parse_csv(&text).map_err(crate::RakError::Runtime)?;
                if header {
                    return Ok(IV::Array(
                        rows.into_iter().map(|r| IV::Array(r.into_iter().map(IV::String).collect())).collect(),
                    ));
                }
                if rows.is_empty() {
                    return Ok(IV::Array(vec![]));
                }
                let cols: Vec<String> = rows[0].clone();
                let mut out = Vec::new();
                for r in rows.into_iter().skip(1) {
                    let mut m = std::collections::HashMap::new();
                    for (i, c) in r.into_iter().enumerate() {
                        let key = cols.get(i).cloned().unwrap_or_else(|| format!("col{}", i));
                        m.insert(key, IV::String(c));
                    }
                    out.push(IV::Map(m));
                }
                Ok(IV::Array(out))
            })())
        }
        "csv_stringify" => {
            if let Some(e) = arg_count(name, args, 1, 1) { return Some(e); }
            Some((|| {
                let rows = match args.first() {
                    Some(IV::Array(items)) => items.clone(),
                    _ => return Err(crate::RakError::Runtime("csv_stringify(rows) requires an array".to_string())),
                };
                if rows.iter().all(|r| matches!(r, IV::Map(_))) {
                    // array of maps: columns from the first row's keys.
                    let mut cols: Vec<String> = Vec::new();
                    if let Some(IV::Map(first)) = rows.first() {
                        cols = first.keys().cloned().collect();
                        cols.sort();
                    }
                    let mut lines = vec![cols.clone()];
                    for r in &rows {
                        if let IV::Map(m) = r {
                            lines.push(cols.iter().map(|c| m.get(c).map(iv_cell).unwrap_or_default()).collect());
                        }
                    }
                    Ok(IV::String(datafmt::write_csv(&lines)))
                } else {
                    let lines: Vec<Vec<String>> = rows
                        .iter()
                        .map(|r| match r {
                            IV::Array(items) => items.iter().map(iv_cell).collect(),
                            IV::Tuple(items) => items.iter().map(iv_cell).collect(),
                            other => vec![iv_cell(other)],
                        })
                        .collect();
                    Ok(IV::String(datafmt::write_csv(&lines)))
                }
            })())
        }

        "yaml_parse" => {
            if let Some(e) = arg_count(name, args, 1, 1) { return Some(e); }
            Some((|| {
                let text = as_str(args.first(), "yaml_parse(text)")?.to_string();
                // Unsupported constructs raise with the offending line.
                let parsed = datafmt::parse_yaml(&text).map_err(crate::RakError::Runtime)?;
                Ok(yamlv_to_iv(&parsed))
            })())
        }
        "gzip_compress" => {
            if let Some(e) = arg_count(name, args, 1, 1) { return Some(e); }
            Some((|| {
                let data = as_bytes(args.first(), "gzip_compress(data)")?;
                Ok(IV::Bytes(archive::gzip_compress(&data)))
            })())
        }
        "gzip_decompress" => {
            if let Some(e) = arg_count(name, args, 1, 1) { return Some(e); }
            Some((|| {
                let data = as_bytes(args.first(), "gzip_decompress(bytes)")?;
                Ok(match archive::gzip_decompress(&data) {
                    Ok(v) => iv_ok(IV::Bytes(v)),
                    Err(e) => iv_err(e),
                })
            })())
        }
        "zip_list" => {
            if let Some(e) = arg_count(name, args, 1, 1) { return Some(e); }
            Some((|| {
                let path = as_str(args.first(), "zip_list(path)")?.to_string();
                Ok(match archive::zip_list(&path) {
                    Ok(items) => iv_ok(IV::Array(
                        items
                            .into_iter()
                            .map(|(name, size, compressed)| {
                                let mut m = std::collections::HashMap::new();
                                m.insert("name".to_string(), IV::String(name));
                                m.insert("size".to_string(), iv_int(size as i64));
                                m.insert("compressed".to_string(), iv_int(compressed as i64));
                                IV::Map(m)
                            })
                            .collect(),
                    )),
                    Err(e) => iv_err(e),
                })
            })())
        }
        "zip_read" => {
            if let Some(e) = arg_count(name, args, 2, 2) { return Some(e); }
            Some((|| {
                let path = as_str(args.first(), "zip_read(path, name)")?.to_string();
                let entry = as_str(args.get(1), "zip_read(path, name)")?.to_string();
                Ok(match archive::zip_read(&path, &entry) {
                    Ok(v) => iv_ok(IV::Bytes(v)),
                    Err(e) => iv_err(e),
                })
            })())
        }
        "zip_write" => {
            if let Some(e) = arg_count(name, args, 2, 2) { return Some(e); }
            Some((|| {
                let path = as_str(args.first(), "zip_write(path, entries)")?.to_string();
                let entries = zip_entries_iv(args.get(1))?;
                Ok(match archive::zip_write(&path, &entries) {
                    Ok(n) => iv_ok(iv_int(n as i64)),
                    Err(e) => iv_err(e),
                })
            })())
        }
        _ => None,
    }
}

/// Try to evaluate an extended builtin on the interpreter backend.
/// Returns `None` when `name` is not a batteries builtin.
pub fn try_interp(name: &str, args: &[IV]) -> Option<crate::Result<IV>> {
    time_interp(name, args)
        .or_else(|| rand_interp(name, args))
        .or_else(|| data_archive_interp(name, args))
}


/// Extract `entries` for zip_write: array of [name, content] pairs/tuples,
/// or a map name -> content.
fn zip_entries_iv(v: Option<&IV>) -> Result<Vec<(String, Vec<u8>)>, crate::RakError> {
    let mut out = Vec::new();
    match v {
        Some(IV::Array(items)) => {
            for it in items {
                match it {
                    IV::Array(pair) if pair.len() == 2 => {
                        out.push((iv_cell(&pair[0]), as_bytes(Some(&pair[1]), "zip_write entry content")?));
                    }
                    IV::Tuple(pair) if pair.len() == 2 => {
                        out.push((iv_cell(&pair[0]), as_bytes(Some(&pair[1]), "zip_write entry content")?));
                    }
                    _ => return Err(crate::RakError::Runtime("zip_write entries must be [name, content] pairs".to_string())),
                }
            }
        }
        Some(IV::Map(m)) => {
            for (k, val) in m {
                out.push((k.clone(), as_bytes(Some(val), "zip_write entry content")?));
            }
        }
        _ => return Err(crate::RakError::Runtime("zip_write(path, entries) expects an array or map of entries".to_string())),
    }
    Ok(out)
}


// ------------------------------------------------------------- VM glue ----

use std::sync::Arc;

fn vs(s: impl AsRef<str>) -> VV {
    VV::String(Arc::from(s.as_ref()))
}
fn vi(i: i64) -> VV {
    VV::I64(i)
}
fn vmap(pairs: Vec<(&str, VV)>) -> VV {
    let mut m = std::collections::HashMap::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    VV::Map(Arc::new(m))
}
fn varr(items: Vec<VV>) -> VV {
    VV::Array(Arc::new(items))
}

fn vs_str<'a>(v: Option<&'a VV>, what: &str) -> Result<&'a str, String> {
    match v {
        Some(VV::String(s)) => Ok(s),
        Some(VV::Bytes(b)) => std::str::from_utf8(b).map_err(|_| format!("{}: invalid utf-8 bytes", what)),
        _ => Err(format!("{} expects a string", what)),
    }
}
fn vs_i64(v: Option<&VV>, what: &str) -> Result<i64, String> {
    match v {
        Some(VV::I64(i)) => Ok(*i),
        Some(VV::F64(f)) => Ok(*f as i64),
        Some(VV::Hex(h, _)) => Ok(*h as i64),
        Some(VV::U64(u)) => Ok(*u as i64),
        _ => Err(format!("{} expects an int", what)),
    }
}
fn vs_bytes(v: Option<&VV>, what: &str) -> Result<Vec<u8>, String> {
    match v {
        Some(VV::Bytes(b)) => Ok(b.to_vec()),
        Some(VV::String(s)) => Ok(s.as_bytes().to_vec()),
        _ => Err(format!("{} expects string or bytes", what)),
    }
}

fn need(name: &str, args: &[VV], min: usize, max: usize) -> Result<(), String> {
    if args.len() < min || args.len() > max {
        Err(format!(
            "{}() expects {} argument(s), got {}",
            name,
            if min == max { format!("{}", min) } else { format!("{}..{}", min, max) },
            args.len()
        ))
    } else {
        Ok(())
    }
}

/// Convert a `YamlV` tree into VM values.
fn yamlv_to_vv(v: &rak_stdlib::datafmt::YamlV) -> VV {
    use rak_stdlib::datafmt::YamlV as Y;
    match v {
        Y::Null => VV::Nil,
        Y::Bool(b) => VV::Bool(*b),
        Y::Int(i) => vi(*i),
        Y::Float(f) => VV::F64(*f),
        Y::Str(s) => vs(s.clone()),
        Y::List(items) => varr(items.iter().map(yamlv_to_vv).collect()),
        Y::Map(entries) => {
            let mut m = std::collections::HashMap::new();
            for (k, v) in entries {
                m.insert(k.clone(), yamlv_to_vv(v));
            }
            VV::Map(Arc::new(m))
        }
    }
}

fn vv_cell(v: &VV) -> String {
    v.to_string()
}

fn vm_time_now(_a: &[VV]) -> Result<VV, String> { Ok(vi(rak_stdlib::timekit::now_unix())) }
fn vm_time_now_millis(_a: &[VV]) -> Result<VV, String> { Ok(vi(rak_stdlib::timekit::now_millis())) }
fn vm_time_fmt(a: &[VV]) -> Result<VV, String> {
    need("time_fmt", a, 2, 2)?;
    let ts = vs_i64(a.first(), "time_fmt(ts, fmt)")?;
    let f = vs_str(a.get(1), "time_fmt(ts, fmt)")?;
    Ok(vs(rak_stdlib::timekit::fmt(ts, f)))
}
fn vm_time_parse(a: &[VV]) -> Result<VV, String> {
    need("time_parse", a, 1, 1)?;
    let s = vs_str(a.first(), "time_parse(s)")?;
    Ok(match rak_stdlib::timekit::parse(s) {
        Ok(ts) => vv_ok(vi(ts)),
        Err(e) => vv_err(e),
    })
}
fn vm_time_parts(a: &[VV]) -> Result<VV, String> {
    need("time_parts", a, 1, 1)?;
    let c = rak_stdlib::timekit::tm_from_unix(vs_i64(a.first(), "time_parts(ts)")?);
    Ok(vmap(vec![
        ("year", vi(c.year)), ("month", vi(c.month)), ("day", vi(c.day)),
        ("hour", vi(c.hour)), ("minute", vi(c.minute)), ("second", vi(c.second)),
        ("weekday", vi(c.weekday)), ("yday", vi(c.yday)),
    ]))
}
fn vm_time_add(a: &[VV]) -> Result<VV, String> {
    need("time_add", a, 2, 2)?;
    Ok(vi(vs_i64(a.first(), "time_add")? + vs_i64(a.get(1), "time_add")?))
}
fn vm_time_diff(a: &[VV]) -> Result<VV, String> {
    need("time_diff", a, 2, 2)?;
    Ok(vi(vs_i64(a.first(), "time_diff")? - vs_i64(a.get(1), "time_diff")?))
}
fn vm_date_today(_a: &[VV]) -> Result<VV, String> {
    Ok(vs(rak_stdlib::timekit::fmt(rak_stdlib::timekit::now_unix(), "%Y-%m-%d")))
}
fn vm_rand_seed(a: &[VV]) -> Result<VV, String> {
    need("rand_seed", a, 1, 1)?;
    rak_stdlib::randkit::reseed(vs_i64(a.first(), "rand_seed(n)")?);
    Ok(VV::Nil)
}
fn vm_rand_int(a: &[VV]) -> Result<VV, String> {
    need("rand_int", a, 1, 2)?;
    let (lo, hi) = if a.len() == 2 {
        (vs_i64(a.first(), "rand_int")?, vs_i64(a.get(1), "rand_int")?)
    } else {
        (0, vs_i64(a.first(), "rand_int")?)
    };
    // An empty range is a programming error — raise, don't return Err.
    rak_stdlib::randkit::gen_int(lo, hi).map(vi)
}
fn vm_rand_float(_a: &[VV]) -> Result<VV, String> { Ok(VV::F64(rak_stdlib::randkit::gen_float())) }
fn vm_rand_bytes(a: &[VV]) -> Result<VV, String> {
    need("rand_bytes", a, 1, 1)?;
    let n = vs_i64(a.first(), "rand_bytes(n)")?.max(0) as usize;
    Ok(VV::Bytes(Arc::from(rak_stdlib::randkit::gen_bytes(n))))
}
fn vm_rand_hex(a: &[VV]) -> Result<VV, String> {
    need("rand_hex", a, 1, 1)?;
    let n = vs_i64(a.first(), "rand_hex(n)")?.max(0) as usize;
    Ok(vs(rak_stdlib::randkit::gen_hex(n)))
}
fn vm_rand_choice(a: &[VV]) -> Result<VV, String> {
    need("rand_choice", a, 1, 1)?;
    let arr = match a.first() {
        Some(VV::Array(v)) => v.to_vec(),
        _ => return Err("rand_choice(arr) requires an array".to_string()),
    };
    Ok(match rak_stdlib::randkit::gen_index(arr.len()) {
        Some(i) => vv_ok(arr[i].clone()),
        None => vv_err("rand_choice: empty array"),
    })
}
fn vm_rand_shuffle(a: &[VV]) -> Result<VV, String> {
    need("rand_shuffle", a, 1, 1)?;
    let mut arr = match a.first() {
        Some(VV::Array(v)) => v.to_vec(),
        _ => return Err("rand_shuffle(arr) requires an array".to_string()),
    };
    rak_stdlib::randkit::gen_shuffle(&mut arr);
    Ok(varr(arr))
}

fn vm_csv_parse(a: &[VV]) -> Result<VV, String> {
    use rak_stdlib::datafmt;
    need("csv_parse", a, 1, 2)?;
    let text = vs_str(a.first(), "csv_parse(text)")?.to_string();
    let header = match a.get(1) {
        Some(VV::Map(opts)) => matches!(opts.get("header"), Some(VV::Bool(false))),
        _ => false,
    };
    let rows = datafmt::parse_csv(&text)?;
    if header {
        return Ok(varr(rows.into_iter().map(|r| varr(r.into_iter().map(vs).collect())).collect()));
    }
    if rows.is_empty() {
        return Ok(varr(vec![]));
    }
    let cols: Vec<String> = rows[0].clone();
    let mut out = Vec::new();
    for r in rows.into_iter().skip(1) {
        let mut m = std::collections::HashMap::new();
        for (i, c) in r.into_iter().enumerate() {
            let key = cols.get(i).cloned().unwrap_or_else(|| format!("col{}", i));
            m.insert(key, vs(c));
        }
        out.push(VV::Map(Arc::new(m)));
    }
    Ok(varr(out))
}
fn vm_csv_stringify(a: &[VV]) -> Result<VV, String> {
    use rak_stdlib::datafmt;
    need("csv_stringify", a, 1, 1)?;
    let rows = match a.first() {
        Some(VV::Array(items)) => items.to_vec(),
        _ => return Err("csv_stringify(rows) requires an array".to_string()),
    };
    if rows.iter().all(|r| matches!(r, VV::Map(_))) {
        let mut cols: Vec<String> = Vec::new();
        if let Some(VV::Map(first)) = rows.first() {
            cols = first.keys().cloned().collect();
            cols.sort();
        }
        let mut lines = vec![cols.clone()];
        for r in &rows {
            if let VV::Map(m) = r {
                lines.push(cols.iter().map(|c| m.get(c).map(vv_cell).unwrap_or_default()).collect());
            }
        }
        Ok(vs(datafmt::write_csv(&lines)))
    } else {
        let lines: Vec<Vec<String>> = rows
            .iter()
            .map(|r| match r {
                VV::Array(items) => items.iter().map(vv_cell).collect(),
                VV::Tuple(items) => items.iter().map(vv_cell).collect(),
                other => vec![vv_cell(other)],
            })
            .collect();
        Ok(vs(datafmt::write_csv(&lines)))
    }
}
fn vm_yaml_parse(a: &[VV]) -> Result<VV, String> {
    need("yaml_parse", a, 1, 1)?;
    let text = vs_str(a.first(), "yaml_parse(text)")?.to_string();
    Ok(yamlv_to_vv(&rak_stdlib::datafmt::parse_yaml(&text)?))
}
fn vm_gzip_compress(a: &[VV]) -> Result<VV, String> {
    need("gzip_compress", a, 1, 1)?;
    let data = vs_bytes(a.first(), "gzip_compress(data)")?;
    Ok(VV::Bytes(Arc::from(rak_stdlib::archive::gzip_compress(&data))))
}
fn vm_gzip_decompress(a: &[VV]) -> Result<VV, String> {
    need("gzip_decompress", a, 1, 1)?;
    let data = vs_bytes(a.first(), "gzip_decompress(bytes)")?;
    Ok(match rak_stdlib::archive::gzip_decompress(&data) {
        Ok(v) => vv_ok(VV::Bytes(Arc::from(v))),
        Err(e) => vv_err(e),
    })
}

fn vm_zip_list(a: &[VV]) -> Result<VV, String> {
    need("zip_list", a, 1, 1)?;
    let path = vs_str(a.first(), "zip_list(path)")?.to_string();
    Ok(match rak_stdlib::archive::zip_list(&path) {
        Ok(items) => vv_ok(varr(
            items
                .into_iter()
                .map(|(name, size, compressed)| {
                    vmap(vec![("name", vs(name)), ("size", vi(size as i64)), ("compressed", vi(compressed as i64))])
                })
                .collect(),
        )),
        Err(e) => vv_err(e),
    })
}
fn vm_zip_read(a: &[VV]) -> Result<VV, String> {
    need("zip_read", a, 2, 2)?;
    let path = vs_str(a.first(), "zip_read")?.to_string();
    let entry = vs_str(a.get(1), "zip_read")?.to_string();
    Ok(match rak_stdlib::archive::zip_read(&path, &entry) {
        Ok(v) => vv_ok(VV::Bytes(Arc::from(v))),
        Err(e) => vv_err(e),
    })
}
fn vm_zip_write(a: &[VV]) -> Result<VV, String> {
    need("zip_write", a, 2, 2)?;
    let path = vs_str(a.first(), "zip_write(path)")?.to_string();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    match a.get(1) {
        Some(VV::Array(items)) => {
            for it in items.iter() {
                match it {
                    VV::Array(p) if p.len() == 2 => {
                        entries.push((vv_cell(&p[0]), vs_bytes(Some(&p[1]), "zip_write content")?));
                    }
                    VV::Tuple(p) if p.len() == 2 => {
                        entries.push((vv_cell(&p[0]), vs_bytes(Some(&p[1]), "zip_write content")?));
                    }
                    _ => return Err("zip_write entries must be [name, content] pairs".to_string()),
                }
            }
        }
        Some(VV::Map(m)) => {
            for (k, v) in m.iter() {
                entries.push((k.clone(), vs_bytes(Some(v), "zip_write content")?));
            }
        }
        _ => return Err("zip_write(path, entries) expects an array or map of entries".to_string()),
    }
    Ok(match rak_stdlib::archive::zip_write(&path, &entries) {
        Ok(n) => vv_ok(vi(n as i64)),
        Err(e) => vv_err(e),
    })
}

/// VM-side natives for the batteries builtins: `(name, fn)` pairs registered
/// by `Vm::register_natives`.
pub fn vm_natives() -> Vec<(&'static str, fn(&[VV]) -> Result<VV, String>)> {
    vec![
        ("time_now", vm_time_now),
        ("time_now_millis", vm_time_now_millis),
        ("time_fmt", vm_time_fmt),
        ("time_parse", vm_time_parse),
        ("time_parts", vm_time_parts),
        ("time_add", vm_time_add),
        ("time_diff", vm_time_diff),
        ("date_today", vm_date_today),
        ("rand_seed", vm_rand_seed),
        ("rand_int", vm_rand_int),
        ("rand_float", vm_rand_float),
        ("rand_bytes", vm_rand_bytes),
        ("rand_hex", vm_rand_hex),
        ("rand_choice", vm_rand_choice),
        ("rand_shuffle", vm_rand_shuffle),
        ("csv_parse", vm_csv_parse),
        ("csv_stringify", vm_csv_stringify),
        ("yaml_parse", vm_yaml_parse),
        ("gzip_compress", vm_gzip_compress),
        ("gzip_decompress", vm_gzip_decompress),
        ("zip_list", vm_zip_list),
        ("zip_read", vm_zip_read),
        ("zip_write", vm_zip_write),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile_module;

    /// Run a snippet through the interpreter backend.
    fn run_interp(src: &str) -> Vec<String> {
        crate::eval(src).unwrap()
    }

    /// Run a snippet through the VM backend.
    fn run_vm(src: &str) -> Vec<String> {
        let tokens = crate::lexer::tokenize(src).unwrap();
        let module = crate::parser::parse(&tokens, src).unwrap();
        let chunk = compile_module(&module).unwrap();
        let mut vm = crate::vm::Vm::new();
        vm.run(&chunk).unwrap()
    }

    const FUZZ: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn battery_time_parse_fmt_roundtrip() {
        for out in [run_interp(&format!(
            "let ts = time_parse(\"2023-11-14 22:13:20\")?\n\
             dump time_fmt(ts, \"%Y-%m-%d %H:%M:%S\")"
        )), run_vm(&format!(
            "let ts = time_parse(\"2023-11-14 22:13:20\")?\n\
             dump time_fmt(ts, \"%Y-%m-%d %H:%M:%S\")"
        ))] {
            assert!(out.iter().any(|l| l.contains("[DUMP] 2023-11-14 22:13:20")), "got: {:?}", out);
        }
    }

    #[test]
    fn battery_time_parts_and_epoch() {
        for out in [run_interp("let p = time_parts(1700000000)\ndump p.year\ndump p.month\ndump p.weekday"),
                    run_vm("let p = time_parts(1700000000)\ndump p.year\ndump p.month\ndump p.weekday")] {
            assert!(out.iter().any(|l| l.contains("[DUMP] 2023")), "got: {:?}", out);
            assert!(out.iter().any(|l| l.contains("[DUMP] 11")), "got: {:?}", out);
            assert!(out.iter().any(|l| l.contains("[DUMP] 2")), "got: {:?}", out);
        }
    }

    #[test]
    fn battery_time_epoch_zero() {
        assert!(run_interp("dump time_fmt(0, \"%Y-%m-%d\")").iter()
            .any(|l| l.contains("[DUMP] 1970-01-01")));
    }

    #[test]
    fn battery_rand_int_range_and_real_range() {
        let src = "rand_seed(33)\nlet mut ok = true\nfor i in 0..200 {\n let v = rand_int(-5, 5)\n if v < -5 || v >= 5 { ok = false }\n}\nlet f1 = rand_float()\nlet f2 = rand_float()\nif f1 >= 0.0 && f2 < 1.0 { ok = true }\ndump ok";
        for out in [run_interp(src), run_vm(src)] {
            assert!(out.iter().any(|l| l.contains("[DUMP] true")), "got: {:?}", out);
        }
    }

    #[test]
    fn battery_rand_helpers() {
        for out in [run_interp("rand_seed(9)\ndump len(rand_bytes(16))\ndump len(rand_hex(24))\ndump len(rand_shuffle([1, 2, 3, 4, 5, 6]))"),
                    run_vm("rand_seed(9)\ndump len(rand_bytes(16))\ndump len(rand_hex(24))\ndump len(rand_shuffle([1, 2, 3, 4, 5, 6]))")] {
            assert!(out.iter().any(|l| l.contains("[DUMP] 16")), "got: {:?}", out);
            assert!(out.iter().any(|l| l.contains("[DUMP] 24")), "got: {:?}", out);
            assert!(out.iter().any(|l| l.contains("[DUMP] 6")), "got: {:?}", out);
        }
    }

    #[test]
    fn battery_rand_choice_result() {
        for out in [run_interp("rand_seed(4)\nlet c = rand_choice([\"x\", \"y\"])?\ndump c"),
                    run_vm("rand_seed(4)\nlet c = rand_choice([\"x\", \"y\"])?\ndump c")] {
            assert!(out.iter().any(|l| l.contains("[DUMP] x") || l.contains("[DUMP] y")), "got: {:?}", out);
        }
    }

    #[test]
    fn battery_csv_parse_by_header() {
        let src = "let rows = csv_parse(\"host,port\\nwin.example,443\\n10.0.0.5,0x1F\\n\")\n\
                   dump rows[1].host\ndump rows[1].port\ndump len(rows)";
        for out in [run_interp(src), run_vm(src)] {
            assert!(out.iter().any(|l| l.contains("[DUMP] 10.0.0.5")), "got: {:?}", out);
            assert!(out.iter().any(|l| l.contains("[DUMP] 0x1F")), "got: {:?}", out);
            assert!(out.iter().any(|l| l.contains("[DUMP] 2")), "got: {:?}", out);
        }
    }

    #[test]
    fn battery_csv_no_header() {
        let src = "let rows = csv_parse(\"a,b\\n1,2\\n\", {header: false})\ndump rows[1][1]";
        for out in [run_interp(src), run_vm(src)] {
            assert!(out.iter().any(|l| l.contains("[DUMP] 2")), "got: {:?}", out);
        }
    }

    #[test]
    fn battery_csv_stringify_roundtrip() {
        let src = "let rows = csv_parse(\"x,y\\n\\\"\\\"quoted\\\"\\\",3\\n\")\ndump csv_stringify(rows)";
        for out in [run_interp(src), run_vm(src)] {
            assert!(out.iter().any(|l| l.contains("[DUMP] x,y")), "got: {:?}", out);
            assert!(out.iter().any(|l| l.contains("quoted,3")), "got: {:?}", out);
        }
    }

    #[test]
    fn battery_yaml_parse_nested() {
        let src = "let cfg = yaml_parse(\"target: 10.0.0.1\\nports:\\n  - 80\\n  - 443\\nstealth: true\\n\")?\n\
                   dump cfg.target\ndump cfg.ports[1]\ndump cfg.stealth";
        for out in [run_interp(src), run_vm(src)] {
            assert!(out.iter().any(|l| l.contains("[DUMP] 10.0.0.1")), "got: {:?}", out);
            assert!(out.iter().any(|l| l.contains("[DUMP] 443")), "got: {:?}", out);
            assert!(out.iter().any(|l| l.contains("[DUMP] true")), "got: {:?}", out);
        }
    }

    #[test]
    fn battery_gzip_roundtrip() {
        let src = format!(
            "let packed = gzip_compress(\"{}\")\n\
             dump len(packed) < 60\n\
             dump (gzip_decompress(packed)?) == b\"{}\"",
            FUZZ, FUZZ
        );
        for out in [run_interp(&src), run_vm(&src)] {
            assert!(out.iter().any(|l| l.contains("[DUMP] true")), "got: {:?}", out);
        }
    }

    #[test]
    fn battery_gzip_corrupt_input_errors() {
        let out = run_interp("let r = gzip_decompress(b\"not gzip data\")\ndump match r { Err => \"bad\", Ok => \"ok\" }");
        assert!(out.iter().any(|l| l.contains("[DUMP] bad")), "got: {:?}", out);
    }
}
