//! OSINT pack: WHOIS, certificate-transparency subdomain enumeration,
//! YARA-lite scanning, and Markdown evidence reports.
//!
//! Same hook design as [`crate::ext_batteries`]: the interpreter tries
//! [`try_interp`] from `eval_builtin`, and the VM registers [`vm_natives`].
//! Network builtins (`whois_lookup`, `ct_subdomains`) run on both backends;
//! the pure ones (`whois_parse`, `yara_scan`, `report_markdown`) are identical.

use crate::interpreter::Value as IV;
use crate::value::Value as VV;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

// ------------------------------------------------------------- helpers ----

fn iv_ok(v: IV) -> IV {
    IV::Result(Some(Box::new(v)), None)
}
fn iv_err(msg: impl Into<String>) -> IV {
    IV::Result(None, Some(Box::new(IV::String(msg.into()))))
}

fn vv_ok(v: VV) -> VV {
    VV::Result(Some(Box::new(v)), None)
}
fn vv_err(msg: impl Into<String>) -> VV {
    VV::Result(None, Some(Box::new(VV::String(Arc::from(msg.into().as_str())))))
}

fn as_bytes(v: Option<&IV>, what: &str) -> Result<Vec<u8>, crate::RakError> {
    match v {
        Some(IV::Bytes(b)) => Ok(b.clone()),
        Some(IV::String(s)) => Ok(s.as_bytes().to_vec()),
        _ => Err(crate::RakError::Runtime(format!("{} expects string or bytes", what))),
    }
}

fn as_bytes_vm(v: Option<&VV>, what: &str) -> Result<Vec<u8>, String> {
    match v {
        Some(VV::Bytes(b)) => Ok(b.to_vec()),
        Some(VV::String(s)) => Ok(s.as_bytes().to_vec()),
        _ => Err(format!("{} expects string or bytes", what)),
    }
}

fn as_map<'a>(v: Option<&'a IV>, what: &str) -> Result<&'a HashMap<String, IV>, crate::RakError> {
    match v {
        Some(IV::Map(m)) => Ok(m),
        _ => Err(crate::RakError::Runtime(format!("{} expects a map", what))),
    }
}

fn as_map_vm<'a>(v: Option<&'a VV>, what: &str) -> Result<&'a HashMap<String, VV>, String> {
    match v {
        Some(VV::Map(m)) => Ok(m),
        _ => Err(format!("{} expects a map", what)),
    }
}

// ---------------------------------------------------- interpreter glue ----

fn whois_parse_interp(name: &str, args: &[IV]) -> Option<crate::Result<IV>> {
    if name != "whois_parse" {
        return None;
    }
    if args.len() != 1 {
        return Some(Err(crate::RakError::Runtime(
            "whois_parse(text) expects 1 argument".to_string(),
        )));
    }
    Some((|| {
        let text = match &args[0] {
            IV::String(s) => s.clone(),
            _ => {
                return Err(crate::RakError::Runtime(
                    "whois_parse(text) expects a string".to_string(),
                ))
            }
        };
        let fields = rak_stdlib::whois::parse(&text);
        let mut m = HashMap::new();
        for (k, v) in fields {
            m.insert(k, IV::String(v));
        }
        Ok(IV::Map(m))
    })())
}

fn iv_str_map_to_btree(m: &HashMap<String, IV>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (k, v) in m {
        out.insert(
            k.clone(),
            match v {
                IV::String(s) => s.clone(),
                other => other.to_string(),
            },
        );
    }
    out
}

