# Standard library batteries

The "batteries" pack: time, seeded randomness, data formats and archives.
These are the builtins that turn a string-and-array language into one that
can process real files.

## Time and dates

```rak
let now = time_now()
dump time_fmt(now, "%Y-%m-%d %H:%M:%S")
dump date_today()
```

| Builtin | Does |
|---|---|
| `time_now()` | Unix seconds. |
| `time_now_millis()` | Unix milliseconds. |
| `time_fmt(ts, spec)` | Format a timestamp: `%Y %m %d %H %M %S %T %y %e %%`. |
| `time_parse(s)` | `Ok(ts)`. Accepts `YYYY-MM-DD` and `YYYY-MM-DD HH:MM:SS`. |
| `time_parts(ts)` | The calendar fields, as a map. |
| `time_add(ts, secs)` | Shift a timestamp. |
| `time_diff(a, b)` | `a - b`, in seconds. |
| `date_today()` | `YYYY-MM-DD`. |

The arithmetic is pure UTC civil time, so it does not depend on the machine's
timezone database.

## Randomness

```rak
rand_seed(7)                          // deterministic, for tests
dump rand_int(1, 100)
dump rand_float()
dump len(rand_bytes(16))
dump rand_hex(24)
dump rand_choice(["a", "b", "c"])?    // Ok(value)
rand_shuffle(list)
```

| Builtin | Does |
|---|---|
| `rand_seed(n)` | Reseed the generator. |
| `rand_int(lo, hi)` / `rand_int(hi)` | An integer in `[lo, hi)`. |
| `rand_float()` | A float in `[0.0, 1.0)`. |
| `rand_bytes(n)` | `n` random bytes. |
| `rand_hex(n)` | `n` random hex characters. |
| `rand_choice(arr)` | `Ok(item)`. |
| `rand_shuffle(arr)` | Shuffle in place, Fisher–Yates. |

`rand_int` raises on an empty range, naming it.

**The `rand` feature.** A build without it keeps the builtins registered and
substitutes a stub, and the behaviour is not uniform: `rand_int` raises
("this build has no random number generator"), while `rand_float` returns
`0.0` and `rand_bytes` returns nothing. The browser build has randomness —
the module asks the page for it — but a build with the feature off should
not be used for anything that needs a real number.

## CSV

```rak
let rows = csv_parse("host,port\nwin.example,443\n")
dump rows[0].host                    // win.example
dump csv_stringify(rows)
```

| Builtin | Does |
|---|---|
| `csv_parse(text, opts?)` | Rows as maps, with the header as the keys. `{header: false}` keeps every row as an array. |
| `csv_stringify(rows)` | Back to CSV, with the fields quoted where the format requires it. |
| `parse_csv_line(line, sep?)` | One line, into an array of fields. |

Malformed CSV raises, with the line number.

## YAML (a subset)

```rak
let doc = yaml_parse("name: rak\nport: 443\n")
dump doc.name
```

The subset is block maps and lists by indentation, scalars (`null`, `~`,
booleans, integers, floats, quoted and bare strings), and `#` comments. Flow
style (`{a: 1}`), anchors and aliases are not supported, and the error names
the line. Maps keep their order.

## gzip and zip

```rak
let packed = gzip("some bytes")
dump gunzip(packed)

let archive = zip_archive([("a.txt", "hello")])
dump zip_extract(archive, "a.txt")
```

| Builtin | Does |
|---|---|
| `gzip(b, level?)` / `gunzip(b)` | gzip. |
| `gzip_compress(b)` / `gzip_decompress(b)` | The batteries spellings; the second returns `Ok(bytes)`. |
| `deflate(b, level?)` / `inflate(b)` | Raw deflate. |
| `zip_archive(files)` | Build a zip in memory. |
| `zip_extract(zip, name)` | Read one entry. |
| `zip_list(path)` | The entries of a zip file, as `{name, size, compressed}`. |
| `zip_read(path, name)` / `zip_write(path, entries)` | Read and write zip files. |

Decompression is bounded at 100 MiB and checked while reading, so a
decompression bomb is refused rather than filling the disk. See
[Safety](safety.html#bounded-decompression).

## The other feature-gated families

Two more standard-library families are behind cargo features, and their
absence is worth knowing about:

- **`html`** — the HTML scraping builtins. A build without the feature keeps
  them registered and returns empty results: `html_title` gives nil,
  `html_count` gives 0, the rest give empty arrays. The reason is never
  surfaced, so a program that scrapes in such a build sees "nothing found"
  rather than "this build cannot".
- **`http`** — the `fetch` statement raises when the build has no HTTP
  client, and says so. `ct_subdomains` returns an `Err` naming the same
  thing.

The `html` family is the one to watch: it fails silently. If a program
depends on scraping, check that the build has the feature.

See [Standard library](stdlib.html) for the full catalogue.
