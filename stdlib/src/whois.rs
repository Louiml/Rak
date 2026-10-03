//! WHOIS: RFC 3912 lookups over TCP/43 plus a structured response parser.
//!
//! The parser is pure and fully testable offline; `lookup` performs real
//! network I/O against the registry for the domain's TLD (falling back to
//! IANA's refer server for unknown TLDs).

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const WHOIS_PORT: u16 = 43;
const TIMEOUT: Duration = Duration::from_secs(10);

/// Best-effort registry server per common TLD. Unknown TLDs hit IANA's refer
/// server, which answers with the correct registrar for the domain.
pub fn whois_server_for(domain: &str) -> &'static str {
    let tld = domain.rsplit('.').next().unwrap_or("");
    match tld {
        "com" | "net" => "whois.verisign-grs.com",
        "org" => "whois.pir.org",
        "io" => "whois.nic.io",
        "info" => "whois.afilias.net",
        "edu" => "whois.educause.edu",
        "gov" => "whois.nic.gov",
        "us" | "xyz" | "top" => "whois.nic.us",
        _ => "whois.iana.org",
    }
}

/// Query WHOIS over TCP/43. Returns the raw response text.
pub fn lookup(domain: &str) -> anyhow::Result<String> {
    let domain = domain.trim().trim_end_matches('.').to_lowercase();
    if domain.is_empty() {
        anyhow::bail!("whois: empty domain");
    }
    if !domain.contains('.') {
        anyhow::bail!("whois: '{}' is not a qualified domain name", domain);
    }
    let server = whois_server_for(&domain);
    let mut stream = TcpStream::connect((server, WHOIS_PORT))?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    writeln!(stream, "{}", domain)?;
    let mut buf = Vec::with_capacity(8192);
    stream.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf).to_string();
    if text.trim().is_empty() {
        anyhow::bail!("whois: server '{}' returned an empty response", server);
    }
    Ok(text)
}

/// Parse WHOIS response text into key/value fields.
///
/// Lines of the form `Key: value` are collected (case-insensitive keys with
/// the colon stripped, trimmed). Repeated keys keep the *last* value, except
/// multi-valued keys (`status`, `name server`, `dnssec`) which accumulate
/// space-separated.
pub fn parse(text: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut pending_key: Option<String> = None;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() && pending_key.is_none() {
            continue;
        }
        let Some(colon) = line.find(':') else {
            // Continuation line of the previous key.
            if let Some(k) = pending_key.take() {
                if let Some(entry) = out.get_mut(&k) {
                    entry.push(' ');
                    entry.push_str(line.trim());
                }
            }
            continue;
        };
        let key = line[..colon].trim().to_lowercase();
        let value = line[colon + 1..].trim().to_string();
        pending_key = Some(key.clone());
        if key.is_empty() {
            continue;
        }
        let multi = matches!(
            key.as_str(),
            "status" | "domain status" | "name server" | "dnssec"
        );
        match out.get_mut(&key) {
            Some(prev) if multi => {
                if !prev.split_whitespace().any(|v| v == value) {
                    prev.push(' ');
                    prev.push_str(&value);
                }
            }
            Some(prev) => *prev = value,
            None => {
                out.insert(key, value);
            }
        }
    }
    out
}

/// Convenience accessor for a parsed field.
pub fn get<'a>(fields: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    fields.get(key).map(|s| s.as_str())
}

/// `true` when the response text indicates the domain is not registered.
pub fn is_unregistered(text: &str) -> bool {
    let t = text.to_lowercase();
    [
        "no match",
        "not found",
        "not been registered",
        "no entries found",
        "not registered",
    ]
    .iter()
    .any(|needle| t.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
% IANA WHOIS server
% for more information on IANA, visit http://www.iana.org

Domain Name: EXAMPLE.COM
Registry Domain ID: 2336799_DOMAIN_COM-VRSN
Registrar WHOIS Server: whois.iana.org
Updated Date: 2023-08-14T07:01:37Z
Creation Date: 1995-08-14T04:00:00Z
Registry Expiry Date: 2024-08-13T04:00:00Z
Registrar: RESERVED-Internet Assigned Numbers Authority
Registrar IANA ID: 376
Domain Status: clientTransferProhibited https://icann.org/epp#clientTransferProhibited
Domain Status: serverDeleteProhibited https://icann.org/epp#serverDeleteProhibited
Name Server: A.IANA-SERVERS.NET
Name Server: B.IANA-SERVERS.NET
DNSSEC: signedDelegation
"#;

    #[test]
    fn parse_basic_fields() {
        let f = parse(FIXTURE);
        assert_eq!(
            f.get("domain name").map(String::as_str),
            Some("EXAMPLE.COM")
        );
        assert_eq!(
            f.get("creation date").map(String::as_str),
            Some("1995-08-14T04:00:00Z")
        );
        assert_eq!(
            f.get("registry expiry date").map(String::as_str),
            Some("2024-08-13T04:00:00Z")
        );
        assert_eq!(
            f.get("registrar").map(String::as_str),
            Some("RESERVED-Internet Assigned Numbers Authority")
        );
    }

    #[test]
    fn parse_multi_valued_fields_accumulate() {
        let f = parse(FIXTURE);
        let status = f.get("domain status").unwrap();
        assert!(status.contains("clientTransferProhibited"));
        assert!(status.contains("serverDeleteProhibited"));
        let ns = f.get("name server").unwrap();
        assert!(ns.contains("A.IANA-SERVERS.NET") && ns.contains("B.IANA-SERVERS.NET"));
    }

    #[test]
    fn parse_skips_comment_banner() {
        let f = parse("% IANA WHOIS server\n% for more information\n\nDomain Name: X");
        assert!(!f.contains_key("% iana whois server"));
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn unregistered_detection() {
        assert!(is_unregistered(
            "NOT FOUND\nNo match for \"does-not-exist.example\"."
        ));
        assert!(is_unregistered(
            "No entries found for the selected source(s)."
        ));
        assert!(!is_unregistered(FIXTURE));
    }
}