fn iv_findings_to_markdown(map: &HashMap<String, IV>) -> Result<String, crate::RakError> {
    let title = match map.get("title") {
        Some(IV::String(s)) => s.clone(),
        Some(IV::Bytes(b)) => String::from_utf8_lossy(b).to_string(),
        _ => "Rak findings".to_string(),
    };
    let meta = match map.get("meta") {
        Some(IV::Map(m)) => iv_str_map_to_btree(m),
        _ => BTreeMap::new(),
    };
    let mut sections: Vec<rak_stdlib::report::Section> = Vec::new();
    if let Some(IV::Map(secs)) = map.get("sections") {
        let mut ordered: Vec<(&String, &IV)> = secs.iter().collect();
        ordered.sort_by(|a, b| a.0.cmp(b.0));
        for (heading, items) in ordered {
            let lines = match items {
                IV::Array(items) => items
                    .iter()
                    .map(|it| match it {
                        IV::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .collect(),
                IV::Tuple(items) => items
                    .iter()
                    .map(|it| match it {
                        IV::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .collect(),
                other => vec![other.to_string()],
            };
            sections.push((heading.clone(), lines));
        }
    }
    Ok(rak_stdlib::report::markdown_report(&title, &meta, &sections))
}

fn yara_scan_interp(rules_src: String, data: Vec<u8>) -> crate::Result<IV> {
    let rules = rak_stdlib::yara::compile(&rules_src).map_err(crate::RakError::Runtime)?;
    let matches = rak_stdlib::yara::scan(&rules, &data);
    let arr: Vec<IV> = matches
        .into_iter()
        .map(|m| {
            let mut mm = HashMap::new();
            mm.insert("rule".to_string(), IV::String(m.rule));
            let hits: Vec<IV> = m
                .hits
                .into_iter()
                .map(|(id, off)| {
                    let mut hm = HashMap::new();
                    hm.insert("id".to_string(), IV::String(id));
                    hm.insert("offset".to_string(), IV::Int(off as i64));
                    IV::Map(hm)
                })
                .collect();
            mm.insert("strings".to_string(), IV::Array(hits));
            IV::Map(mm)
        })
        .collect();
    Ok(IV::Array(arr))
}

fn osint_network_interp(name: &str, args: &[IV]) -> Option<crate::Result<IV>> {
    match name {
        "whois_lookup" => {
            if args.len() != 1 {
                return Some(Err(crate::RakError::Runtime(
                    "whois_lookup(domain) expects 1 argument".to_string(),
                )));
            }
            Some((|| {
                let domain = match &args[0] {
                    IV::String(s) => s.clone(),
                    _ => {
                        return Err(crate::RakError::Runtime(
                            "whois_lookup(domain) expects a string".to_string(),
                        ))
                    }
                };
                match rak_stdlib::whois::lookup(&domain) {
                    Ok(text) => Ok(iv_ok(IV::String(text))),
                    Err(e) => Ok(iv_err(e.to_string())),
                }
            })())
        }
        "ct_subdomains" => {
            if args.len() != 1 {
                return Some(Err(crate::RakError::Runtime(
                    "ct_subdomains(domain) expects 1 argument".to_string(),
                )));
            }
            Some((|| {
                let domain = match &args[0] {
                    IV::String(s) => s.clone(),
                    _ => {
                        return Err(crate::RakError::Runtime(
                            "ct_subdomains(domain) expects a string".to_string(),
                        ))
                    }
                };
                match rak_stdlib::ctlogs::subdomains(&domain) {
                    Ok(list) => Ok(iv_ok(IV::Array(list.into_iter().map(IV::String).collect()))),
                    Err(e) => Ok(iv_err(e.to_string())),
                }
            })())
        }
        _ => None,
    }
}

/// Try to evaluate an OSINT builtin on the interpreter backend.
pub fn try_interp(name: &str, args: &[IV]) -> Option<crate::Result<IV>> {
    if let Some(r) = whois_parse_interp(name, args) {
        return Some(r);
    }
    if let Some(r) = osint_network_interp(name, args) {
        return Some(r);
    }
    match name {
        "yara_scan" => Some((|| {
            if args.len() != 2 {
                return Err(crate::RakError::Runtime(
                    "yara_scan(rule_source, data) expects 2 arguments".to_string(),
                ));
            }
            let rules_src = match &args[0] {
                IV::String(s) => s.clone(),
                _ => {
                    return Err(crate::RakError::Runtime(
                        "yara_scan(rule_source, data): rule_source expects a string".to_string(),
                    ))
                }
            };
            let data = as_bytes(args.get(1), "yara_scan(rule_source, data)")?;
            yara_scan_interp(rules_src, data)
        })()),
        "report_markdown" => Some((|| {
            if args.len() != 1 {
                return Err(crate::RakError::Runtime(
                    "report_markdown(report) expects 1 argument".to_string(),
                ));
            }
            let map = as_map(args.first(), "report_markdown(report)")?;
            Ok(IV::String(iv_findings_to_markdown(map)?))
        })()),
        _ => None,
    }
}

// ------------------------------------------------------------ VM glue -----

fn vmap(pairs: Vec<(&str, VV)>) -> VV {
    let mut m = HashMap::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    VV::Map(Arc::new(m))
}

fn vm_whois_parse(a: &[VV]) -> Result<VV, String> {
    let text = match a.first() {
        Some(VV::String(s)) => s.to_string(),
        _ => return Err("whois_parse(text) expects a string".to_string()),
    };
    let fields = rak_stdlib::whois::parse(&text);
    let mut m = HashMap::new();
    for (k, v) in fields {
        m.insert(k, VV::String(Arc::from(v.as_str())));
    }
    Ok(VV::Map(Arc::new(m)))
}

fn vm_whois_lookup(a: &[VV]) -> Result<VV, String> {
    if a.len() != 1 {
        return Err("whois_lookup(domain) expects 1 argument".to_string());
    }
    let domain = match a.first() {
        Some(VV::String(s)) => s.to_string(),
        _ => return Err("whois_lookup(domain) expects a string".to_string()),
    };
    Ok(match rak_stdlib::whois::lookup(&domain) {
        Ok(text) => vv_ok(VV::String(Arc::from(text.as_str()))),
        Err(e) => vv_err(e.to_string()),
    })
}

fn vm_ct_subdomains(a: &[VV]) -> Result<VV, String> {
    if a.len() != 1 {
        return Err("ct_subdomains(domain) expects 1 argument".to_string());
    }
    let domain = match a.first() {
        Some(VV::String(s)) => s.to_string(),
        _ => return Err("ct_subdomains(domain) expects a string".to_string()),
    };
    match rak_stdlib::ctlogs::subdomains(&domain) {
        Ok(list) => Ok(vv_ok(VV::Array(Arc::new(
            list.into_iter().map(|s| VV::String(Arc::from(s.as_str()))).collect(),
        )))),
        Err(e) => Ok(vv_err(e.to_string())),
    }
}

fn vm_yara_scan(a: &[VV]) -> Result<VV, String> {
    if a.len() != 2 {
        return Err("yara_scan(rule_source, data) expects 2 arguments".to_string());
    }
    let rules_src = match a.first() {
        Some(VV::String(s)) => s.to_string(),
        _ => return Err("yara_scan(rule_source, data): rule_source expects a string".to_string()),
    };
    let data = as_bytes_vm(a.get(1), "yara_scan(rule_source, data)")?;
    let rules = rak_stdlib::yara::compile(&rules_src).map_err(|e| format!("yara: {}", e))?;
    let matches = rak_stdlib::yara::scan(&rules, &data);
    let arr: Vec<VV> = matches
        .into_iter()
        .map(|m| {
            let hits_v: Vec<VV> = m
                .hits
                .into_iter()
                .map(|(id, off)| {
                    vmap(vec![
                        ("id", VV::String(Arc::from(id.as_str()))),
                        ("offset", VV::I64(off as i64)),
                    ])
                })
                .collect();
            vmap(vec![
                ("rule", VV::String(Arc::from(m.rule.as_str()))),
                ("strings", VV::Array(Arc::new(hits_v))),
            ])
        })
        .collect();
    Ok(VV::Array(Arc::new(arr)))
}

fn vm_report_markdown(a: &[VV]) -> Result<VV, String> {
    if a.len() != 1 {
        return Err("report_markdown(report) expects 1 argument".to_string());
    }
    let map = as_map_vm(a.first(), "report_markdown(report)")?;
    let title = match map.get("title") {
        Some(VV::String(s)) => s.to_string(),
        _ => "Rak findings".to_string(),
    };
    let meta = match map.get("meta") {
        Some(VV::Map(m)) => {
            let mut b = BTreeMap::new();
            for (k, v) in m.iter() {
                b.insert(k.clone(), match v {
                    VV::String(s) => s.to_string(),
                    other => other.to_string(),
                });
            }
            b
        }
        _ => BTreeMap::new(),
    };
    let mut sections: Vec<rak_stdlib::report::Section> = Vec::new();
    if let Some(VV::Map(secs)) = map.get("sections") {
        let mut ordered: Vec<(&String, &VV)> = secs.iter().collect();
        ordered.sort_by(|a, b| a.0.cmp(b.0));
        for (heading, items) in ordered {
            let lines = match items {
                VV::Array(items) => items
                    .iter()
                    .map(|it| match it {
                        VV::String(s) => s.to_string(),
                        other => other.to_string(),
                    })
                    .collect(),
                other => vec![other.to_string()],
            };
            sections.push((heading.clone(), lines));
        }
    }
    Ok(VV::String(Arc::from(
        rak_stdlib::report::markdown_report(&title, &meta, &sections).as_str(),
    )))
}

/// VM-side OSINT natives.
pub fn vm_natives() -> Vec<(&'static str, fn(&[VV]) -> Result<VV, String>)> {
    vec![
        ("whois_lookup", vm_whois_lookup),
        ("whois_parse", vm_whois_parse),
        ("ct_subdomains", vm_ct_subdomains),
        ("yara_scan", vm_yara_scan),
        ("report_markdown", vm_report_markdown),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile_module;

    fn run_interp(src: &str) -> Vec<String> {
        crate::eval(src).unwrap()
    }

    fn run_vm(src: &str) -> Vec<String> {
        let tokens = crate::lexer::tokenize(src).unwrap();
        let module = crate::parser::parse(&tokens, src).unwrap();
        let chunk = compile_module(&module).unwrap();
        let mut vm = crate::vm::Vm::new();
        vm.run(&chunk).unwrap()
    }

    const RULES: &str = "rule pe {\n    strings:\n        $mz = { 4D 5A }\n    condition:\n        $mz at 0\n}\n";

    #[test]
    fn yara_scan_both_backends() {
        let src = format!(
            "let data = b\"MZ\\x90\\x00PE\\x00\\x00\"\n\
             let hits = yara_scan(\"{}\", data)\n\
             dump len(hits)\ndump hits[0].rule\ndump (hits[0].strings[0].id) + \"@\" + string(hits[0].strings[0].offset)",
            RULES
        );
        for out in [run_interp(&src), run_vm(&src)] {
            assert!(out.iter().any(|l| l.contains("[DUMP] 1")), "got: {:?}", out);
            assert!(out.iter().any(|l| l.contains("[DUMP] pe")), "got: {:?}", out);
            assert!(out.iter().any(|l| l.contains("[DUMP] mz@0")), "got: {:?}", out);
        }
    }

    #[test]
    fn whois_parse_both_backends() {
        let src = "let w = whois_parse(\"Domain Name: X.COM\\nCreation Date: 2020-01-02T00:00:00Z\\nRegistrar: Acme\\n\")\ndump w[\"creation date\"]\ndump w[\"registrar\"]";
        for out in [run_interp(src), run_vm(src)] {
            assert!(out.iter().any(|l| l.contains("[DUMP] 2020-01-02T00:00:00Z")), "got: {:?}", out);
            assert!(out.iter().any(|l| l.contains("[DUMP] Acme")), "got: {:?}", out);
        }
    }

    #[test]
    fn report_markdown_both_backends() {
        let src = "let rep = report_markdown({title: \"T\", meta: {target: \"x.com\"}, sections: {\"Recon\": [\"a\", \"b\"]}})\ndump rep[..10]\ndump \"## Recon\" in rep";
        for out in [run_interp(src), run_vm(src)] {
            assert!(out.iter().any(|l| l.contains("[DUMP] # T")), "got: {:?}", out);
            assert!(out.iter().any(|l| l.contains("[DUMP] true")), "got: {:?}", out);
        }
    }

    #[test]
    fn yara_scan_invalid_rules_raise() {
        let src = "let hits = yara_scan(\"this is not yara\", b\"abc\")\ndump len(hits)";
        let err_interp = crate::eval(src).unwrap_err();
        assert!(err_interp.to_string().contains("yara"), "got: {:?}", err_interp);
    }

    #[test]
    fn network_builtins_validate_args() {
        // Argument validation happens before any network I/O.
        let err = crate::eval("whois_lookup(123)").unwrap_err();
        assert!(err.to_string().contains("whois_lookup(domain) expects a string"));
    }
}
