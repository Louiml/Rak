# Rak fuzzing targets (libFuzzer / cargo-fuzz)

Coverage-guided fuzz targets for Rak's security-sensitive components. They are a
companion to the proptest property tests in `rakc/tests/proptest_harness.rs`
(which run on the stable toolchain in CI). These libFuzzer targets run under
`cargo-fuzz`, which requires the **nightly** toolchain and a sanitizer-capable
compiler (Linux or WSL recommended; MSVC Windows does not support the required
`-fsanitize=fuzzer` passes).

The `fuzz/` crate is intentionally **not** part of the root Rak workspace, so the
standard `cargo build` / `cargo test` stay on stable.

## Targets

| Target | Exercises |
|--------|-----------|
| `lexer` | `rakc::lexer::tokenize` |
| `parser` | `tokenize` + `rakc::parser::parse` |
| `eval` | full interpreter (`rakc::eval`) on capped input |
| `manifest` | `rakpkg::parse_manifest_str` (package manifest) |
| `dns` | `dns::parse_response` |
| `tls` | `tls::parse_client_hello` + `parse_cert_chain` |
| `json` | `js::json_parse` + stringify + `json_keys/get/path/find_all` |
| `websocket` | `websocket::parse_frame` + `accept_value` |
| `tunnel` | `tunnel_unframe`, `x25519_keypair`, `chacha20_decrypt`, `psk_derive` |
| `netraw` | `net_raw::*` header builders + `csum16` |
| `csv` | CSV / JSONL per-line parsers |
| `gzip` | `gzip_decompress` + `deflate_decompress` |
| `zip` | `zip_list` + `zip_extract` |

## Running

Prereqs:

```bash
rustup toolchain install nightly
cargo install cargo-fuzz
```

Then run any target for a while:

```bash
cd fuzz
cargo +nightly fuzz run lexer -j8 -timeout=10 -max_len=4096
cargo +nightly fuzz run parser
cargo +nightly fuzz run dns
cargo +nightly fuzz run tls
# ... etc
```

`cargo-fuzz` maintains a growing corpus and writes crashing inputs to
`fuzz/artifacts/<target>/`. Any crash is a bug — file it as a release-blocker.

## Notes

- Forgiveness is enforced at the API boundary: every parser/decoder returns
  `Result`/`Option` and must never panic or read out of bounds on arbitrary input.
- Decompression (`gzip`/`zip`) targets rely on libFuzzer's RSS limit to contain
  decompression bombs; the underlying crates already bound output before OOM.
- `fuzz/corpus` and `fuzz/artifacts` are git-ignored build output.