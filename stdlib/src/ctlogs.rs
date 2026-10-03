//! Certificate-transparency subdomain discovery backed by crt.sh.
//!
//! `subdomains` performs a live `%domain` search against `https://crt.sh`
//! and returns deduplicated, lowercased subdomains; `parse_crtsh` parses the
//! JSON response body offline (unit-tested against fixtures).

use std::collections::BTreeSet;

/// Parse a crt.sh `?output=json` response body into deduplicated subdomains.
pub fn parse_crtsh(json: &str) -> anyhow::Result<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| anyhow::anyhow!("crt.sh returned invalid JSON: {}", e))?;
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("crt.sh response is not a JSON array"))?;
    let mut out: BTreeSet<String> = BTreeSet::new();
    for ent in arr {
        for field in ["name_value", "common_name"] {
            let Some(s) = ent.get(field).and_then(|x| x.as_str()) else {
                continue;
            };
            // name_value is often a \n-separated list.
            for part in s.split('\n') {
                let part = part.trim().trim_start_matches("*.");
                if part.is_empty() || !part.contains('.') {
                    continue;
                }
                out.insert(part.to_lowercase());
            }
        }
    }
    Ok(out.into_iter().collect())
}

/// Keep only names that are the apex or within `domain`'s tree.
pub fn filter_within(names: Vec<String>, domain: &str) -> Vec<String> {
    let domain = domain.trim_start_matches("*.").to_lowercase();
    let suffix = format!(".{}", domain);
    names
        .into_iter()
        .filter(|n| {
            let n = n.to_lowercase();
            n == domain || n.ends_with(&suffix)
        })
        .collect()
}

/// Search crt.sh for certificates matching `%.<domain>` and return the
/// deduplicated subdomains (including the bare apex for matched names).
pub fn subdomains(domain: &str) -> anyhow::Result<Vec<String>> {
    let domain = domain.trim().trim_start_matches("*.").to_lowercase();
    if domain.is_empty() {
        anyhow::bail!("ct: empty domain");
    }
    if !domain.contains('.') {
        anyhow::bail!("ct: '{}' is not a qualified domain name", domain);
    }
    let query = format!("%.{}", domain);
    let url = format!(
        "https://crt.sh/?q={}&output=json",
        urlencoding::encode(&query)
    );
    let resp = crate::net::http_get(&url, None)?;
    if resp.status >= 400 {
        anyhow::bail!("crt.sh returned HTTP {}", resp.status);
    }
    Ok(filter_within(parse_crtsh(&resp.body)?, &domain))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"[
  {"common_name": "example.com", "name_value": "example.com\nexample.com", "id": 1},
  {"common_name": "*.example.com", "name_value": "www.example.com\nmail.example.com", "id": 2},
  {"common_name": "api.example.com", "name_value": "api.example.com", "id": 3},
  {"common_name": "example.net", "name_value": "example.net", "id": 4}
]"#;

    #[test]
    fn parses_and_dedupes() {
        let subs = parse_crtsh(FIXTURE).unwrap();
        assert!(subs.contains(&"example.com".to_string()));
        assert!(subs.contains(&"www.example.com".to_string()));
        assert!(subs.contains(&"mail.example.com".to_string()));
        assert!(subs.contains(&"api.example.com".to_string()));
        // "example.com" appears twice in name_value; must be deduped.
        assert_eq!(
            subs.iter().filter(|s| s.as_str() == "example.com").count(),
            1
        );
        // The parser is generic; the caller filters to the queried tree.
        let within = filter_within(subs, "example.com");
        assert!(within.contains(&"www.example.com".to_string()));
        assert!(!within.contains(&"example.net".to_string()));
    }

    #[test]
    fn rejects_non_array() {
        assert!(parse_crtsh("{\"error\": \"boom\"}").is_err());
        assert!(parse_crtsh("not json at all").is_err());
    }
}
