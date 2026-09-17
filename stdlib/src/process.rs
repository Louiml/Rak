//! Process management: spawn, wait, kill, and read stdout/stderr.
//!
//! Thin wrapper over `std::process::Command`. All values are converted to UTF-8
//! lossily. Each spawned process is tracked by a synthetic Rak pid (u64) that
//! maps to a `ChildInfo` holding the OS `Child` plus a cached exit code. Once
//! reaped, the exit code is cached so `wait`/`stdout`/`stderr` may be called in
//! any order.

use std::collections::HashMap;
use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Mutex;

struct ChildInfo {
    child: Option<Child>,
    exit: Option<i64>,
    reaped: bool,
}

static TABLE: Mutex<Option<HashMap<u64, ChildInfo>>> = Mutex::new(None);

fn next_pid() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::SeqCst)
}

fn table_mut() -> Result<std::sync::MutexGuard<'static, Option<HashMap<u64, ChildInfo>>>, std::sync::PoisonError<std::sync::MutexGuard<'static, Option<HashMap<u64, ChildInfo>>>>> {
    TABLE.lock()
}

fn insert(pid: u64, child: Child) {
    if let Ok(mut g) = table_mut() {
        g.get_or_insert_with(HashMap::new).insert(pid, ChildInfo { child: Some(child), exit: None, reaped: false });
    }
}

fn code_of(status: ExitStatus) -> i64 {
    status.code().map(|c| c as i64).unwrap_or(-1)
}

fn reap(info: &mut ChildInfo) {
    if let Some(c) = info.child.as_mut() {
        let status = c.wait();
        info.exit = Some(status.map(code_of).unwrap_or(-1));
    }
    info.reaped = true;
}

fn get_mut(pid: u64) -> Option<std::sync::MutexGuard<'static, Option<HashMap<u64, ChildInfo>>>> {
    let g = table_mut().ok()?;
    if g.as_ref().map(|m| m.contains_key(&pid)).unwrap_or(false) {
        Some(g)
    } else {
        None
    }
}

/// Spawn a process capturing stdout/stderr. Returns Ok(pid).
pub fn spawn(cmd: &str, args: &[String]) -> Result<u64, String> {
    let child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("process_spawn: {}: {}", cmd, e))?;
    let pid = next_pid();
    insert(pid, child);
    Ok(pid)
}

/// Spawn a process inheriting this process's stdio (interactive tools).
pub fn spawn_inherit(cmd: &str, args: &[String]) -> Result<u64, String> {
    let child = Command::new(cmd)
        .args(args)
        .spawn()
        .map_err(|e| format!("process_spawn: {}: {}", cmd, e))?;
    let pid = next_pid();
    insert(pid, child);
    Ok(pid)
}

/// Wait for a process to exit. Returns its exit code.
pub fn wait(pid: u64) -> Result<i64, String> {
    let mut g = get_mut(pid).ok_or_else(|| format!("process_wait: unknown pid {}", pid))?;
    let info = g.as_mut().unwrap().get_mut(&pid).unwrap();
    if info.exit.is_none() {
        reap(info);
    }
    Ok(info.exit.unwrap_or(-1))
}

/// Blockingly wait then read captured stdout into a string.
pub fn read_stdout(pid: u64) -> Result<String, String> {
    let mut g = get_mut(pid).ok_or_else(|| format!("process_stdout: unknown pid {}", pid))?;
    let info = g.as_mut().unwrap().get_mut(&pid).unwrap();
    if info.exit.is_none() {
        reap(info);
    }
    let mut buf = String::new();
    if let Some(c) = info.child.as_mut() {
        if let Some(mut out) = c.stdout.take() {
            let _ = out.read_to_string(&mut buf);
        }
    }
    Ok(buf)
}

/// Blockingly wait then read captured stderr into a string.
pub fn read_stderr(pid: u64) -> Result<String, String> {
    let mut g = get_mut(pid).ok_or_else(|| format!("process_stderr: unknown pid {}", pid))?;
    let info = g.as_mut().unwrap().get_mut(&pid).unwrap();
    if info.exit.is_none() {
        reap(info);
    }
    let mut buf = String::new();
    if let Some(c) = info.child.as_mut() {
        if let Some(mut err) = c.stderr.take() {
            let _ = err.read_to_string(&mut buf);
        }
    }
    Ok(buf)
}

/// Kill a running process.
pub fn kill(pid: u64) -> Result<(), String> {
    let mut g = get_mut(pid).ok_or_else(|| format!("process_kill: unknown pid {}", pid))?;
    let info = g.as_mut().unwrap().get_mut(&pid).unwrap();
    if let Some(c) = info.child.as_mut() {
        c.kill().map_err(|e| format!("process_kill: {}", e))?;
        let _ = c.wait();
        info.exit = Some(-9);
    }
    Ok(())
}

/// Whether the process is still tracked (running or awaiting reap).
pub fn is_alive(pid: u64) -> bool {
    get_mut(pid).is_some()
}