'use client';

import React, { useMemo, useRef, useState } from 'react';
import { Icon } from './Icon';

interface DocEntry {
  title: string;
  kind: 'builtin' | 'keyword' | 'snippet' | 'guide';
  signature?: string;
  body: string;
}

const DOCS: DocEntry[] = [
  // --- Keywords / statements ---
  { title: 'let', kind: 'keyword', signature: 'let name = expr', body: 'Bind a value to a name. Also supported: `mut name = expr` for reassignable bindings.' },
  { title: 'fn', kind: 'keyword', signature: 'fn name(args) { ... }', body: 'Define a function. Returns the last expression or an explicit `return value`.' },
  { title: 'async fn', kind: 'keyword', signature: 'async fn name(args) { ... }', body: 'Define an async function that returns a `Future`. The body runs on the first `await`.' },
  { title: 'if / else', kind: 'keyword', signature: 'if cond { } else { }', body: 'Conditional branch. `cond` is any truthy value.' },
  { title: 'for / while / loop', kind: 'keyword', signature: 'for x in xs { } / while c { } / loop { }', body: 'Iteration. `for` iterates arrays/tuples/maps; `while` loops while a condition holds; `loop` loops forever (use `break`).' },
  { title: 'match', kind: 'keyword', signature: 'match value { pattern => { } }', body: 'Pattern match. Supports literals, arrays, byte/hex patterns `[0x89, ..]`, and a `_` catch-all arm.' },
  { title: 'struct / enum / impl', kind: 'keyword', signature: 'struct P { x } / enum E { A } / impl Trait for T { fn ... }', body: 'Define composite types, sum types, and trait method implementations (Display, Iterable, Index, IndexMut, etc.).' },
  { title: 'tunnel', kind: 'keyword', signature: 'tunnel <name> <passphrase> { ... }', body: 'Derive a 32-byte session key (PBKDF2-HMAC-SHA256) and open an encrypted UDP conduit. Inside the block binds `<name>` (session key), `<name>_udp`, `<name>_addr`, and `tunnel_key`.' },
  { title: 'binstruct', kind: 'keyword', signature: 'binstruct Name { field: u16be, ... }', body: 'Declarative wire-format layout that compiles to both a decoder (`binstruct.decode`) and an encoder (`.encode`) for binary/forensic parsing.' },
  { title: 'evidence', kind: 'keyword', signature: 'evidence<T> from expr', body: 'Provenance-tagged value for OSINT/forensic work. Chain additional provenance with `cite(value, tool, target)`.' },
  { title: 'scan / fetch / dump / trace', kind: 'keyword', signature: 'scan ... / fetch ... / dump expr / trace expr', body: 'OSINT verbs: `scan` port-scans a target, `fetch` performs an HTTP request, `dump` prints a value, `trace` logs a trace line.' },
  { title: 'import / from / export', kind: 'keyword', signature: 'import mod / from mod import name / export fn ...', body: 'Module system. Imports resolve relative to the importer, then `./packages/`, then `RAK_PATH`.' },
  { title: 'macro', kind: 'keyword', signature: 'macro name($param: expr) { ... }', body: 'AST-expanding macros. Placeholders are `$name`; invocations use `name!(...)`.' },
  { title: 'try / catch / raise / throw', kind: 'keyword', signature: 'try { } catch e { }', body: 'Error handling. Catches runtime errors; `raise`/`throw` raise a new error.' },

  // --- Core builtins ---
  { title: 'fmt', kind: 'builtin', signature: 'fmt("{} {}", a, b) -> string', body: 'Format a string with positional `{}` placeholders. Also supports Rust-style format specifiers like `{:02X}`.' },
  { title: 'len', kind: 'builtin', signature: 'len(x) -> int', body: 'Length of a string, array, tuple, bytes, or map.' },
  { title: 'dump', kind: 'builtin', signature: 'dump expr', body: 'Print a value to the console. Color-coded by the IDE console.' },
  { title: 'print', kind: 'builtin', signature: 'print(...)', body: 'Print to stdout.' },
  { title: 'hex_encode / hex_decode', kind: 'builtin', signature: 'hex_encode(bytes) -> string / hex_decode(s) -> bytes', body: 'Encode bytes to lowercase hex, or decode a hex string to bytes.' },
  { title: 'base64_encode / base64_decode', kind: 'builtin', signature: 'base64_encode(bytes) -> string', body: 'Base64 encode/decode.' },
  { title: 'string / int / float / bytes', kind: 'builtin', signature: 'string(x) / int(x)', body: 'Type conversions.' },
  { title: 'split / join / contains / replace / find', kind: 'builtin', signature: 'split(s, sep) / join(arr, sep) / contains(s, sub) / replace(s, from, to) / find(s, sub)', body: 'Common string utilities.' },
  { title: 'push / keys / values / has / get / sort', kind: 'builtin', signature: 'push(arr, v) / keys(map) / has(map, k)', body: 'Collection utilities for arrays and maps.' },
  { title: 'json_parse / json_stringify / json_get', kind: 'builtin', signature: 'json_parse(s) / json_get(s, "key")', body: 'JSON parsing and field access. `json_get` takes a raw JSON string and a key.' },
  { title: 'regex_new / regex_match / regex_find_all', kind: 'builtin', signature: 'regex_new(pat, flags) / regex_find_all(re, s)', body: 'Regular expression utilities. Regex literals `/pattern/flags` are also supported.' },

  // --- File / process ---
  { title: 'file_read / file_write', kind: 'builtin', signature: 'file_read(path) / file_write(path, data)', body: 'Read/write a file. Many other `file_*` helpers exist (list, delete, mkdir, copy, rename, size, exists, ext, basename, dirname).' },
  { title: 'process_spawn / process_wait / process_stdout', kind: 'builtin', signature: 'process_spawn(cmd, args)', body: 'Spawn and manage child processes.' },
  { title: 'secret_get / secret_set / secret_persist', kind: 'builtin', signature: 'secret_get(name)', body: 'Named secrets store (session -> env -> ~/.rak/secrets.json). Never hardcode real secrets.' },

  // --- Networking ---
  { title: 'net_listen / net_accept / net_connect', kind: 'builtin', signature: 'net_listen("ip:port") / net_accept(listener) / net_connect("ip:port")', body: 'TCP server/client. `net_accept` returns `(stream, peer_addr)`.' },
  { title: 'tcp_read / tcp_write / tcp_read_line / tcp_close', kind: 'builtin', signature: 'tcp_read(stream, n) / tcp_write(stream, data)', body: 'TCP stream I/O on a connection handle.' },
  { title: 'dns_query / dns_resolve / dns_records', kind: 'builtin', signature: 'dns_query(name, "A") / dns_resolve(name)', body: 'DNS lookups and record enumeration.' },
  { title: 'http_get_async / tcp_probe', kind: 'builtin', signature: 'await http_get_async(url) / await tcp_probe(host, port, ms)', body: 'Async HTTP and TCP probing that return a `Future`; use with `await`.' },
  { title: 'net_raw_ipv4 / net_raw_tcp / net_raw_udp / net_raw_send', kind: 'builtin', signature: 'net_raw_tcp_syn(src, dst, sport, dport) -> bytes', body: 'Packet forging (pure computation) and raw-socket send/recv (Unix: CAP_NET_RAW, Windows: Administrator). Header builders run anywhere.' },

  // --- VPN / tunneling ---
  { title: 'x25519_keypair', kind: 'builtin', signature: 'x25519_keypair(seed32) -> (pub, sec)', body: 'Deterministic X25519 (Curve25519) keypair from a 32-byte seed. Returns a tuple of (public, secret) bytes.' },
  { title: 'x25519_shared', kind: 'builtin', signature: 'x25519_shared(secret, peer_pub) -> bytes', body: 'Compute the ECDH shared secret between your secret key and a peer public key. Both sides derive the same 32-byte secret.' },
  { title: 'chacha20_encrypt', kind: 'builtin', signature: 'chacha20_encrypt(key32, nonce12, aad, data) -> bytes', body: 'ChaCha20-Poly1305 authenticated encryption. Returns ciphertext with a 16-byte auth tag appended.' },
  { title: 'chacha20_decrypt', kind: 'builtin', signature: 'chacha20_decrypt(key32, nonce12, aad, ct) -> bytes', body: 'ChaCha20-Poly1305 decrypt/verify. Raises an error on auth-tag mismatch or a bad key/nonce.' },
  { title: 'tunnel_preshared_key', kind: 'builtin', signature: 'tunnel_preshared_key(pass, salt, iters, len) -> bytes', body: 'Derive a session key from a passphrase via PBKDF2-HMAC-SHA256.' },
  { title: 'kdf_next', kind: 'builtin', signature: 'kdf_next(prev_key, counter, len) -> bytes', body: 'Derive a rolling per-packet ratchet key via HKDF-SHA256. Gives forward secrecy even if one packet key leaks.' },
  { title: 'tunnel_frame', kind: 'builtin', signature: 'tunnel_frame(seq, payload) -> bytes', body: 'Frame a datagram with an 8-byte big-endian sequence prefix: `[seq(8)] ++ payload`. Helps detect reordering/dropped frames.' },
  { title: 'tunnel_unframe', kind: 'builtin', signature: 'tunnel_unframe(frame) -> (seq, payload)', body: 'Parse a `tunnel_frame`. Returns `(sequence, payload)`.' },
  { title: 'tunnel_nonce', kind: 'builtin', signature: 'tunnel_nonce(seq) -> bytes(12)', body: 'Deterministic 12-byte AEAD nonce for a given sequence, avoiding nonce reuse under a single key.' },
  { title: 'udp_bind', kind: 'builtin', signature: 'udp_bind("ip:port") -> (transport, "ip:port")', body: 'Bind a UDP transport (the outer VPN conduit). Returns `(handle, bound_addr)`. Interpreter only.' },
  { title: 'udp_send', kind: 'builtin', signature: 'udp_send(transport, data, "ip:port") -> int', body: 'Send a datagram from a bound UDP transport to a target.' },
  { title: 'udp_recv', kind: 'builtin', signature: 'udp_recv(transport, max, timeout_ms) -> (data, "ip:port") | nil', body: 'Receive a datagram. Returns `nil` on timeout (after `timeout_ms`, 0 = block).' },
  { title: 'udp_local_addr', kind: 'builtin', signature: 'udp_local_addr(transport) -> string', body: 'The local bound "ip:port" of a UDP transport.' },

  // --- Guides ---
  { title: 'VPN overview', kind: 'guide', body: 'Rak ships an application-layer VPN toolkit: X25519 key agreement + ChaCha20-Poly1305 AEAD + HKDF/PBKDF2 key derivation + tunnel framing + a UDP transport. Everything runs at the application layer, so no TUN/TAP or raw socket is needed on Windows. See the `examples/VPN/` suite and the `tunnel` keyword.' },
  { title: 'VPN key exchange', kind: 'guide', body: '1) Both sides derive X25519 keypairs (`x25519_keypair`). 2) Exchange only public keys. 3) Each computes `x25519_shared(my_secret, peer_pub)` — both get the same 32-byte secret. 4) Feed it into `tunnel_preshared_key`/HKDF for an AEAD session key, then encrypt datagrams with `chacha20_encrypt` and frame them with `tunnel_frame`.' },
  { title: 'Tunnel encryption best practices', kind: 'guide', body: 'Never reuse a nonce under the same key — use `tunnel_nonce(seq)` with a monotonic sequence. Roll keys per packet with `kdf_next` for forward secrecy. Use a fresh random seed for ephemeral handshake keys. Pull passphrases from the secrets store, never hardcode them.' },
];

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

