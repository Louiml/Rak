export interface Example { name: string; filename: string; source: string; }

export const EXAMPLES: Example[] = [
  { name: "hello", filename: "hello.rak", source: `dump "Hello, World"
` },
  { name: "quick", filename: "quick.rak", source: `dump "=== Hello ==="
dump md5("password")
dump hex_encode("ABC")
dump base64_encode("hello")
dump fmt("0x{:04X}", 0x0050)
` },
  { name: "osint_scan", filename: "osint_scan.rak", source: `// Rak OSINT Script - Full feature demo
use net.http
use recon.dns
use web.html
use crypto.hash

let target = "127.0.0.1"

// Port scan using scan_ports() function - returns array for for loops
dump "=== Port Scan ==="
for port in scan_ports(target, { range: [0x0016, 0x0050] }) {
    dump fmt("Port 0x{:04X} open", port)
}

// HTML parsing
dump "=== HTML Parsing ==="
let html = "<html><title>Rak Test</title><a href='https://rak.dev'>Rak</a><a href='https://github.com'>GitHub</a></html>"
dump html_title(html)
for link in html_links(html) {
    dump fmt("Link: {}", link)
}

// Hashing and encoding
dump "=== Crypto ==="
dump md5("password")
dump sha256("secret")
dump hex_encode("ABC")
dump base64_encode("hello")

// File operations
dump "=== Files ==="
file_write("output.txt", "Rak was here")
dump file_read("output.txt")
dump file_exists("output.txt")
file_delete("output.txt")

// JSON utilities
dump "=== JSON ==="
let json = "{\"user\": \"admin\", \"role\": \"superuser\"}"
dump json_get(json, "user")
dump json_keys(json)

// Hex arithmetic
let signature = 0xDEADBEEF
let mask = 0xFF00FF00
let result = signature & mask
dump fmt("Masked: 0x{:08X}", result)

// DNS lookup
dump "=== DNS ==="
let ips = dns_lookup("localhost")
for ip in ips {
    dump ip
}` },
  { name: "echo_server", filename: "echo_server.rak", source: `let listener = net_listen("127.0.0.1:18391")
dump "echo server listening on 127.0.0.1:18391"

fn handle_client(stream) {
    let line = tcp_read_line(stream)
    dump f"got: {line}"
    tcp_write(stream, f"echo: {line}\n")
    tcp_close(stream)
}

let i = 0
while i < 3 {
    let conn = net_accept(listener)
    let stream = conn.0
    dump f"connection from {conn.1}"
    let h = spawn(fn() { handle_client(stream) })
    i = i + 1
}

dump "done"
` },
  { name: "echo_client", filename: "echo_client.rak", source: `let client = net_listen("127.0.0.1:0")
let _ = client

let s = net_connect("127.0.0.1:18392")
tcp_write(s, "hello rak server\n")
let reply = tcp_read_line(s)
dump f"reply: {reply}"
tcp_close(s)
` },
  { name: "bench", filename: "bench.rak", source: `fn fib(n) { if n < 2 { return n } return fib(n - 1) + fib(n - 2) }
dump fib(30)
` },
  { name: "sql_client", filename: "sql_client.rak", source: `fn query(sql) {
    let s = net_connect("127.0.0.1:18393")
    tcp_write(s, sql + "\n")
    let resp = tcp_read_line(s)
    tcp_close(s)
    return resp
}

dump query("CREATE TABLE users (id int, name text, age int)")
dump query("INSERT INTO users (id, name, age) VALUES (1, 'Rak', 7)")
dump query("INSERT INTO users (id, name, age) VALUES (2, 'Ada', 9)")
dump query("INSERT INTO users (id, name, age) VALUES (3, 'Lin', 42)")
dump query("SELECT * FROM users")
dump query("SELECT name, age FROM users WHERE age > 8")
dump query("UPDATE users SET age = 10 WHERE name = 'Rak'")
dump query("SELECT * FROM users WHERE age >= 10")
dump query("DELETE FROM users WHERE id = 2")
dump query("SELECT * FROM users")
` },
  { name: "self_host", filename: "self_host.rak", source: `fn r_is_digit(c) {
    return c == "0" || c == "1" || c == "2" || c == "3" || c == "4" || c == "5" || c == "6" || c == "7" || c == "8" || c == "9"
}

fn r_is_alpha(c) {
    let code = ord(c)
    return (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || c == "_"
}

fn r_is_alnum(c) {
    return r_is_alpha(c) || r_is_digit(c)
}

fn r_lex(src) {
    let toks = []
    let i = 0
    let n = len(src)
    while i < n {
        let c = src[i]
        if c == " " || c == "\n" || c == "\t" || c == "\r" {
            i = i + 1
        } else if c == "/" && i + 1 < n && src[i + 1] == "/" {
            while i < n && src[i] != "\n" {
                i = i + 1
            }
        } else if c == "\"" {
            i = i + 1
            let buf = ""
            while i < n && src[i] != "\"" {
                buf = buf + src[i]
                i = i + 1
            }
            i = i + 1
            toks = push(toks, {ty: "str", value: buf})
        } else if r_is_digit(c) {
            let buf = ""
            while i < n && r_is_digit(src[i]) {
                buf = buf + src[i]
                i = i + 1
            }
            toks = push(toks, {ty: "num", value: buf})
        } else if r_is_alpha(c) {
            let buf = ""
            while i < n && (r_is_alnum(src[i]) || src[i] == "_") {
                buf = buf + src[i]
                i = i + 1
            }
            toks = push(toks, {ty: "ident", value: buf})
        } else if c == "+" || c == "-" || c == "*" || c == "/" || c == "%" || c == "=" || c == "<" || c == ">" || c == "!" {
            let two = ""
            if i + 1 < n {
                two = src[i] + src[i + 1]
            }
            if two == "==" || two == "!=" || two == "<=" || two == ">=" {
                toks = push(toks, {ty: "op", value: two})
                i = i + 2
            } else if two == "&&" || two == "||" {
                toks = push(toks, {ty: "op", value: two})
                i = i + 2
            } else {
                toks = push(toks, {ty: "op", value: c})
                i = i + 1
            }
        } else if c == "(" || c == ")" || c == "{" || c == "}" || c == "," || c == ";" {
            toks = push(toks, {ty: "punct", value: c})
            i = i + 1
        } else {
            i = i + 1
        }
    }
    return toks
}

fn r_tok(toks, i) {
    if i < len(toks) {
        return toks[i]
    }
    return {ty: "eof", value: ""}
}

fn r_peek() {
    return r_tok(g_toks, g_pos)
}

fn r_next() {
    let t = r_peek()
    g_pos = g_pos + 1
    return t
}

fn r_is_punct(v) {
    let t = r_peek()
    return t.ty == "punct" && t.value == v
}

fn r_is_op(v) {
    let t = r_peek()
    return t.ty == "op" && t.value == v
}

fn r_is_kw(v) {
    let t = r_peek()
    return t.ty == "ident" && t.value == v
}

fn r_expect_punct(v) {
    if r_is_punct(v) {
        g_pos = g_pos + 1
        return true
    }
    return false
}

fn r_parse_primary() {
    let t = r_peek()
    if t.ty == "num" {
        r_next()
        return {kind: "num", value: t.value}
    }
    if t.ty == "str" {
        r_next()
        return {kind: "str", value: t.value}
    }
    if t.ty == "ident" {
        r_next()
        let e = {kind: "ident", name: t.value}
        let cont = true
        while cont {
            if r_is_punct("(") {
                r_next()
                let args = []
                while !r_is_punct(")") && r_peek().ty != "eof" {
                    args = push(args, r_parse_expr())
                    if r_is_punct(",") {
                        r_next()
                    }
                }
                r_expect_punct(")")
                e = {kind: "call", callee: e, args: args}
            } else {
                cont = false
            }
        }
        return e
    }
    if r_is_punct("(") {
        r_next()
        let e = r_parse_expr()
        r_expect_punct(")")
        return e
    }
    return {kind: "nil"}
}

fn r_parse_unary() {
    if r_is_op("-") {
        r_next()
        return {kind: "unary", op: "-", e: r_parse_unary()}
    }
    if r_is_op("!") {
        r_next()
        return {kind: "unary", op: "!", e: r_parse_unary()}
    }
    return r_parse_primary()
}

fn r_parse_mul() {
    let left = r_parse_unary()
    while r_is_op("*") || r_is_op("/") || r_is_op("%") {
        let op = r_next().value
        let right = r_parse_unary()
        left = {kind: "bin", op: op, l: left, r: right}
    }
    return left
}

fn r_parse_add() {
    let left = r_parse_mul()
    while r_is_op("+") || r_is_op("-") {
        let op = r_next().value
        let right = r_parse_mul()
        left = {kind: "bin", op: op, l: left, r: right}
    }
    return left
}

fn r_parse_cmp() {
    let left = r_parse_add()
    while r_is_op("<") || r_is_op(">") || r_is_op("<=") || r_is_op(">=") {
        let op = r_next().value
        let right = r_parse_add()
        left = {kind: "bin", op: op, l: left, r: right}
    }
    return left
}

fn r_parse_eq() {
    let left = r_parse_cmp()
    while r_is_op("==") || r_is_op("!=") {
        let op = r_next().value
        let right = r_parse_cmp()
        left = {kind: "bin", op: op, l: left, r: right}
    }
    return left
}

fn r_parse_and() {
    let left = r_parse_eq()
    while r_is_op("&&") || r_is_op("||") {
        let op = r_next().value
        let right = r_parse_eq()
        left = {kind: "bin", op: op, l: left, r: right}
    }
    return left
}

fn r_parse_expr() {
    return r_parse_and()
}

fn r_parse_block() {
    r_expect_punct("{")
    let stmts = []
    while !r_is_punct("}") && r_peek().ty != "eof" {
        stmts = push(stmts, r_parse_stmt())
    }
    r_expect_punct("}")
    return stmts
}

fn r_parse_stmt() {
    if r_is_kw("let") {
        r_next()
        let name = r_next().value
        if r_is_op("=") {
            r_next()
        }
        let v = r_parse_expr()
        if r_is_punct(";") {
            r_next()
        }
        return {kind: "let", name: name, value: v}
    }
    if r_is_kw("fn") {
        r_next()
        let name = r_next().value
        r_expect_punct("(")
        let params = []
        while !r_is_punct(")") {
            params = push(params, r_next().value)
            if r_is_punct(",") {
                r_next()
            }
        }
        r_expect_punct(")")
        let body = r_parse_block()
        return {kind: "fn", name: name, params: params, body: body}
    }
    if r_is_kw("if") {
        r_next()
        let cond = r_parse_expr()
        let then_b = r_parse_block()
        let else_b = []
        if r_is_kw("else") {
            r_next()
            else_b = r_parse_block()
        }
        return {kind: "if", cond: cond, then: then_b, else_branch: else_b}
    }
    if r_is_kw("while") {
        r_next()
        let cond = r_parse_expr()
        let body = r_parse_block()
        return {kind: "while", cond: cond, body: body}
    }
    if r_is_kw("return") {
        r_next()
        let v = {kind: "nil"}
        if !r_is_punct("}") && r_peek().ty != "eof" {
            v = r_parse_expr()
        }
        if r_is_punct(";") {
            r_next()
        }
        return {kind: "return", value: v}
    }
    if r_is_kw("dump") {
        r_next()
        let v = r_parse_expr()
        if r_is_punct(";") {
            r_next()
        }
        return {kind: "dump", value: v}
    }
    let e = r_parse_expr()
    if r_is_punct(";") {
        r_next()
    }
    return {kind: "expr", value: e}
}

fn r_parse(src) {
    g_toks = r_lex(src)
    g_pos = 0
    let prog = []
    while r_peek().ty != "eof" {
        prog = push(prog, r_parse_stmt())
    }
    return prog
}

fn r_env_push() {
    g_scopes = push(g_scopes, {})
}

fn r_env_pop() {
    let n = len(g_scopes)
    g_scopes = r_slice(g_scopes, 0, n - 1)
}

fn r_slice(arr, start, end) {
    let out = []
    let i = start
    while i < end {
        out = push(out, arr[i])
        i = i + 1
    }
    return out
}

fn r_define(name, v) {
    let n = len(g_scopes)
    g_scopes[n - 1][name] = v
}

fn r_lookup(name) {
    let i = len(g_scopes) - 1
    while i >= 0 {
        if has(g_scopes[i], name) {
            return g_scopes[i][name]
        }
        i = i - 1
    }
    return nil
}

fn r_truthy(v) {
    if v == nil {
        return false
    }
    if v.kind == "num" {
        return int(v.value) != 0
    }
    if v.kind == "str" {
        return v.value != ""
    }
    return true
}

fn r_to_str(v) {
    if v == nil {
        return "nil"
    }
    if v.kind == "num" {
        return v.value
    }
    if v.kind == "str" {
        return v.value
    }
    if v.kind == "fn" {
        return "<fn>"
    }
    return "?"
}

fn r_num(v) {
    if v == nil {
        return 0
    }
    if v.kind == "num" {
        return int(v.value)
    }
    return 0
}

fn r_bool(b) {
    if b {
        return 1
    }
    return 0
}

fn r_num_eq(a, b) {
    if a.kind == "str" && b.kind == "str" {
        return r_bool(a.value == b.value)
    }
    return r_bool(r_num(a) == r_num(b))
}

fn r_eval_bin(op, a, b) {
    if op == "+" {
        if a.kind == "str" || b.kind == "str" {
            return {kind: "str", value: r_to_str(a) + r_to_str(b)}
        }
        return {kind: "num", value: r_num(a) + r_num(b)}
    }
    let an = r_num(a)
    let bn = r_num(b)
    if op == "-" {
        return {kind: "num", value: an - bn}
    }
    if op == "*" {
        return {kind: "num", value: an * bn}
    }
    if op == "/" {
        if bn == 0 {
            return {kind: "num", value: 0}
        }
        return {kind: "num", value: an / bn}
    }
    if op == "%" {
        if bn == 0 {
            return {kind: "num", value: 0}
        }
        return {kind: "num", value: an - (an / bn) * bn}
    }
    if op == "==" {
        return {kind: "num", value: r_num_eq(a, b)}
    }
    if op == "!=" {
        return {kind: "num", value: 1 - r_num_eq(a, b)}
    }
    if op == "<" {
        return {kind: "num", value: r_bool(an < bn)}
    }
    if op == ">" {
        return {kind: "num", value: r_bool(an > bn)}
    }
    if op == "<=" {
        return {kind: "num", value: r_bool(an <= bn)}
    }
    if op == ">=" {
        return {kind: "num", value: r_bool(an >= bn)}
    }
    if op == "&&" {
        return {kind: "num", value: r_bool(r_truthy(a) && r_truthy(b))}
    }
    if op == "||" {
        return {kind: "num", value: r_bool(r_truthy(a) || r_truthy(b))}
    }
    return {kind: "nil"}
}

fn r_eval(e) {
    let k = e.kind
    if k == "num" {
        return e
    }
    if k == "str" {
        return e
    }
    if k == "nil" {
        return e
    }
    if k == "ident" {
        return r_lookup(e.name)
    }
    if k == "bin" {
        let a = r_eval(e.l)
        let b = r_eval(e.r)
        return r_eval_bin(e.op, a, b)
    }
    if k == "unary" {
        let v = r_eval(e.e)
        if e.op == "-" {
            return {kind: "num", value: 0 - r_num(v)}
        }
        if e.op == "!" {
            return {kind: "num", value: r_bool(!r_truthy(v))}
        }
        return v
    }
    if k == "call" {
        return r_eval_call(e)
    }
    return {kind: "nil"}
}

fn r_eval_call(e) {
    let callee = e.callee
    if callee.kind == "ident" {
        let f = r_lookup(callee.name)
        if f != nil && f.kind == "fn" {
            return r_call(f, e.args)
        }
    }
    return {kind: "nil"}
}

fn r_call(f, arg_exprs) {
    let args = []
    for a in arg_exprs {
        args = push(args, r_eval(a))
    }
    r_env_push()
    let i = 0
    while i < len(f.params) {
        if i < len(args) {
            r_define(f.params[i], args[i])
        } else {
            r_define(f.params[i], {kind: "nil"})
        }
        i = i + 1
    }
    let saved_ret = g_ret
    let saved_ret_flag = g_returning
    g_returning = false
    for s in f.body {
        r_exec(s)
        if g_returning {
            break
        }
    }
    let result = g_ret
    if !g_returning {
        result = {kind: "nil"}
    }
    g_returning = saved_ret_flag
    g_ret = saved_ret
    r_env_pop()
    return result
}

fn r_exec(s) {
    let k = s.kind
    if k == "let" {
        r_define(s.name, r_eval(s.value))
        return
    }
    if k == "fn" {
        r_define(s.name, s)
        return
    }
    if k == "expr" {
        r_eval(s.value)
        return
    }
    if k == "dump" {
        print(r_to_str(r_eval(s.value)))
        return
    }
    if k == "return" {
        g_ret = r_eval(s.value)
        g_returning = true
        return
    }
    if k == "if" {
        if r_truthy(r_eval(s.cond)) {
            r_env_push()
            for st in s.then {
                r_exec(st)
                if g_returning {
                    break
                }
            }
            r_env_pop()
        } else {
            r_env_push()
            for st in s.else_branch {
                r_exec(st)
                if g_returning {
                    break
                }
            }
            r_env_pop()
        }
        return
    }
    if k == "while" {
        while r_truthy(r_eval(s.cond)) {
            r_env_push()
            for st in s.body {
                r_exec(st)
                if g_returning {
                    break
                }
            }
            r_env_pop()
            if g_returning {
                break
            }
        }
        return
    }
}

fn r_run(src) {
    let prog = r_parse(src)
    r_env_push()
    for s in prog {
        r_exec(s)
    }
    r_env_pop()
}

let g_pos = 0
let g_toks = []
let g_scopes = []
let g_ret = nil
let g_returning = false

print("=== Rak self-hosting demo ===")
print("A Rak interpreter (lexer + parser + tree-walker) written in Rak.")
print("It reads Rak source and executes it.")
print("")
let demo = "dump 1 + 2 * 3"
print("Source: " + demo)
print("Output:")
r_run(demo)
print("")
let demo2 = "let x = 5; let y = 7; dump x * y - 1"
print("Source: " + demo2)
print("Output:")
r_run(demo2)
` },
  { name: "sql_server", filename: "sql_server.rak", source: `fn is_digit(c) {
    return c == "0" || c == "1" || c == "2" || c == "3" || c == "4" || c == "5" || c == "6" || c == "7" || c == "8" || c == "9"
}

fn is_alpha(c) {
    let code = ord(c)
    return (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || c == "_"
}

fn is_alnum(c) {
    return is_alpha(c) || is_digit(c)
}

fn sql_lex(src) {
    let toks = []
    let i = 0
    let n = len(src)
    let keywords = {select: 1, from: 1, where: 1, insert: 1, into: 1, values: 1, create: 1, table: 1, delete: 1, and: 1, or: 1, not: 1, null: 1, int: 1, text: 1, bool: 1, update: 1, set: 1, drop: 1}
    while i < n {
        let c = src[i]
        if c == " " || c == "\n" || c == "\t" || c == "\r" {
            i = i + 1
        } else if c == "-" && i + 1 < n && src[i + 1] == "-" {
            while i < n && src[i] != "\n" {
                i = i + 1
            }
        } else if c == "'" {
            i = i + 1
            let buf = ""
            while i < n && src[i] != "'" {
                buf = buf + src[i]
                i = i + 1
            }
            i = i + 1
            toks = push(toks, {ty: "string", value: buf})
        } else if c == "\"" {
            i = i + 1
            let buf = ""
            while i < n && src[i] != "\"" {
                buf = buf + src[i]
                i = i + 1
            }
            i = i + 1
            toks = push(toks, {ty: "ident", value: buf})
        } else if is_digit(c) {
            let buf = ""
            while i < n && is_digit(src[i]) {
                buf = buf + src[i]
                i = i + 1
            }
            toks = push(toks, {ty: "number", value: buf})
        } else if is_alpha(c) {
            let buf = ""
            while i < n && (is_alnum(src[i]) || src[i] == "_") {
                buf = buf + src[i]
                i = i + 1
            }
            let low = lower(buf)
            let iskw = false
            for k in keys(keywords) {
                if k == low {
                    iskw = true
                }
            }
            if iskw {
                toks = push(toks, {ty: "kw", value: low})
            } else {
                toks = push(toks, {ty: "ident", value: buf})
            }
        } else if c == "(" || c == ")" || c == "," || c == ";" || c == "*" || c == "." {
            toks = push(toks, {ty: "punct", value: c})
            i = i + 1
        } else if c == "=" {
            toks = push(toks, {ty: "op", value: "="})
            i = i + 1
        } else if c == "<" {
            if i + 1 < n && src[i + 1] == "=" {
                toks = push(toks, {ty: "op", value: "<="})
                i = i + 2
            } else if i + 1 < n && src[i + 1] == ">" {
                toks = push(toks, {ty: "op", value: "<>"})
                i = i + 2
            } else {
                toks = push(toks, {ty: "op", value: "<"})
                i = i + 1
            }
        } else if c == ">" {
            if i + 1 < n && src[i + 1] == "=" {
                toks = push(toks, {ty: "op", value: ">="})
                i = i + 2
            } else {
                toks = push(toks, {ty: "op", value: ">"})
                i = i + 1
            }
        } else if c == "!" && i + 1 < n && src[i + 1] == "=" {
            toks = push(toks, {ty: "op", value: "!="})
            i = i + 2
        } else {
            i = i + 1
        }
    }
    return toks
}

fn tok_at(toks, i) {
    if i < len(toks) {
        return toks[i]
    }
    return {ty: "eof", value: ""}
}

fn is_kw(toks, i, val) {
    let t = tok_at(toks, i)
    return t.ty == "kw" && t.value == val
}

fn is_punct(toks, i, val) {
    let t = tok_at(toks, i)
    return t.ty == "punct" && t.value == val
}

fn is_op(toks, i, val) {
    let t = tok_at(toks, i)
    return t.ty == "op" && t.value == val
}

fn lit_value(t) {
    if t.ty == "number" {
        return {kind: "num", value: t.value}
    }
    if t.ty == "string" {
        return {kind: "str", value: t.value}
    }
    if t.value == "null" || t.value == "NULL" {
        return {kind: "null", value: ""}
    }
    return {kind: "str", value: t.value}
}

fn parse_cond(toks, i) {
    let t = tok_at(toks, i)
    let col = ""
    if t.ty == "ident" {
        col = t.value
        i = i + 1
    }
    let op_t = tok_at(toks, i)
    let op = op_t.value
    if op_t.ty != "op" {
        return {expr: {op: "true"}, pos: i}
    }
    i = i + 1
    let val_t = tok_at(toks, i)
    let val = lit_value(val_t)
    i = i + 1
    return {expr: {op: op, col: col, value: val}, pos: i}
}

fn parse_and(toks, i) {
    let left_res = parse_cond(toks, i)
    let left = left_res.expr
    i = left_res.pos
    while is_kw(toks, i, "and") {
        i = i + 1
        let right_res = parse_cond(toks, i)
        let right = right_res.expr
        i = right_res.pos
        left = {op: "and", left: left, right: right}
    }
    return {expr: left, pos: i}
}

fn parse_or(toks, i) {
    let left_res = parse_and(toks, i)
    let left = left_res.expr
    i = left_res.pos
    while is_kw(toks, i, "or") {
        i = i + 1
        let right_res = parse_and(toks, i)
        let right = right_res.expr
        i = right_res.pos
        left = {op: "or", left: left, right: right}
    }
    return {expr: left, pos: i}
}

fn parse_where(toks, i) {
    return parse_or(toks, i)
}

fn parse_select(toks) {
    let i = 1
    let cols = []
    if is_punct(toks, i, "*") {
        cols = push(cols, "*")
        i = i + 1
    } else {
        let cont = true
        while cont {
            let t = tok_at(toks, i)
            if t.ty == "ident" {
                cols = push(cols, t.value)
                i = i + 1
                if is_punct(toks, i, ",") {
                    i = i + 1
                } else {
                    cont = false
                }
            } else {
                cont = false
            }
        }
    }
    let from = ""
    if is_kw(toks, i, "from") {
        i = i + 1
        let t = tok_at(toks, i)
        if t.ty == "ident" {
            from = t.value
            i = i + 1
        }
    }
    let where = nil
    if is_kw(toks, i, "where") {
        i = i + 1
        let res = parse_where(toks, i)
        where = res.expr
        i = res.pos
    }
    return {ty: "select", columns: cols, from: from, where: where}
}

fn parse_insert(toks) {
    let i = 1
    if !is_kw(toks, i, "into") {
        return {ty: "error", message: "expected INTO"}
    }
    i = i + 1
    let table = tok_at(toks, i).value
    i = i + 1
    let cols = []
    if is_punct(toks, i, "(") {
        i = i + 1
        let cont = true
        while cont {
            let t = tok_at(toks, i)
            if t.ty == "ident" {
                cols = push(cols, t.value)
                i = i + 1
                if is_punct(toks, i, ",") {
                    i = i + 1
                } else {
                    cont = false
                }
            } else {
                cont = false
            }
        }
        if is_punct(toks, i, ")") {
            i = i + 1
        }
    }
    if !is_kw(toks, i, "values") {
        return {ty: "error", message: "expected VALUES"}
    }
    i = i + 1
    let rows = []
    let cont = true
    while cont {
        if !is_punct(toks, i, "(") {
            cont = false
        } else {
            i = i + 1
            let row = []
            let inner = true
            while inner {
                let t = tok_at(toks, i)
                if t.ty == "number" || t.ty == "string" || t.ty == "ident" {
                    row = push(row, lit_value(t))
                    i = i + 1
                } else {
                    inner = false
                }
                if is_punct(toks, i, ",") {
                    i = i + 1
                } else {
                    inner = false
                }
            }
            if is_punct(toks, i, ")") {
                i = i + 1
            }
            rows = push(rows, row)
            if is_punct(toks, i, ",") {
                i = i + 1
            } else {
                cont = false
            }
        }
    }
    return {ty: "insert", table: table, columns: cols, rows: rows}
}

fn parse_create(toks) {
    let i = 1
    if !is_kw(toks, i, "table") {
        return {ty: "error", message: "expected TABLE"}
    }
    i = i + 1
    let table = tok_at(toks, i).value
    i = i + 1
    let cols = []
    if is_punct(toks, i, "(") {
        i = i + 1
        let cont = true
        while cont {
            let t = tok_at(toks, i)
            if t.ty == "ident" {
                let name = t.value
                i = i + 1
                let ty_t = tok_at(toks, i)
                let type_name = ""
                if ty_t.ty == "kw" || ty_t.ty == "ident" {
                    type_name = ty_t.value
                    i = i + 1
                }
                cols = push(cols, {name: name, ty: type_name})
                if is_punct(toks, i, ",") {
                    i = i + 1
                } else {
                    cont = false
                }
            } else {
                cont = false
            }
        }
        if is_punct(toks, i, ")") {
            i = i + 1
        }
    }
    return {ty: "create", table: table, columns: cols}
}

fn parse_delete(toks) {
    let i = 1
    if !is_kw(toks, i, "from") {
        return {ty: "error", message: "expected FROM"}
    }
    i = i + 1
    let from = tok_at(toks, i).value
    i = i + 1
    let where = nil
    if is_kw(toks, i, "where") {
        i = i + 1
        let res = parse_where(toks, i)
        where = res.expr
        i = res.pos
    }
    return {ty: "delete", from: from, where: where}
}

fn parse_update(toks) {
    let i = 1
    let table = tok_at(toks, i).value
    i = i + 1
    if !is_kw(toks, i, "set") {
        return {ty: "error", message: "expected SET"}
    }
    i = i + 1
    let sets = []
    let cont = true
    while cont {
        let col = tok_at(toks, i).value
        i = i + 1
        if !is_op(toks, i, "=") {
            cont = false
        } else {
            i = i + 1
            let t = tok_at(toks, i)
            sets = push(sets, {col: col, value: lit_value(t)})
            i = i + 1
            if is_punct(toks, i, ",") {
                i = i + 1
            } else {
                cont = false
            }
        }
    }
    let where = nil
    if is_kw(toks, i, "where") {
        i = i + 1
        let res = parse_where(toks, i)
        where = res.expr
        i = res.pos
    }
    return {ty: "update", table: table, sets: sets, where: where}
}

fn parse_drop(toks) {
    let i = 1
    if !is_kw(toks, i, "table") {
        return {ty: "error", message: "expected TABLE"}
    }
    i = i + 1
    let table = tok_at(toks, i).value
    return {ty: "drop", table: table}
}

fn sql_parse(toks) {
    if len(toks) == 0 {
        return {ty: "error", message: "empty query"}
    }
    let t = toks[0]
    if t.ty != "kw" {
        return {ty: "error", message: "expected keyword"}
    }
    if t.value == "select" {
        return parse_select(toks)
    }
    if t.value == "insert" {
        return parse_insert(toks)
    }
    if t.value == "create" {
        return parse_create(toks)
    }
    if t.value == "delete" {
        return parse_delete(toks)
    }
    if t.value == "update" {
        return parse_update(toks)
    }
    if t.value == "drop" {
        return parse_drop(toks)
    }
    return {ty: "error", message: f"unknown statement: {t.value}"}
}

fn lit_to_str(v) {
    if v == nil {
        return "NULL"
    }
    if v.kind == "null" {
        return "NULL"
    }
    return v.value
}

fn cmp_kind(v) {
    if v == nil {
        return "null"
    }
    return v.kind
}

fn eval_where(expr, row) {
    let op = expr.op
    if op == "and" {
        return eval_where(expr.left, row) && eval_where(expr.right, row)
    }
    if op == "or" {
        return eval_where(expr.left, row) || eval_where(expr.right, row)
    }
    if op == "true" {
        return true
    }
    let col_val = row[expr.col]
    let cmp_val = expr.value
    if col_val == nil {
        return false
    }
    let a = lit_to_str(col_val)
    let b = lit_to_str(cmp_val)
    if cmp_kind(col_val) == "num" && cmp_kind(cmp_val) == "num" {
        let an = int(a)
        let bn = int(b)
        if op == "=" {
            return an == bn
        }
        if op == "!=" || op == "<>" {
            return an != bn
        }
        if op == "<" {
            return an < bn
        }
        if op == ">" {
            return an > bn
        }
        if op == "<=" {
            return an <= bn
        }
        if op == ">=" {
            return an >= bn
        }
    }
    if op == "=" {
        return a == b
    }
    if op == "!=" || op == "<>" {
        return a != b
    }
    if op == "<" {
        return a < b
    }
    if op == ">" {
        return a > b
    }
    if op == "<=" {
        return a <= b
    }
    if op == ">=" {
        return a >= b
    }
    return false
}

fn esc(v) {
    let kind = v.kind
    if kind == "null" {
        return "\\n"
    }
    let s = v.value
    let out = ""
    for ch in s {
        if ch == " " {
            out = out + "\\s"
        } else if ch == "\\" {
            out = out + "\\\\"
        } else if ch == "\n" {
            out = out + "\\l"
        } else {
            out = out + ch
        }
    }
    if kind == "num" {
        return "N" + out
    }
    return "S" + out
}

fn unesc(p) {
    if p == "\\n" {
        return {kind: "null", value: ""}
    }
    let kind = substr(p, 0, 1)
    let body = substr(p, 1, len(p) - 1)
    let out = ""
    let i = 0
    while i < len(body) {
        let c = body[i]
        if c == "\\" && i + 1 < len(body) {
            let nx = body[i + 1]
            if nx == "s" {
                out = out + " "
            } else if nx == "\\" {
                out = out + "\\"
            } else if nx == "l" {
                out = out + "\n"
            } else {
                out = out + nx
            }
            i = i + 2
        } else {
            out = out + c
            i = i + 1
        }
    }
    if kind == "N" {
        return {kind: "num", value: out}
    }
    return {kind: "str", value: out}
}

fn db_serialize(db) {
    let out = ""
    let tnames = keys(db.tables)
    for name in tnames {
        let table = db.tables[name]
        if table != nil {
            out = out + "T " + name + "\n"
            out = out + "C"
            for c in table.columns {
                out = out + " " + c.name + ":" + c.ty
            }
            out = out + "\n"
            for row in table.rows {
                out = out + "R"
                for v in row {
                    out = out + " " + esc(v)
                }
                out = out + "\n"
            }
        }
    }
    return out
}

fn db_deserialize(s) {
    let db = {tables: {}, file: ""}
    let lines = split(s, "\n")
    let cur_name = ""
    let cur_table = nil
    for line in lines {
        if len(line) >= 2 && substr(line, 0, 2) == "T " {
            cur_name = substr(line, 2, len(line) - 2)
            cur_table = {columns: [], rows: []}
        } else if len(line) >= 1 && substr(line, 0, 1) == "C" {
            let parts = split(substr(line, 2, len(line) - 2), " ")
            let cols = []
            for p in parts {
                if p != "" {
                    let kv = split(p, ":")
                    cols = push(cols, {name: kv[0], ty: kv[1]})
                }
            }
            cur_table.columns = cols
        } else if len(line) >= 1 && substr(line, 0, 1) == "R" {
            let parts = split(substr(line, 2, len(line) - 2), " ")
            let row = []
            for p in parts {
                if p != "" {
                    row = push(row, unesc(p))
                }
            }
            cur_table.rows = push(cur_table.rows, row)
            db.tables[cur_name] = cur_table
        }
    }
    return db
}

fn db_save() {
    if g_db.file != "" {
        file_write(g_db.file, db_serialize(g_db))
    }
}

fn db_load(path) {
    if file_exists(path) {
        let s = file_read(path)
        let db = db_deserialize(s)
        db.file = path
        return db
    }
    let d = {tables: {}, file: path}
    return d
}

fn sql_exec(db, q) {
    if q.ty == "error" {
        return {ok: false, message: q.message, rows: []}
    }
    if q.ty == "create" {
        g_db.tables[q.table] = {columns: q.columns, rows: []}
        db_save()
        return {ok: true, message: f"created table {q.table}", rows: []}
    }
    if q.ty == "insert" {
        let table = g_db.tables[q.table]
        if table == nil {
            return {ok: false, message: f"no such table {q.table}", rows: []}
        }
        for row in q.rows {
            let full = []
            for c in table.columns {
                let found = false
                let idx = 0
                while idx < len(q.columns) {
                    if q.columns[idx] == c.name {
                        found = true
                    }
                    idx = idx + 1
                }
                if found {
                    let j = 0
                    while j < len(q.columns) {
                        if q.columns[j] == c.name {
                            full = push(full, row[j])
                        }
                        j = j + 1
                    }
                } else {
                    full = push(full, {kind: "null", value: ""})
                }
            }
            table.rows = push(table.rows, full)
        }
        g_db.tables[q.table] = table
        db_save()
        return {ok: true, message: "inserted", rows: []}
    }
    if q.ty == "select" {
        let table = g_db.tables[q.from]
        if table == nil {
            return {ok: false, message: f"no such table {q.from}", rows: []}
        }
        let out_rows = []
        let cols = []
        if len(q.columns) == 1 && q.columns[0] == "*" {
            for c in table.columns {
                cols = push(cols, c.name)
            }
        } else {
            cols = q.columns
        }
        out_rows = push(out_rows, cols)
        for row in table.rows {
            let row_map = {}
            let idx = 0
            while idx < len(table.columns) {
                row_map[table.columns[idx].name] = row[idx]
                idx = idx + 1
            }
            let ok = true
            if q.where != nil {
                ok = eval_where(q.where, row_map)
            }
            if ok {
                let out = []
                for c in cols {
                    out = push(out, lit_to_str(row_map[c]))
                }
                out_rows = push(out_rows, out)
            }
        }
        return {ok: true, message: "ok", rows: out_rows}
    }
    if q.ty == "delete" {
        let table = g_db.tables[q.from]
        if table == nil {
            return {ok: false, message: f"no such table {q.from}", rows: []}
        }
        let kept = []
        let removed = 0
        for row in table.rows {
            let row_map = {}
            let idx = 0
            while idx < len(table.columns) {
                row_map[table.columns[idx].name] = row[idx]
                idx = idx + 1
            }
            let del = false
            if q.where != nil {
                del = eval_where(q.where, row_map)
            }
            if del {
                removed = removed + 1
            } else {
                kept = push(kept, row)
            }
        }
        table.rows = kept
        g_db.tables[q.from] = table
        db_save()
        return {ok: true, message: f"deleted {removed} rows", rows: []}
    }
    if q.ty == "update" {
        let table = g_db.tables[q.table]
        if table == nil {
            return {ok: false, message: f"no such table {q.table}", rows: []}
        }
        let updated = 0
        let new_rows = []
        for row in table.rows {
            let row_map = {}
            let idx = 0
            while idx < len(table.columns) {
                row_map[table.columns[idx].name] = row[idx]
                idx = idx + 1
            }
            let matches = true
            if q.where != nil {
                matches = eval_where(q.where, row_map)
            }
            if matches {
                let nr = []
                let j = 0
                while j < len(table.columns) {
                    let setv = row[j]
                    for s in q.sets {
                        if table.columns[j].name == s.col {
                            setv = s.value
                        }
                    }
                    nr = push(nr, setv)
                    j = j + 1
                }
                new_rows = push(new_rows, nr)
                updated = updated + 1
            } else {
                new_rows = push(new_rows, row)
            }
        }
        table.rows = new_rows
        g_db.tables[q.table] = table
        db_save()
        return {ok: true, message: f"updated {updated} rows", rows: []}
    }
    if q.ty == "drop" {
        g_db.tables[q.table] = nil
        db_save()
        return {ok: true, message: f"dropped {q.table}", rows: []}
    }
    return {ok: false, message: "unknown query", rows: []}
}

fn json_escape(s) {
    let out = ""
    for ch in s {
        if ch == "\"" {
            out = out + "\\\""
        } else if ch == "\\" {
            out = out + "\\\\"
        } else if ch == "\n" {
            out = out + "\\n"
        } else {
            out = out + ch
        }
    }
    return out
}

fn json_cell(cell) {
    if cell == "NULL" {
        return "null"
    }
    let isnum = true
    try {
        let _ = int(cell)
    } catch e {
        isnum = false
    }
    if isnum {
        return cell
    }
    return f"\"{json_escape(cell)}\""
}

fn json_result(result) {
    let out = "{\"ok\":"
    if result.ok {
        out = out + "true"
    } else {
        out = out + "false"
    }
    out = out + ",\"message\":\"" + json_escape(result.message) + "\",\"rows\":["
    let first = true
    for row in result.rows {
        if !first {
            out = out + ","
        }
        first = false
        out = out + "["
        let f2 = true
        for cell in row {
            if !f2 {
                out = out + ","
            }
            f2 = false
            out = out + json_cell(cell)
        }
        out = out + "]"
    }
    return out + "]}"
}

fn run_query(sql) {
    let toks = sql_lex(sql)
    let q = sql_parse(toks)
    let r = sql_exec(q)
    return json_result(r)
}

fn handle_client(stream) {
    let line = tcp_read_line(stream)
    if line == "" || line == nil {
        tcp_close(stream)
        return
    }
    if line == "QUIT" {
        tcp_close(stream)
        return
    }
    let resp = ""
    try {
        resp = run_query(line)
    } catch e {
        resp = f"{{\"ok\":false,\"message\":\"{e}\",\"rows\":[]}}"
    }
    tcp_write(stream, resp + "\n")
    tcp_close(stream)
}

let g_db = db_load("rakdb.dat")
let listener = net_listen("127.0.0.1:18393")
print("rak-sql listening on 127.0.0.1:18393 (send SQL lines, JSON back)")

let running = true
while running {
    let conn = net_accept(listener)
    let stream = conn.0
    handle_client(stream)
}
` },
  { name: "pipeline", filename: "pipeline.rak", source: `// pipeline.rak -- the |> pipeline operator (interpreter + VM).
// x |> f        desugars to  f(x)
// x |> f(a, b)  desugars to  f(x, a, b)

fn inc(n) { return n + 1 }
fn dbl(n) { return n * 2 }
fn add(a, b) { return a + b }

dump 5 |> inc |> dbl          // 12
dump 3 |> add(10)             // 13
dump [1, 2, 3, 4] |> sum      // 10
` },
  { name: "regex", filename: "regex.rak", source: `// regex.rak -- regex literals and matching.
// Flags: i (case-insensitive), m (multi-line), s (dotall), x (extended), g (no-op).

let re = /\\d+/g
dump re.is_match("abc123")            // true
dump re.find_all("a1 b22 c333")       // [1, 22, 333]
dump re.find("no digits here")        // nil

let ws = /\\s+/g
dump ws.replace("a  b   c", "_")      // a_b_c

// Free-function form (also works on the VM):
dump regex_find_all(/[a-z]+/g, "a1bc2def")   // [a, bc, def]
` },
  { name: "binary_patterns", filename: "binary_patterns.rak", source: `// binary_patterns.rak -- binary pattern matching over byte slices.

fn sniff(data: bytes) {
    match data {
        [0x89, 'P', 'N', 'G', ..] => { return "png" },
        [0xFF, 0xD8, 0xFF, ..] => { return "jpeg" },
        ['%', 'P', 'D', 'F', ..] => { return "pdf" },
        _ => { return "unknown" },
    }
}

dump sniff(b"\\x89PNG\\x0d\\x0a\\x1a\\x0a")  // png
dump sniff(b"\\xFF\\xD8\\xFF\\xE0")          // jpeg
dump sniff(b"%PDF-1.4")                      // pdf
` },
  { name: "traits", filename: "traits.rak", source: `// traits.rak -- Display, Iterable, Index, IndexMut.
// The receiver is passed as the first argument of each method.

struct Point { x: int, y: int }

impl Display for Point {
    fn fmt(self) { return fmt("({}, {})", self.x, self.y) }
}
dump Point { x: 3, y: 4 }   // (3, 4)

struct Range { lo: int, hi: int }
impl Iterable for Range {
    fn iter(self) {
        let out = []
        let i = self.lo
        while i <= self.hi { out = push(out, i); i = i + 1 }
        return out
    }
}

let s = 0
for n in Range { lo: 1, hi: 5 } { s = s + n }
dump s   // 15

struct Table { entries: map }
impl Index for Table {
    fn index(self, key) { return get(self.entries, key, 0) }
}
impl IndexMut for Table {
    fn set(self, key, value) { self.entries[key] = value; return self }
}

let t = Table { entries: {a: 1, b: 2} }
dump t["a"]   // 1
t["c"] = 99
dump t["c"]   // 99
` },
  {
    name: 'ffi',
    filename: 'ffi.rak',
    source: `// FFI: call native C functions and manage raw memory.
extern "C" {
    fn abs(n: i32) -> i32
}
dump abs(-42)            // 42
let buf = ffi_alloc(4)
ffi_write(buf, 0, 0x41)
dump ffi_cstr_to_string(buf)  // "A"
ffi_free(buf)
let p = ffi_ptr(0xDEADBEEF)
dump fmt("ptr = 0x{:08X}", p)
`,
  },
  {
    name: 'mmap',
    filename: 'mmap.rak',
    source: `// Memory-mapped files: inspect large files without loading them.
let m = mmap_open("examples/mmap_sample.bin", "r")
dump fmt("size = {} bytes", mmap_size(m))
let hdr = mmap_slice(m, 0, 8)
dump fmt("byte 0 = 0x{:02X}", hdr[0])
dump mmap_find(m, "GET")
let lines = mmap_lines_off(m, "\\n")
dump fmt("line count = {}", len(lines))
`,
  },
  {
    name: 'async',
    filename: 'async.rak',
    source: `// Async event loop: async fn, await, and futures.
async fn probe(host, port) {
    let open = await tcp_probe(host, port, 200)
    return open
}
dump await probe("127.0.0.1", 80)
dump await probe("127.0.0.1", 9999)
let f = tcp_probe("127.0.0.1", 22, 200)
dump fmt("port 22 open = {}", await f)
`,
  },
  {
    name: 'net_raw',
    filename: 'net_raw.rak',
    source: `// Raw sockets: forge IPv4/TCP/UDP packets with checksums.
let pkt = net_raw_tcp_syn("10.0.0.5", "10.0.0.10", 12345, 80)
dump fmt("syn packet = {} bytes", len(pkt))
dump fmt("byte 0 = 0x{:02X} (IPv4)", pkt[0])
dump fmt("byte 9 = 0x{:02X} (proto TCP)", pkt[9])
dump fmt("byte 33 = 0x{:02X} (SYN flag)", pkt[33])
dump net_raw_send(pkt)   // Ok(...) on unix w/ CAP_NET_RAW, Err(...) otherwise
`,
  },
  {
    name: 'parsers',
    filename: 'parsers.rak',
    source: `// DNS / TLS / PCAP wire-format parsers.
let q = dns_build("example.com", "A")
dump fmt("dns query = {} bytes", len(q))
dump dns_query("example.com", "A")   // Ok({answers: [...], truncated}) or Err offline
dump pcap_open("capture.pcap")       // Ok(<pcap>) or Err (needs --features pcap)
`,
  },
  {
    name: 'macros',
    filename: 'macros.rak',
    source: `// Compile-time macros: AST-expanding templates with $param placeholders.
macro add1(x: expr) { $x + 1 }
dump add1!(41)          // 42
macro swap(a: expr, b: expr) {
    let t = $a
    t + $b
}
dump swap!(10, 20)      // 30
macro pair(a: expr, b: expr) { [$a, $b] }
dump pair!(1, 2)        // [1, 2]
const MAX_LEN = 256
dump MAX_LEN
`,
  },
  {
    name: 'import_demo',
    filename: 'import_demo.rak',
    source: `// Python-style imports & exports. NOTE: this script uses 'import mymod'
// which resolves a sibling mymod/ package — save it next to a mymod/ folder
// (with init.rak + sub.rak) to run. It still highlights correctly here.
import mymod
dump mymod.PI
dump mymod.add(2, 3)
import mymod as m
dump m.add(1, 1)
from mymod import add
dump add(10, 20)
from mymod import add as plus, mul as times
dump plus(7, 8)
dump times(6, 7)
let PI = "local-pi"
from mymod import *
dump PI               // "local-pi" (local wins)
dump mul(3, 3)        // 9 (imported)
from mymod.sub import sub_add, SUB_NAME, add
dump SUB_NAME
dump sub_add(100, 1)
dump add(5, 6)
`,
  },
];
