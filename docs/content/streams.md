# Streams and data

## Streaming

A stream is a lazy sequence. It is pulled one value at a time, so a file
larger than memory is not a problem, and nothing happens until the values are
consumed.

```rak
let s = stream_from_array([1, 2, 3])
dump stream_map(s, fn(x) { return x * 2 })
dump collect(stream_map(s, fn(x) { return x * 2 }))   // [2, 4, 6]
```

| Builtin | Does |
|---|---|
| `stream_from_array(arr)` | A stream over an array. |
| `stream_map(s, f)` | Map each value. |
| `filter(s, f)` | Keep the values where `f` holds. |
| `take(s, n)` | The first n values. |
| `stream_next(s)` | `Some(value)`, or `None` at the end. |
| `collect(s)` | Drain into an array. |

`filter` and `take` take a **stream**, not an array. To filter an array, make
a stream first, or use `take_while` on the array family.

A `for` loop consumes a stream directly:

```rak
for line in read_lines("access.log") {
    if line |> regex_is_match(/admin/g) { dump line }
}
```

Note the argument order of the regex builtins: the pattern comes **first**.
`regex_is_match(re, haystack)`.

## CSV and JSON lines

```rak
for row in stream_csv("hosts.csv") {
    dump row.host
    dump row.port
}

for event in stream_jsonl("events.jsonl") {
    dump event["kind"]
}
```

`stream_csv` takes a path and an options string. A `;` in the options selects
a semicolon delimiter; `header` in the options consumes the first row as the
header, so the rows come back as maps. Without it, the rows are arrays.

`stream_jsonl` reads one JSON value per line.

Both are lazy: the file is opened and read as the loop asks for values.

## JSON

```rak
let j = json_parse("{\"user\": \"admin\", \"id\": 7}")
dump j.user                         // admin
dump j.id                           // 7
dump json_stringify(j)              // {"user":"admin","id":7}
```

`json_parse` returns native arrays, maps, ints, floats, bools and nil — there
is no wrapper object to unwrap. `json_stringify` pretty-prints.

| Builtin | Does |
|---|---|
| `json_get(text, key)` | One value, by key or index. |
| `json_path(text, path)` | One value, by dotted path: `data.users.0.name`. |
| `json_keys(text)` | The keys of an object. |
| `json_len(text)` | The length of an array or object, or the byte length of a string. |
| `json_find_all(text, needle)` | Every match, recursively. |

## CSV and YAML

```rak
let rows = csv_parse("host,port\nwin.example,443\n10.0.0.5,0x1F\n")
dump rows[1].host                  // win.example

let docs = yaml_parse("name: rak\nport: 443\n")
dump docs.name
```

`csv_parse` returns a map per row, with the header as the keys. The options
`{header: false}` keep every row as an array instead. Malformed CSV raises,
with the line number.

`yaml_parse` handles a subset: block maps and lists by indentation, scalars,
and `#` comments. Flow style, anchors and aliases are not supported, and the
error names the line.

## Compression and archives

```rak
let packed = gzip("some bytes")
dump gunzip(packed)

let archive = zip_archive([("a.txt", "hello"), ("b.txt", "world")])
dump zip_extract(archive, "a.txt")
```

| Builtin | Does |
|---|---|
| `gzip(b, level?)` / `gunzip(b)` | gzip. |
| `deflate(b, level?)` / `inflate(b)` | Raw deflate, with no container. |
| `zip_archive(files)` | Build a zip in memory from `[name, bytes]` pairs. |
| `zip_extract(zip, name)` | Read one entry. |
| `zip_list(path)` | The entries of a zip file: `{name, size, compressed}`. |
| `zip_read(path, name)` | Read one entry of a zip file. |
| `zip_write(path, entries)` | Write a zip file. |

The `zip_*` file functions take a **path** and return `Result` values:

```rak
let listing = zip_list("bundle.zip")
if listing is Ok(entries) {
    for e in entries { dump e.name }
}
```

Decompression is bounded at 100 MiB, checked while reading rather than
after, so a decompression bomb is refused instead of filling the disk. See
[Safety](safety.html#bounded-decompression).

## Higher-order array builtins

The array family has a set of builtins that take a function:

| Builtin | Does |
|---|---|
| `enumerate(arr)` | Index-and-value pairs. |
| `fold(arr, init, f)` | `f(acc, item)`, left to right. |
| `reduce(arr, f)` | `f(acc, item)`, with the first element as the seed. |
| `any(arr, f)` / `all(arr, f)` | `f(item)`. |
| `flat_map(arr, f)` | Map, then flatten one level. |
| `take_while(arr, f)` | The prefix where `f` holds. |
| `skip(arr, n)` | Drop the first n. |
| `zip(a, b)` | Pairs, truncated to the shorter side. |

```rak
let total = fold([1, 2, 3, 4], 0, fn(acc, x) { return acc + x })
dump total                          // 10
```

`reduce` returns `None` for an empty array, where `fold` returns the seed.
