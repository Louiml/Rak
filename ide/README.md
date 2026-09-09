# Rak IDE

A Tauri 2 + Next.js + React desktop IDE for the [Rak](https://github.com/Louiml/Rak) language. The editor is a bespoke `<textarea>` over a syntax-highlighted `<pre>` overlay with a hand-rolled tokenizer (no CodeMirror/Monaco). Runs scripts via the bundled `rakc` (interp/vm/bench) as a CLI.

## Editor syntax highlighting

The editor tokenizer (`src/app/components/CodeEditor.tsx`) highlights:

- **Keywords** — `let`, `fn`, `struct`, `enum`, `impl`, `trait`, `mod`, `pub`, `use`, `import`, `from`, `export`, `macro`, `const`, `extern`, `async`, `await`, `spawn`, `match`, `if`/`else`/`for`/`while`/`loop`, `try`/`catch`/`raise`, `scan`/`fetch`/`dump`/`trace`, `type`, `as`.
- **Literals** — regex literals `/…/flags`, byte strings `b"…"`, interpolation strings `f"…{expr}…"`, char literals `'…'`, hex `0x…`, typed ints `42i32`.
- **Macros** — `$placeholder` variables and `name!(…)` macro invocations.
- **Operators** — `|>` (pipeline), `..`, `::`, `->`, `=>`, `!`, `$`, and the usual arithmetic/logic ops.
- **Builtins** — `ffi_*`, `mmap_*`, `net_raw_*`, `dns_*`, `tls_*`, `pcap_*`, `http_get_async`, `tcp_probe`, the crypto/file/html/json/net/regex builtins, and `Some`/`None`/`Ok`/`Err`.

Autocomplete offers keywords, types, builtins, and snippets (incl. `import`, `from`, `export`, `macro`, `extern`, `const`, `async`, `mmap`, `netraw`, `ffi`, `dns`). The built-in **Examples** gallery includes `ffi`, `mmap`, `async`, `net_raw`, `parsers`, `macros`, and `import_demo`.

## Getting Started

---

This is a [Next.js](https://nextjs.org) project bootstrapped with [`create-next-app`](https://nextjs.org/docs/app/api-reference/cli/create-next-app).

## Getting Started

First, run the development server:

```bash
npm run dev
# or
yarn dev
# or
pnpm dev
# or
bun dev
```

Open [http://localhost:3000](http://localhost:3000) with your browser to see the result.

You can start editing the page by modifying `app/page.tsx`. The page auto-updates as you edit the file.

This project uses [`next/font`](https://nextjs.org/docs/app/building-your-application/optimizing/fonts) to automatically optimize and load [Geist](https://vercel.com/font), a new font family for Vercel.

## Learn More

To learn more about Next.js, take a look at the following resources:

- [Next.js Documentation](https://nextjs.org/docs) - learn about Next.js features and API.
- [Learn Next.js](https://nextjs.org/learn) - an interactive Next.js tutorial.

You can check out [the Next.js GitHub repository](https://github.com/vercel/next.js) - your feedback and contributions are welcome!

## Deploy on Vercel

The easiest way to deploy your Next.js app is to use the [Vercel Platform](https://vercel.com/new?utm_medium=default-template&filter=next.js&utm_source=create-next-app&utm_campaign=create-next-app-readme) from the creators of Next.js.

Check out our [Next.js deployment documentation](https://nextjs.org/docs/app/building-your-application/deploying) for more details.
