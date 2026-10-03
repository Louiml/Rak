//! Debug Adapter Protocol server (`rakc dap <file>`).
//!
//! Speaks DAP over stdio with `Content-Length` framing so editors (VS Code,
//! the Rak IDE) can drive the existing VM debugger: breakpoints, stepping,
//! stack/locals views.
//!
//! Request model: the DAP reader runs on the main thread; the program executes
//! on a worker thread whose debugger hook parks on a condvar whenever it hits
//! a breakpoint or a requested step. Snapshot requests (`stackTrace`,
//! `scopes`, `variables`) answer from the most recent pause.

use crate::vm::{Vm, VmDebugAction};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{BufRead, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// One paused-program snapshot (locals/globals rendered as strings).
#[derive(Clone)]
struct Paused {
    line: u32,
    locals: Vec<(String, String)>,
    globals: Vec<(String, String)>,
}

/// What the DAP reader asked the program to do next.
#[derive(Clone, Copy, PartialEq)]
enum Cmd {
    Noop,
    Continue,
    Next,
    Quit,
}

/// Shared state between the DAP reader and the VM worker.
struct Shared {
    breakpoints: HashSet<u32>,
    step: bool,
    cmd: Cmd,
    paused: Option<Paused>,
    running: bool,
    done: bool,
    output: Vec<String>,
    error: Option<String>,
    /// Source line this session last paused on; guards against re-firing on
    /// later opcodes of the same line.
    last_hit_line: u32,
}

/// Frame writer shared between the DAP reader and the VM worker. The stdout
/// lock and the sequence counter are shared so frames never interleave.
#[derive(Clone)]
struct Writer {
    out: Arc<Mutex<std::io::Stdout>>,
    seq: Arc<AtomicU64>,
}

impl Writer {
    fn new() -> Self {
        Writer {
            out: Arc::new(Mutex::new(std::io::stdout())),
            seq: Arc::new(AtomicU64::new(0)),
        }
    }

    fn send(&self, body: &Value) {
        let frame = format!("Content-Length: {}\r\n\r\n{}", body.to_string().len(), body);
        let mut out = self.out.lock().unwrap();
        let _ = out.write_all(frame.as_bytes());
        let _ = out.flush();
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn response(&self, id: u64, command: &str, inner: Value) {
        self.send(&json!({
            "seq": self.next_seq(),
            "type": "response",
            "request_seq": id,
            "success": true,
            "command": command,
            "body": inner,
        }));
    }

    fn event(&self, name: &str, body: Value) {
        self.send(&json!({
            "seq": self.next_seq(),
            "type": "event",
            "event": name,
            "body": body,
        }));
    }
}

/// DAP reader on the main thread: stdin only (responses go through the
/// shared [`Writer`]).
struct Conn {
    stdin: std::io::Stdin,
}

impl Conn {
    /// Read one DAP frame from stdin. `Err` on EOF/malformed input.
    fn read_frame(&mut self) -> Result<Value, String> {
        let mut stdin = self.stdin.lock();
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            let n = stdin
                .read_line(&mut line)
                .map_err(|e| format!("dap: read error: {}", e))?;
            if n == 0 {
                return Err("dap: eof".to_string());
            }
            let trimmed = line.trim_end_matches("\r\n").trim_end_matches('\n');
            if trimmed.is_empty() {
                break;
            }
            if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
                content_length = rest.trim().parse::<usize>().ok();
            }
        }
        let len = content_length.ok_or_else(|| "dap: missing Content-Length".to_string())?;
        let mut buf = vec![0u8; len];
        stdin
            .read_exact(&mut buf)
            .map_err(|e| format!("dap: body read error: {}", e))?;
        serde_json::from_slice(&buf).map_err(|e| format!("dap: bad json: {}", e))
    }
}

/// Value rendering used in the locals/globals snapshots.
fn val_to_str(v: &crate::value::Value) -> String {
    format!("{}", v)
}