export default function DocsBrowser({ onClose, onInsert }: { onClose: () => void; onInsert?: (snippet: string) => void }) {
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<DocEntry | null>(null);
  const queryRef = useRef<HTMLInputElement>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return DOCS;
    return DOCS.filter((d) => {
      const hay = `${d.title} ${d.kind} ${d.signature ?? ''} ${d.body}`.toLowerCase();
      // All space-separated query tokens must appear (AND search).
      return q.split(/\s+/).every((tok) => hay.includes(tok));
    });
  }, [query]);

  const kindColor: Record<DocEntry['kind'], string> = {
    builtin: 'text-yellow-400',
    keyword: 'text-purple-400',
    snippet: 'text-green-400',
    guide: 'text-cyan-400',
  };

  return (
    <div className="flex flex-col h-full w-80 bg-zinc-900 border-l border-zinc-800 flex-shrink-0 text-xs">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 bg-zinc-800 border-b border-zinc-700">
        <span className="text-xs font-semibold text-zinc-300 flex items-center gap-2"><Icon name="book" size={13} className="text-emerald-400" /> Docs</span>
        <div className="flex items-center gap-2">
          <a
            href="https://rak-lang.dev"
            target="_blank"
            rel="noreferrer"
            className="text-[10px] text-zinc-500 hover:text-emerald-400"
            title="Open web documentation"
          >
            Web docs ↗
          </a>
          <button onClick={onClose} className="text-zinc-400 hover:text-zinc-200 px-1" title="Close docs (Ctrl+K)"><Icon name="x" size={13} /></button>
        </div>
      </div>

      {/* Search */}
      <div className="p-2 border-b border-zinc-800">
        <div className="flex items-center gap-2 bg-zinc-800 border border-zinc-700 rounded px-2 py-1 focus-within:border-emerald-500">
          <Icon name="search" size={13} className="text-zinc-500" />
          <input
            ref={queryRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search docs (e.g. chacha, tunnel)..."
            className="flex-1 bg-transparent text-zinc-200 outline-none placeholder:text-zinc-600"
          />
        </div>
      </div>

      {/* Results / detail */}
      <div className="flex-1 overflow-y-auto">
        {selected ? (
          <div className="p-3 space-y-2">
            <button onClick={() => setSelected(null)} className="text-zinc-500 hover:text-zinc-200"><Icon name="arrow-up" size={12} /></button>
            <div>
              <div className={`font-semibold ${kindColor[selected.kind]}`}>{selected.title}</div>
              {selected.signature && <div className="text-[11px] font-mono text-zinc-400 mt-1">{selected.signature}</div>}
            </div>
            <p className="text-zinc-300 leading-relaxed whitespace-pre-wrap">{selected.body}</p>
            {onInsert && (
              <button
                onClick={() => onInsert(selected.title)}
                className="mt-2 w-full text-center bg-zinc-800 hover:bg-emerald-600 hover:text-white text-zinc-300 rounded px-2 py-1"
              >
                Insert <span className="font-mono">{selected.title}</span>
              </button>
            )}
          </div>
        ) : filtered.length === 0 ? (
          <div className="p-4 text-zinc-600 text-center">No docs match &quot;{query}&quot;.</div>
        ) : (
          filtered.map((d) => (
            <button
              key={d.title + d.kind}
              onClick={() => setSelected(d)}
              className="w-full text-left px-3 py-1.5 hover:bg-zinc-800 flex items-start gap-2 border-b border-zinc-800/50"
            >
              <Icon name={d.kind === 'keyword' ? 'key' : d.kind === 'builtin' ? 'zap' : d.kind === 'guide' ? 'book' : 'file-code'} size={12} className="mt-0.5 text-zinc-500 flex-shrink-0" />
              <span className="min-w-0">
                <span className={`block truncate ${kindColor[d.kind]}`}>{d.title}</span>
                {d.signature && <span className="block truncate text-[10px] text-zinc-500 font-mono">{d.signature}</span>}
              </span>
            </button>
          ))
        )}
      </div>

      <div className="px-3 py-1.5 text-[10px] text-zinc-600 border-t border-zinc-800">
        {DOCS.length} entries · searchable reference
      </div>
    </div>
  );
}