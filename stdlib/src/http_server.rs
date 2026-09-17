//! A minimal HTTP/1.1 server with a pull-based request model.
//!
//! Rak is single-threaded and its values are not `Send`, so requests/responses
//! cannot cross between the server thread and Rak closures directly. The design
//! therefore cycles through the main thread:
//!
//!   1. `server_start(addr, port)` spawns a thread that accepts connections,
//!      parses each HTTP/1.1 request, and queues it.
//!   2. The Rak script pulls requests with `poll()` (non-blocking, returns nil
//!      when idle) and inspects `{id, method, path, query, headers, body}`.
//!   3. The script sends a response with `respond(id, status, headers, body)`,
//!      which unblocks the server thread so it writes the bytes and closes.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, SyncSender};
use std::sync::Mutex;

/// A parsed HTTP request handed to the interpreter.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub id: u64,
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: String,
}

static REQ_COUNTER: AtomicU64 = AtomicU64::new(1);
static REQUESTS: Mutex<Option<Receiver<HttpRequest>>> = Mutex::new(None);
static RESPONDERS: Mutex<Option<HashMap<u64, SyncSender<String>>>> = Mutex::new(None);

/// Start an HTTP server on `addr:port`. Returns the bound port, or directs the
/// error if already running. Requests are queued for `poll`.
pub fn server_start(addr: &str, port: u16) -> Result<u16, String> {
    if REQUESTS.lock().map(|g| g.is_some()).unwrap_or(false) {
        return Err("http_server: already running".to_string());
    }
    let listener = TcpListener::bind((addr, port)).map_err(|e| format!("http_server: bind {}:{}: {}", addr, port, e))?;
    let bound = listener.local_addr().map_err(|e| format!("http_server: local_addr: {}", e))?.port();
    let (req_tx, req_rx) = channel::<HttpRequest>();
    *REQUESTS.lock().map_err(|_| "http_server: lock".to_string())? = Some(req_rx);

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            handle_conn(stream, &req_tx);
        }
    });
    Ok(bound)
}

fn handle_conn(mut stream: TcpStream, req_tx: &std::sync::mpsc::Sender<HttpRequest>) {
    let Ok(clone) = stream.try_clone() else { return };
    let mut reader = BufReader::new(clone);
    // Request line.
    let mut line = String::new();
    let _ = reader.read_line(&mut line);
    let line = line.trim_end_matches("\r\n");
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let raw_path = parts.next().unwrap_or("").to_string();
    // Headers.
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        let n = reader.read_line(&mut h).unwrap_or(0);
        if n == 0 {
            break;
        }
        let t = h.trim_end_matches("\r\n");
        if t.is_empty() {
            break;
        }
        if let Some(idx) = t.find(':') {
            headers.insert(t[..idx].trim().to_lowercase(), t[idx + 1..].trim().to_string());
        }
    }
    // Body.
    let len: usize = headers.get("content-length").and_then(|v| v.trim().parse().ok()).unwrap_or(0);
    let mut body = String::new();
    if len > 0 {
        let mut buf = vec![0u8; len];
        let mut got = 0;
        while got < len {
            match reader.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(_) => break,
            }
        }
        body = String::from_utf8_lossy(&buf[..got]).to_string();
    }
    let (path, query) = split_query(&raw_path);
    let id = REQ_COUNTER.fetch_add(1, Ordering::SeqCst);
    let (rtx, rrx) = sync_channel::<String>(0);
    RESPONDERS.lock().ok().and_then(|mut g| g.get_or_insert_with(HashMap::new).insert(id, rtx));
    let req = HttpRequest { id, method, path, query, headers, body };
    let _ = req_tx.send(req);
    if let Ok(resp) = rrx.recv() {
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
    }
    RESPONDERS.lock().ok().and_then(|mut g| g.as_mut().map(|m| m.remove(&id)));
}

fn split_query(path: &str) -> (String, HashMap<String, String>) {
    let mut q = HashMap::new();
    let (p, query) = match path.find('?') {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => (path, ""),
    };
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut kv = pair.splitn(2, '=');
        let k = url_decode(kv.next().unwrap_or(""));
        let v = url_decode(kv.next().unwrap_or(""));
        q.insert(k, v);
    }
    (p.to_string(), q)
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Non-blocking: return the next queued request, or None.
pub fn poll() -> Option<HttpRequest> {
    let g = REQUESTS.lock().ok()?;
    let rx = g.as_ref()?;
    rx.try_recv().ok()
}

/// Queue an HTTP response for request `id`.
pub fn respond(id: u64, status: u16, headers: &[(String, String)], body: &str) -> Result<(), String> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", status, status_text(status));
    head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    head.push_str("Connection: close\r\n");
    for (k, v) in headers {
        head.push_str(k);
        head.push_str(": ");
        head.push_str(v);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let response = head + body;
    let sender = RESPONDERS
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|m| m.get(&id).cloned()))
        .ok_or_else(|| format!("http_respond: unknown or closed connection {}", id))?;
    sender.send(response).map_err(|_| format!("http_respond: connection {} closed", id))
}

/// Stop the server and clear all state.
pub fn server_stop() {
    if let Ok(mut g) = REQUESTS.lock() {
        *g = None;
    }
    if let Ok(mut g) = RESPONDERS.lock() {
        *g = None;
    }
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    }
}