/// Spawn the program on a worker thread with DAP-driven breakpoints/stepping.
fn spawn_worker(
    file: &str,
    source: &str,
    state: Arc<Mutex<Shared>>,
    cvar: Arc<Condvar>,
    writer: Writer,
) -> std::thread::JoinHandle<()> {
    let base_dir = std::path::Path::new(file)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());
    let source = source.to_string();

    std::thread::spawn(move || {
        let tokens = match crate::lexer::tokenize(&source) {
            Ok(t) => t,
            Err(e) => {
                let mut s = state.lock().unwrap();
                s.done = true;
                s.error = Some(format!("Lexer error: {}", e));
                drop(s);
                writer.event(
                    "output",
                    json!({"category": "stderr", "output": format!("Lexer error: {}\n", e)}),
                );
                writer.event("terminated", json!({"restart": false}));
                return;
            }
        };
        let ast = match crate::parser::parse(&tokens, &source) {
            Ok(a) => a,
            Err(e) => {
                let mut s = state.lock().unwrap();
                s.done = true;
                s.error = Some(format!("Parser error: {}", e));
                drop(s);
                writer.event(
                    "output",
                    json!({"category": "stderr", "output": format!("Parser error: {}\n", e)}),
                );
                writer.event("terminated", json!({"restart": false}));
                return;
            }
        };
        let chunk = match crate::compiler::compile_module_in(&ast, &base_dir) {
            Ok(c) => c,
            Err(e) => {
                let mut s = state.lock().unwrap();
                s.done = true;
                s.error = Some(format!("Compile error: {}", e));
                drop(s);
                writer.event(
                    "output",
                    json!({"category": "stderr", "output": format!("Compile error: {}\n", e)}),
                );
                writer.event("terminated", json!({"restart": false}));
                return;
            }
        };

        let mut vm = Vm::new();
        let st = state.clone();
        let cv_in = cvar.clone();
        let w_stopped = writer.clone();
        vm.debugger(HashSet::new(), move |line, locals, globals, _callstack| {
            let mut s = st.lock().unwrap();
            // Pause once per source line: a breakpoint (or a requested step)
            // only fires when `line` differs from the line we last paused on,
            // so later opcodes of the same line don't re-trigger.
            let fresh_line = line != s.last_hit_line;
            let on_bp = fresh_line && s.breakpoints.contains(&line);
            let on_step = s.step && fresh_line;
            if !on_bp && !on_step {
                s.running = true;
                drop(s);
                return VmDebugAction::Continue;
            }
            s.step = false;
            s.last_hit_line = line;
            s.paused = Some(Paused {
                line,
                locals: locals
                    .iter()
                    .map(|(k, v)| (k.clone(), val_to_str(v)))
                    .collect(),
                globals: {
                    let mut g: Vec<(String, String)> = globals
                        .iter()
                        .map(|(k, v)| (k.clone(), val_to_str(v)))
                        .collect();
                    g.sort_by(|a, b| a.0.cmp(&b.0));
                    g
                },
            });
            s.running = false;
            s.cmd = Cmd::Noop;
            drop(s);
            w_stopped.event(
                "stopped",
                json!({"reason": "breakpoint", "threadId": 1, "allThreadsStopped": true}),
            );
            let mut s = st.lock().unwrap();
            // Park until the reader sends a resume command.
            while !s.done && s.cmd == Cmd::Noop {
                s = cv_in.wait(s).unwrap();
            }
            let cmd = s.cmd;
            s.cmd = Cmd::Noop;
            s.paused = None;
            s.running = true;
            drop(s);
            match cmd {
                Cmd::Continue => {
                    // Clear any stale step request.
                    st.lock().unwrap().step = false;
                    VmDebugAction::Continue
                }
                Cmd::Next => {
                    // Pause at the very next source line.
                    st.lock().unwrap().step = true;
                    VmDebugAction::Step
                }
                Cmd::Quit => VmDebugAction::Quit,
                Cmd::Noop => VmDebugAction::Continue,
            }
        });

        let result = vm.run(&chunk);

        let mut s = state.lock().unwrap();
        match result {
            Ok(lines) => {
                s.output = lines;
                for line in &s.output {
                    writer.event(
                        "output",
                        json!({"category": "stdout", "output": format!("{}\n", line)}),
                    );
                }
            }
            Err(e) => s.error = Some(e),
        }
        s.done = true;
        s.running = false;
        s.paused = None;
        if let Some(e) = &s.error {
            writer.event(
                "output",
                json!({"category": "stderr", "output": format!("{}\n", e)}),
            );
        }
        drop(s);
        cvar.notify_all();
        writer.event("terminated", json!({"restart": false}));
    })
}

