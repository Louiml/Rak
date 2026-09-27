# OSINT toolkit

Passive/structured recon: WHOIS, certificate-transparency subdomain
discovery, a YARA-lite scanner, and Markdown findings reports. All builtins
run identically on `rakc run` and `rakc vm`.

## WHOIS

```rak
let raw = whois_lookup("example.com")?        // RFC 3912 over TCP/43
let w = whois_parse(raw)
dump w["creation date"]
dump w["registrar"]
dump w["name server"]           // multiple values accumulate
```

- `whois_lookup(domain)` → `Ok(raw text)` / `Err(message)` (needs network,
  uses the registrar for the domain's TLD).
- `whois_parse(text)` → map of lowercased, colon-stripped fields. Repeated
  keys (`status`, `domain status`, `name server`, `dnssec`) merge.

## Certificate transparency (crt.sh)

```rak
let subs = ct_subdomains("example.com")?
for s in subs { dump s }        // deduped subdomains incl. the apex
```

`ct_subdomains(domain)` queries `https://crt.sh` for `%.domain` and returns
deduplicated names within the domain tree.

## YARA-lite

A small, deterministic rule engine in the spirit of YARA:

```rak
let rules = "
rule pe_header {
    strings:
        $mz = { 4D 5A }
        $pe = { 50 45 00 00 }
    condition:
        $mz at 0 and $pe
}
rule api_hint {
    strings:
        $k = \"/api/v1\" nocase
    condition:
        $k
}
"
let hits = yara_scan(rules, b"MZ\x90\x00...PE\x00\x00")
for hit in hits {
    dump hit.rule                       // matched rule name
    for s in hit.strings { dump s.id, s.offset }
}
```

`yara_scan(rule_source, data)` returns an array of `{rule, strings: [{id,
offset}]}` maps. Supported surface:

- strings: text literals (`"..."`, optional `nocase`), hex literals
  (`{ 4D 5A ?? }`, `??` wildcards matching any byte), `\xNN` escapes
- conditions: `$id`, `$a at N`, `$a in (lo..hi)`, `all of them`,
  `any of them`, `none of them`, `all/any/none of ($a, $b)`, `and`/`or`/`not`,
  parenthesised groups

Bad rule syntax raises at runtime (like `json_parse`).

## Reports

Build a Markdown findings document from a plain map:

```rak
let rep = report_markdown({
  title: "Recon: example.com",
  meta: { target: "example.com", tool: "rakc", confidence: "high" },
  sections: {
    "Headline": ["registrar: RESERVED-Internet Assigned Numbers Authority"],
    "YARA": ["pe_header matched ($mz at 0)"],
  },
})
dump rep
```

Produces `# title`, a key/value `| field | value |` table from `meta`, and a
`## heading` + bullet list per `sections` entry (sorted by heading). Pipe
characters in values are escaped for the table.