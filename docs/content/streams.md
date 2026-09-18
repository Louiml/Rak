# Streams & data processing

## Streaming (0.7)

`Value::Stream` is a pull-based stream with a `next()` — natural backpressure.
`for item in stream` iterates lazily; nothing is fully loaded into memory.

| Builtin | Signature | Notes |
|---------|-----------|-------|
| `stream_from_array` | `stream_from_array(arr)` | Wrap an array in a stream. |
| `stream_map` | `stream_map(s, f)` | Lazy transform. |
| `filter` | `filter(s, f)` | Lazy predicate. |
| `take` | `take(s, n)` | First `n` items. |
| `stream_next` | `stream_next(s) -> value or nil` | Pull one item manually. |
| `collect` | `collect(s) -> array` | Drain to an array. |
| `read_lines` | `read_lines(path)` | Stream a file line by line. |
| `tcp_stream` | `tcp_stream(addr)` | Stream a TCP connection. |

```rak
let s = stream_from_array([1, 2, 3, 4, 5])
let evens = collect(take(filter(stream_map(s, fn(x) { return x * 2 }), fn(x) { return x % 4 == 0 }), 2))
dump evens

for line in read_lines("access.log") {
    if line |> regex_is_match(/"GET \/admin/g) { dump line }
}
```

Interpreter builtins; the VM omits `stream_*` with a clear error.

## CSV & JSONL

Lazy parsers (RFC-4180 CSV + JSON Lines) that never fully load files into
memory:

```rak
for row in stream_csv("data.csv", {}) {
    dump row.name          // header row becomes keys
}
for obj in stream_jsonl("events.jsonl") {
    dump obj.id
}
dump parse_csv_line("a,b,\"c,d\"")   // [a, b, c,d]
```

## Compression & archives (0.7)

```rak
let z = gzip(b"payload")          // gzip-compress bytes
dump gunzip(z)                    // b"payload"
dump deflate(b"payload")          // raw DEFLATE
dump inflate(deflate(b"payload"))

let za = zip_archive({ "a.txt": b"hello", "b.txt": b"world" })
dump zip_list(za)                 // [{name, size}, ...]
zip_extract(za, "./out")          // extract to a directory
```

## JSON

`json_parse` returns native arrays, maps, ints, floats, bools, nil;
`json_stringify` serializes back.

```rak
let j = json_parse("{\"user\": \"admin\", \"id\": 7}")
dump j.user                         // admin
dump j.id                           // 7
dump json_stringify(j)              // {"user":"admin","id":7}
```

Also available: `json_get(s, "key")`, `json_path`, `json_keys`, `json_len`,
`json_find_all`.

## String maps

String-keyed map literals are supported for JSON-style maps:

```rak
{ "Content-Type": "application/json" }
```