/// Run the DAP server for `source` (already read from `file`) on stdio.
pub fn run(file: &str, source: &str) {
    let writer = Writer::new();
    let mut conn = Conn {
        stdin: std::io::stdin(),
    };
    let state = Arc::new(Mutex::new(Shared {
        breakpoints: HashSet::new(),
        step: false,
        cmd: Cmd::Noop,
        paused: None,
        running: false,
        done: false,
        output: Vec::new(),
        error: None,
        last_hit_line: 0,
    }));
    let cvar = Arc::new(Condvar::new());

    let mut worker: Option<std::thread::JoinHandle<()>> = None;
    let mut configured = false;

    loop {
        let msg = match conn.read_frame() {
            Ok(m) => m,
            Err(_) => break,
        };
        let seq = msg.get("seq").and_then(|s| s.as_u64()).unwrap_or(0);
        let command = msg
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        let args = msg.get("arguments").cloned().unwrap_or(Value::Null);

        match command.as_str() {
            "initialize" => {
                writer.response(
                    seq,
                    &command,
                    json!({
                        "supportsConfigurationDoneRequest": true,
                        "supportsEvaluate": true,
                        "supportsSetVariable": false,
                        "supportsTerminateRequest": true,
                        "supportsRestartRequest": false,
                    }),
                );
                writer.event("initialized", json!({}));
            }
            "setBreakpoints" => {
                let lines: Vec<u32> = args
                    .get("breakpoints")
                    .and_then(|b| b.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|b| b.get("line").and_then(|l| l.as_u64()))
                            .map(|l| l as u32)
                            .collect()
                    })
                    .unwrap_or_default();
                let mut s = state.lock().unwrap();
                // DAP clients send 1-based source lines; the VM's opcode line
                // table is 0-based, so translate before comparing.
                s.breakpoints = lines.iter().map(|l| l.saturating_sub(1)).collect();
                drop(s);
                let bps: Vec<Value> = lines
                    .iter()
                    .map(|l| json!({"verified": true, "line": l}))
                    .collect();
                writer.response(seq, &command, json!({"breakpoints": bps}));
            }
            "setExceptionBreakpoints" => {
                writer.response(seq, &command, json!({"breakpoints": []}));
            }
            "configurationDone" => {
                writer.response(seq, &command, json!({}));
                if !configured && worker.is_none() {
                    configured = true;
                    worker = Some(spawn_worker(
                        file,
                        source,
                        state.clone(),
                        cvar.clone(),
                        writer.clone(),
                    ));
                }
            }
            "launch" | "attach" => {
                writer.response(seq, &command, json!({}));
            }
            "threads" => {
                writer.response(
                    seq,
                    &command,
                    json!({"threads": [{"id": 1, "name": "main"}]}),
                );
            }
            "stackTrace" => {
                let paused = state.lock().unwrap().paused.clone();
                let frames: Vec<Value> = match &paused {
                    Some(p) => vec![json!({
                        "id": 1,
                        "name": "main",
                        // VM line table is 0-based; DAP lines are 1-based.
                        "line": p.line + 1,
                        "column": 1,
                        "source": {"name": file.rsplit(['/', '\\']).next().unwrap_or(file), "path": file},
                    })],
                    None => vec![],
                };
                writer.response(
                    seq,
                    &command,
                    json!({"stackFrames": frames, "totalFrames": frames.len()}),
                );
            }
            "scopes" => {
                let has = state.lock().unwrap().paused.is_some();
                let scopes = if has {
                    json!([
                        {"name": "Locals", "variablesReference": 1, "expensive": false},
                        {"name": "Globals", "variablesReference": 2, "expensive": true},
                    ])
                } else {
                    json!([])
                };
                writer.response(seq, &command, json!({"scopes": scopes}));
            }
            "variables" => {
                let ref_id = args
                    .get("variablesReference")
                    .and_then(|r| r.as_u64())
                    .unwrap_or(0);
                let paused = {
                    let s = state.lock().unwrap();
                    match (&s.paused, ref_id) {
                        (Some(p), 1) => Some(p.locals.clone()),
                        (Some(p), 2) => Some(p.globals.clone()),
                        _ => None,
                    }
                };
                let vars: Vec<Value> = paused
                    .unwrap_or_default()
                    .iter()
                    .map(|(k, v)| json!({"name": k, "value": v, "variablesReference": 0}))
                    .collect();
                writer.response(seq, &command, json!({"variables": vars}));
            }
            "evaluate" => {
                let expr = args
                    .get("expression")
                    .and_then(|e| e.as_str())
                    .unwrap_or("");
                let paused = state.lock().unwrap().paused.clone();
                let value = match &paused {
                    Some(p) => p
                        .locals
                        .iter()
                        .chain(p.globals.iter())
                        .find(|(k, _)| k == expr)
                        .map(|(_, v)| v.clone()),
                    None => None,
                };
                match value {
                    Some(v) => writer.response(
                        seq,
                        &command,
                        json!({"result": v, "variablesReference": 0}),
                    ),
                    None => writer.response(
                        seq,
                        &command,
                        json!({"result": "", "variablesReference": 0}),
                    ),
                }
            }
            "source" => {
                writer.response(
                    seq,
                    &command,
                    json!({"content": source, "mimeType": "text/plain"}),
                );
            }
            "continue" | "next" => {
                let mut s = state.lock().unwrap();
                if s.paused.is_some() {
                    s.cmd = if command == "continue" {
                        Cmd::Continue
                    } else {
                        Cmd::Next
                    };
                    cvar.notify_all();
                }
                drop(s);
                writer.response(seq, &command, json!({}));
            }
            "pause" => {
                let mut s = state.lock().unwrap();
                if s.running {
                    s.step = true;
                }
                drop(s);
                writer.response(seq, &command, json!({}));
            }
            "disconnect" | "terminate" => {
                let mut s = state.lock().unwrap();
                s.done = true;
                s.cmd = Cmd::Quit;
                cvar.notify_all();
                drop(s);
                writer.response(seq, &command, json!({}));
                break;
            }
            // Unknown requests: respond gracefully so clients don't block.
            _ => {
                writer.response(seq, &command, json!({}));
            }
        }
    }

    if let Some(w) = worker.take() {
        let _ = w.join();
    }
}
