# Batteries — time, randomness, data formats, archives

The batteries builtins cover the daily-driver tasks that scripts keep re-writing
by hand: civil date handling, a tiny deterministic RNG, CSV/YAML, and gzip/zip.
Everything is available on **both** backends (`rakc run` and `rakc vm`) with
identical semantics, and randomized functions are **seedable** so demos and tests
stay reproducible.

## Time & dates

The RNG's uniforms are the only source of nondeterminism here — but see
`rand_seed`.

**Note:** fallible calls return a `Result`, unwrapped with `?`:

```rak
let ts = time_parse("2023-11-14 22:13:20")?   // epoch seconds
dump time_fmt(ts, "%Y-%m-%d %H:%M:%S")         // 2023-11-14 22:13:20
dump time_fmt(0, "%Y-%m-%d (%T, %A)")          // 1970-01-01 (00:00:00, Thursday)
let p = time_parts(1700000000)                 // { year, month, day, hour, ... }
dump p.year                                    // 2023
dump p.weekday                                 // 2 (0 = Sunday)
dump date_today()                              // 2023-11-14
dump time_now()                                // seconds since the Unix epoch
dump time_now_millis()                         // milliseconds
dump time_add(100, 50)                        // 150
dump time_diff(150, 100)                      // 50
```

`time_fmt` supports the classic `strftime` specifiers (`%Y %m %d %H %M %S %T %A`),
and `time_parse` accepts `YYYY-MM-DD` and `YYYY-MM-DD HH:MM:SS`. `time_add` /
`time_diff` work on epoch seconds.

## Randomness

```rak
rand_seed(7)                     // deterministic stream for demos/tests
dump rand_int(0, 100)            // uniform in [0, 100)
dump rand_int(100)               // same as rand_int(0, 100)
dump rand_float()                // f64 in [0.0, 1.0)
dump len(rand_bytes(16))         // 16 random bytes
dump len(rand_hex(16))           // 16 hex chars
dump (rand_choice(["a", "b"])?)   // one element; Err on empty input
let xs = rand_shuffle([1, 2, 3])
```

`rand_int` raises on an empty range (a programming error, like division by zero)
instead of returning `Err`.

## CSV

```rak
let rows = csv_parse("host,port\nexample.com,443\n10.0.0.5,0x1F\n")
dump rows[1].host                // 10.0.0.5
dump rows[1].port                // 0x1F
dump len(rows)                   // 2 (header row consumed)

// Fields as arrays (no header interpretation):
let raw = csv_parse("a,b\n1,2\n", { header: false })
dump raw[1][1]                   // 2

// Serialize back. Arrays-of-maps re-emit headers; arrays-of-arrays don't.
dump csv_stringify(rows)
```

The parser is RFC-4180: quoted fields, embedded commas and `"` (doubled) are
handled. Malformed input **raises** (like `json_parse`).

## YAML (subset)

A pragmatic YAML subset: scalars (`bool/int/float/string`), nested maps, and
block-style lists.

```rak
let cfg = yaml_parse("target: 10.0.0.1\nports:\n  - 80\n  - 443\nstealth: true\n")?
dump cfg.target                  // 10.0.0.1
dump cfg.ports[1]                // 443
dump cfg.stealth                 // true
```

Unsupported constructs raise with the offending line. Use JSON for anything
more exotic.

## gzip

```rak
let packed = gzip_compress("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
dump len(packed) < 40            // true — repetition compresses well
dump (gzip_decompress(packed)?) == b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
```

`gzip_decompress` returns `Err` for corrupt input instead of raising, so pair it
with `?` when the data must be valid:

```rak
if let Ok(plain) = gzip_decompress(blob) { use plain }
```

## zip archives

`zip_list(path)` → array of `{ name, size, compressed }` maps. `zip_read` returns
the entry as bytes. `zip_write(path, entries)` writes `[name, content]` pairs
(arrays of pairs or a single map) and returns the entry count:

```rak
zip_write("scan.zip", {
  "a.txt": b"hello",
  "b.txt": "world data",
})
dump zip_list("scan.zip")        // [{name: "a.txt", size: 5, ...}, ...]
dump zip_read("scan.zip", "a.txt")
```

Archive errors (`Err`/`Ok`) are returned as Results, never raised.