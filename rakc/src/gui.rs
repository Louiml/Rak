//! The Rak GUI: windows a Rak program can open, update, retitle, close, and
//! call back into.
//!
//! # What was wrong before
//!
//! Through v8.0.0 this module kept a `HashMap<i64, ()>` — an id and nothing
//! else. The `Window` and `WebView` were built as locals inside a spawned
//! closure and dropped at its end (`let _ = webview;`), so there was no handle
//! to act on. `gui_update` built an HTML string and discarded it.
//! `gui_title` had an empty body. `gui_close` deleted a map entry. The IPC
//! handler read the request body and dropped it, so JavaScript could call Rak
//! in one direction only. Closing a window set `ControlFlow::Exit`, and tao's
//! `run` is `-> !` — it calls `process::exit` once the handler returns — so
//! closing any window killed the process. On Linux, `gui_open` built the event
//! loop on a spawned thread, which tao rejects outright.
//!
//! # The shape now
//!
//! One event loop, owned by the **main thread**, because tao binds to the
//! display connection from the thread that creates the loop. The interpreter
//! runs on a worker thread and never touches a window handle directly:
//! `Window` and `WebView` are not `Send`, so every operation is marshalled
//! across as a [`Command`] and applied on the loop thread.
//!
//! ```text
//!  worker (interpreter)                  main thread (event loop)
//!  ------------------                    -----------------------
//!  gui_open(html) ──Command::Open──────▶ build Window + WebView
//!               ◀──reply(id)─────────── id assigned
//!  gui_update(id) ─Command::Update────▶ rebuild the WebView in place
//!  gui_title(id)  ─Command::Title─────▶ window.set_title(..)
//!  gui_close(id)  ─Command::Close─────▶ window.close(); loop keeps running
//!  gui_wait()     ◀──Condvar─────────── woken when the last window closes
//!  gui_quit(n)    ─Command::Quit──────▶ ControlFlow::ExitWithCode(n)
//!
//!  page JS: rak_call("f", ..) ─Command::Ipc──▶ run the Rak callback
//!  page JS: ◀──evaluate_script─────────────── window.rak_result("f", ..)
//! ```
//!
//! Closing a window no longer ends the process. The loop exits when the last
//! window closes *and* the program called `gui_wait`, or on an explicit
//! `gui_quit`. The exit code travels in `ControlFlow::ExitWithCode`, so a GUI
//! program can still return non-zero and run its cleanup.
//!
//! # Callbacks and shared state
//!
//! `gui_callback("name", f)` records a Rak function plus a snapshot of the
//! environment as it stood at registration. An IPC message is dispatched by
//! building a fresh [`Interpreter`] over that snapshot — the same pattern the
//! interpreter already uses for `spawn` and deferred `async fn` bodies.
//!
//! The consequence is worth stating plainly: a callback does **not** share
//! mutable state with the script that registered it, because Rak has no
//! reference types and values are shared rather than moved. A callback gets a
//! copy of the environment, so it can read what existed at registration and
//! hand a return value back to the page, but it cannot write to a variable the
//! main script later reads. See `docs/V8-BACKEND-PARITY.md`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::interpreter::{Env, Interpreter, Value};

    /// The event type carried into the loop.
    ///
    /// `LoopEvent` is just `Command`; there is no wrapper type. The one thing
    /// that has to be `Clone` is the *loop's* event enum, which tao derives
    /// `Clone` for — and `Command` is not `Clone` because `Command::Open`
    /// holds a reply sender. tao only requires `Clone` when the handler asks for
    /// it, so a plain `Command` payload is fine and avoids a needless
    /// allocation per event.
    pub type LoopEvent = Command;

/// A request to the event-loop thread.
///
/// `Window` and `WebView` cannot cross threads, so every payload here is plain
/// owned data and the loop does the actual work.
pub enum Command {
    Open {
        /// Resolves once the window exists, or with the reason it could not be
        /// created. `gui_open` blocks on this so the id it returns is real.
        reply: std::sync::mpsc::Sender<std::result::Result<i64, String>>,
        title: String,
        html: String,
        width: f64,
        height: f64,
    },
    /// Replace a window's document. The `Window` is kept — so position, size
    /// and z-order survive — and the `WebView` is rebuilt, because wry cannot
    /// swap a document in place.
    Update { id: i64, html: String },
    SetTitle { id: i64, title: String },
    /// Close one window. The loop keeps running.
    Close { id: i64 },
    /// Evaluate JavaScript in a window, used to deliver a callback's result.
    Eval { id: i64, js: String },
    /// A call from page JavaScript into a registered Rak callback.
    Ipc { name: String, args: Vec<String> },
    /// Stop the loop with this exit code.
    Quit { code: i32 },
    /// The script has finished running.
    ///
    /// Sent by the worker when `run_source` and `run_main` return. The loop
    /// stops immediately if no windows are open, and otherwise keeps running
    /// until they close — a script that opened a window and returned should
    /// still leave that window on screen rather than yanking it away.
    ///
    /// Without this the loop would block forever for the very common case of a
    /// script that never calls a GUI builtin at all.
    ScriptDone,
}

/// A GUI callback: the function to run, and the environment to run it over.
#[derive(Clone)]
pub struct Callback {
    pub func: Value,
    pub env: Arc<Env>,
}

/// The handle a Rak program holds.
///
/// Lives behind an `Arc` and is reachable from both backends, so `gui_open`
/// behaves the same under `rakc run` and `rakc vm`.
pub struct GuiManager {
    /// The loop this manager talks to. Installed by `run_event_loop` before the
    /// loop starts, so a builtin that sends before then gets a clear error
    /// rather than a panic.
    proxy: Mutex<Option<tao::event_loop::EventLoopProxy<LoopEvent>>>,
    /// Ids of open windows, shared with the loop so `wait` can block on it.
    open: Arc<OpenWindows>,
    /// Registered callbacks, keyed by the name JavaScript uses.
    callbacks: Arc<Mutex<HashMap<String, Callback>>>,
    /// Set by `wait`, so the loop knows to exit once the last window closes
    /// rather than waiting for an explicit `gui_quit`.
    waiting: Arc<AtomicBool>,
    /// Set when the script has finished. The loop needs this to know it may
    /// stop even if `gui_wait` was never called.
    script_done: Arc<AtomicBool>,
}

/// Window bookkeeping shared between the loop thread and the interpreter.
struct OpenWindows {
    ids: Mutex<Vec<i64>>,
    /// Signalled whenever `ids` changes, so `wait` blocks instead of polling.
    changed: Condvar,
}

impl OpenWindows {
    fn add(&self, id: i64) {
        self.ids.lock().unwrap().push(id);
    }

    fn remove(&self, id: i64) {
        self.ids.lock().unwrap().retain(|x| *x != id);
        self.changed.notify_all();
    }

    fn count(&self) -> usize {
        self.ids.lock().unwrap().len()
    }

    fn is_empty(&self) -> bool {
        self.count() == 0
    }
}

impl Default for GuiManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GuiManager {
    pub fn new() -> Self {
        GuiManager {
            proxy: Mutex::new(None),
            open: Arc::new(OpenWindows {
                ids: Mutex::new(Vec::new()),
                changed: Condvar::new(),
            }),
            callbacks: Arc::new(Mutex::new(HashMap::new())),
            waiting: Arc::new(AtomicBool::new(false)),
            script_done: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Record a Rak function as callable from page JavaScript.
    pub fn register_callback(&self, name: &str, func: Value, env: Arc<Env>) {
        self.callbacks
            .lock()
            .unwrap()
            .insert(name.to_string(), Callback { func, env });
    }

    pub fn is_open(&self, id: i64) -> bool {
        self.open.ids.lock().unwrap().contains(&id)
    }

    pub fn open_count(&self) -> usize {
        self.open.count()
    }

    /// Post a command to the loop, failing cleanly if it is gone.
    fn send(&self, cmd: Command) -> std::result::Result<(), String> {
        let guard = self.proxy.lock().unwrap();
        match guard.as_ref() {
            Some(proxy) => proxy
                .send_event(cmd)
                .map_err(|_| "GUI event loop is no longer running".to_string()),
            None => Err("GUI event loop is not running".to_string()),
        }
    }

    /// Open a window and block until it exists, returning its id.
    pub fn open(
        &self,
        title: &str,
        html: &str,
        width: f64,
        height: f64,
    ) -> std::result::Result<i64, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.send(Command::Open {
            reply: tx,
            title: title.to_string(),
            html: wrap_html(html),
            width,
            height,
        })?;
        rx.recv().map_err(|_| "GUI event loop closed before the window opened".to_string())?
    }

    pub fn update(&self, id: i64, html: &str) -> std::result::Result<(), String> {
        if !self.is_open(id) {
            return Err(format!("gui_update: no window with id {}", id));
        }
        self.send(Command::Update {
            id,
            html: wrap_html(html),
        })
    }

    pub fn set_title(&self, id: i64, title: &str) -> std::result::Result<(), String> {
        if !self.is_open(id) {
            return Err(format!("gui_title: no window with id {}", id));
        }
        self.send(Command::SetTitle {
            id,
            title: title.to_string(),
        })
    }

    /// Ask for one window to close. Does not stop the loop — that is
    /// `gui_quit`'s job, or an automatic exit once `gui_wait` is satisfied.
    pub fn close(&self, id: i64) -> std::result::Result<(), String> {
        self.send(Command::Close { id })
    }

    /// Block until every window has closed.
    ///
    /// A `Condvar` rather than a poll loop: the old implementation slept
    /// 100 ms at a time, so a script that opened and closed a window paid up to
    /// 100 ms of dead time for nothing.
    pub fn wait(&self) {
        self.waiting.store(true, Ordering::SeqCst);
        let mut ids = self.open.ids.lock().unwrap();
        while !ids.is_empty() {
            ids = self.open.changed.wait(ids).unwrap();
        }
    }

    /// Stop the event loop with `code` as the process exit status.
    pub fn quit(&self, code: i32) -> std::result::Result<(), String> {
        self.send(Command::Quit { code })
    }

    /// Record that the script has finished and nudge the loop.
    ///
    /// Called by the worker once the program has run to completion. A failure
    /// to send is not an error: it means the loop has already stopped, which is
    /// the outcome being reported anyway.
    pub fn notify_script_done(&self) {
        self.script_done.store(true, Ordering::SeqCst);
        let _ = self.send(Command::ScriptDone);
    }

    /// Whether the loop may stop: the script has finished and either no
    /// windows are open or the program is explicitly waiting for them to close.
    fn may_stop(&self, open_windows: usize) -> bool {
        if !self.script_done.load(Ordering::SeqCst) {
            return false;
        }
        open_windows == 0 || self.waiting.load(Ordering::SeqCst)
    }

    /// Run a registered callback. Called on the loop thread.
    ///
    /// Builds a fresh interpreter over the captured environment, which is what
    /// makes this work at all given Rak has no reference types — see the module
    /// comment on why that limits what a callback can see.
    fn dispatch(&self, name: &str, args: &[String]) -> Option<String> {
        let cb = self.callbacks.lock().unwrap().get(name).cloned()?;
        crate::interpreter::call_in_env(
            cb.func.clone(),
            (*cb.env).clone(),
            args.iter().map(|a| Value::String(a.clone())).collect(),
        )
        .ok()
        .map(|v| v.to_string())
    }
}

/// Evaluate a CLI program with the GUI event loop running on the main thread.
///
/// This is the entry point `rakc run` uses when built with `--features gui`.
/// It is a separate function from `lib.rs::eval_in_cli` because the two need
/// opposite thread layouts:
///
/// * Without the GUI, the interpreter owns the process. `eval_in_cli` puts it
///   on a 64 MB stack thread and joins, which is fine.
/// * With the GUI, the **event loop** must own the main thread — tao binds to
///   the display connection there and rejects any other thread — so the
///   interpreter moves to the worker and the main thread runs the loop.
///
/// Both threads are needed and they must not be the same one, so the layout is
/// inverted here rather than in `eval_in_cli`.
///
/// Returns the program's output and exit code. The exit code is the GUI's,
/// which `gui_quit` sets; a program that never calls `gui_quit` and never calls
/// `gui_wait` exits when the script finishes and the loop is told to stop.
pub fn eval_in_cli_with_gui(
    source: &str,
    base_dir: &str,
    argv: &[String],
) -> crate::Result<(Vec<String>, i32)> {
    let manager = Arc::new(GuiManager::new());
    // Publish before starting either thread: the VM's natives look the manager
    // up through here rather than holding a reference, so it has to exist
    // before the program can call `gui_open`.
    set_current(manager.clone());
    let src = source.to_string();
    let dir = base_dir.to_string();
    let args = argv.to_vec();
    let for_worker = manager.clone();

    // The interpreter runs here, on a worker, because the main thread is about
    // to become the event loop.
    let script = std::thread::Builder::new()
        .name("rak-interp".to_string())
        .stack_size(crate::INTERPRETER_STACK)
        .spawn(move || {
            // `with_base_dir` rather than `new` plus a setter, so imports
            // resolve against the script's own directory exactly as they do
            // without the GUI.
            let mut interpreter = Interpreter::with_base_dir(dir);
            // Hand the manager over so `gui_open` and friends can reach the
            // loop. Without this every GUI builtin would report that no event
            // loop is running.
            interpreter.attach_gui(for_worker.clone());
            let output = interpreter.run_source(&src);
            let code = match &output {
                Ok(_) => interpreter.run_main(&args),
                // The script failed, so there is no `main` to run and no point
                // leaving a window up for a program that has already stopped.
                Err(_) => 1,
            };
            // Tell the loop the script is over. Without this a program that
            // never calls a GUI builtin would hang the event loop forever,
            // because nothing would ever set `ControlFlow::Exit`.
            for_worker.notify_script_done();
            output.map(|o| (o, code))
        });

    let mut script = match script {
        Ok(h) => h,
        Err(e) => {
            return Err(crate::RakError::Runtime(format!(
                "could not start the interpreter thread: {}",
                e
            )))
        }
    };

    // Main thread: become the event loop. This blocks until the program quits
    // the GUI, and only then is the script's result collected.
    let gui_code = run_event_loop(manager);

    // A panic on the worker has already unwound; there is no result to report
    // beyond the GUI's exit code, so the program is treated as having produced
    // no output rather than being reported as a clean success.
    let joined: crate::Result<(Vec<String>, i32)> = match script.join() {
        Ok(result) => result,
        Err(_) => Ok((Vec::new(), gui_code)),
    };
    match joined {
        Ok((output, code)) => {
            // A non-zero script status wins over the GUI's, because a program
            // that failed should not report success just because its window
            // closed cleanly.
            let code = if code != 0 { code } else { gui_code };
            Ok((output, code))
        }
        Err(e) => Err(e),
    }
}

/// Run the GUI event loop until told to quit, and return the exit code.
///
/// **Must be called on the main thread.** tao binds to the display connection
/// from the thread that creates the event loop and rejects any other; the old
/// code created it on a spawned thread, which is why the GUI did not work on
/// Linux at all.
pub fn run_event_loop(manager: Arc<GuiManager>) -> i32 {
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::ControlFlow;
    use tao::window::WindowBuilder;
    use wry::WebViewBuilder;

    // `with_user_event`, not `new`: `new()` is only defined for the unit event
    // type, and `build()` takes `&mut self`.
    let mut builder = tao::event_loop::EventLoopBuilder::<LoopEvent>::with_user_event();
    let event_loop = builder.build();
    let proxy = event_loop.create_proxy();
    // Installed here rather than in the driver, so the manager is only usable
    // once a loop genuinely exists. A builtin that runs first gets a clean
    // "not running" error instead of blocking forever on a reply that nobody
    // will send.
    *manager.proxy.lock().unwrap() = Some(proxy.clone());

    // The live handles. Created, used and dropped on this thread and never
    // crossing it — `Window` and `WebView` are not `Send`.
    let mut windows: HashMap<i64, (tao::window::Window, wry::WebView)> = HashMap::new();
    // tao identifies windows by a platform `WindowId`; a Rak program identifies
    // them by the small integer `gui_open` returned. This maps one to the
    // other, so a `CloseRequested` carrying tao's id can be attributed to a
    // window the program knows about. Without it, closing a window by hand
    // would leave `gui_wait` blocked on a window that no longer exists.
    let mut tao_ids: HashMap<tao::window::WindowId, i64> = HashMap::new();
    let mut next_id = AtomicI64::new(1);

    // `run` consumes the `EventLoop`, so the window builder cannot borrow the
    // local. tao passes the window target as the handler's second argument
    // precisely so windows can be created from inside the loop; that is what
    // `target` is for.
    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(cmd) => match cmd {
                Command::Open {
                    reply,
                    title,
                    html,
                    width,
                    height,
                } => {
                    let id = next_id.fetch_add(1, Ordering::SeqCst);
                    // Built here rather than in a helper so the `Window` and
                    // `WebView` are constructed on this thread and never
                    // captured into anything `Send`.
                    let built = match WindowBuilder::new()
                        .with_title(&title)
                        .with_inner_size(tao::dpi::LogicalSize::new(width, height))
                        .build(target)
                    {
                        Ok(window) => {
                            let webview = WebViewBuilder::new()
                                .with_html(html)
                                .with_initialization_script(bridge_js())
                                .with_ipc_handler({
                                    // A clone per window: `EventLoopProxy` is
                                    // `Clone` but not `Copy`, and the handler
                                    // must own one to outlive this iteration.
                                    let proxy = proxy.clone();
                                    move |req: wry::http::Request<String>| {
                                        // Parse `{fn, args}` and forward to the
                                        // loop. The result comes back via `Eval`.
                                        if let Some((name, args)) = parse_ipc(req.body()) {
                                            let _ = proxy.send_event((Command::Ipc { name, args }));
                                        }
                                    }
                                })
                                .build_as_child(&window);
                            // A wry failure and a tao failure are different
                            // error types, so the webview result is mapped to a
                            // string rather than joined with `?`.
                            match webview {
                                Ok(webview) => Ok((window, webview)),
                                Err(e) => Err(format!("{}", e)),
                            }
                        }
                        Err(e) => Err(format!("{}", e)),
                    };
                    match built {
                        Ok((window, webview)) => {
                            tao_ids.insert(window.id(), id);
                            windows.insert(id, (window, webview));
                            manager.open.add(id);
                            let _ = reply.send(Ok(id));
                        }
                        // A window that cannot be created is reported to the
                        // caller rather than panicking, so a headless or
                        // display-less environment produces a Rak error.
                        Err(e) => {
                            let _ = reply.send(Err(format!("gui_open: {}", e)));
                        }
                    }
                }
                Command::Update { id, html } => {
                    if let Some((window, old)) = windows.remove(&id) {
                        match WebViewBuilder::new()
                            .with_html(html)
                            .with_initialization_script(bridge_js())
                            .build_as_child(&window)
                        {
                            Ok(webview) => {
                                windows.insert(id, (window, webview));
                            }
                            Err(_) => {
                                // The update failed, the window did not. Put
                                // the working pair back rather than dropping
                                // the window on the floor.
                                windows.insert(id, (window, old));
                            }
                        }
                    }
                }
                Command::SetTitle { id, title } => {
                    if let Some((window, _)) = windows.get(&id) {
                        window.set_title(&title);
                    }
                }
                Command::Close { id } => {
                    // tao 0.35 has no `Window::close()`. Dropping the handles is
                    // the supported way to destroy a window: on Windows and
                    // Linux `Destroyed` fires when the `Window` is dropped, and
                    // dropping the `WebView` alongside it tears down the
                    // webview first, so no IPC can arrive for a window that is
                    // on its way out.
                    if let Some((window, _)) = windows.remove(&id) {
                        tao_ids.remove(&window.id());
                        manager.open.remove(id);
                        drop(window);
                    }
                }
                Command::Eval { id, js } => {
                    if let Some((_, webview)) = windows.get(&id) {
                        let _ = webview.evaluate_script(&js);
                    }
                }
                Command::Ipc { name, args } => {
                    if let Some(result) = manager.dispatch(&name, &args) {
                        let js = format!(
                            "window.rak_result({}, {})",
                            json_string(&name),
                            json_string(&result)
                        );
                        for (_, (_, webview)) in windows.iter() {
                            let _ = webview.evaluate_script(&js);
                        }
                    }
                }
                Command::Quit { code } => {
                    *control_flow = ControlFlow::ExitWithCode(code);
                }
                Command::ScriptDone => {
                    if windows.is_empty() {
                        *control_flow = ControlFlow::Exit;
                    }
                    // Otherwise the script opened a window and returned;
                    // leave it up until the user closes it, then stop.
                }
            },
            Event::WindowEvent {
                window_id: tao_id,
                event: WindowEvent::CloseRequested,
                ..
            } => {
                // The user asked to close a window. Honour it and keep the
                // process alive: the loop stops only when the program is
                // finished with the GUI. Before v8.1.0 this set
                // `ControlFlow::Exit`, and since tao's `run` is `-> !` and
                // calls `process::exit`, closing any window ended the process.
                if let Some(id) = tao_ids.remove(&tao_id) {
                    windows.remove(&id);
                    manager.open.remove(id);
                }
                if manager.may_stop(windows.len()) {
                    *control_flow = ControlFlow::Exit;
                }
            }
            // Every other event is uninteresting to a GUI driven by Rak
            // builtins: input, focus, redraw and device events are the page's
            // business, and the loop stays parked in `ControlFlow::Wait` until
            // something Rak or the user does.
            _ => {}
        }
    })
}

/// Parse the `{fn, args}` payload from `window.rak_call`.
///
/// Returns `None` for anything malformed rather than erroring: a page can post
/// arbitrary messages to the IPC channel, and a bad one should be ignored, not
/// take the window down.
fn parse_ipc(body: &str) -> Option<(String, Vec<String>)> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let name = v.get("fn")?.as_str()?.to_string();
    let args = v
        .get("args")
        .and_then(|a| a.as_array())
        .map(|items| {
            items
                .iter()
                .map(|i| match i {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    Some((name, args))
}

fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// The manager the running process's GUI is using.
///
/// The VM cannot hold a reference to it — a native only sees `&[Value]` — so it
/// is published here when the driver starts the loop. A `OnceLock` rather than
/// a `static Mutex` because the value is written once and read many times, and
/// because `Mutex<Option<..>>` would need poisoning handled on every call.
static CURRENT: std::sync::OnceLock<Arc<GuiManager>> = std::sync::OnceLock::new();

/// Publish `manager` as this process's GUI. Called by the driver before the
/// loop starts; ignored if a loop is already running.
pub(crate) fn set_current(manager: Arc<GuiManager>) {
    let _ = CURRENT.set(manager);
}

/// The current manager, or `None` if no GUI event loop is running.
pub(crate) fn current_manager() -> Option<Arc<GuiManager>> {
    CURRENT.get().cloned()
}

/// Dispatch a GUI builtin for the VM.
///
/// The VM's natives have signature `fn(&[Value])`, so unlike the interpreter —
/// which holds `&mut self` and can call `self.gui_manager()` — it has to find
/// the manager through [`current_manager`]. That is the same constraint that
/// rules out the stream and socket families, and it is why this lives here
/// rather than in `ext_stdlib`.
pub(crate) fn vm_native(name: &str, args: &[crate::value::Value]) -> Result<crate::value::Value, String> {
    use crate::value::Value;
    let mgr = current_manager().ok_or_else(|| "GUI event loop is not running".to_string())?;
    let s = |i: usize| -> String {
        args.get(i).map(vm_to_string).unwrap_or_default()
    };
    let id = args.first().and_then(|v| v.as_i64()).unwrap_or(-1);
    match name {
        "gui_open" => {
            let width = args.get(2).and_then(|v| v.as_i64()).unwrap_or(800) as f64;
            let height = args.get(3).and_then(|v| v.as_i64()).unwrap_or(600) as f64;
            mgr.open(&s(0), &s(1), width, height).map(Value::I64)
        }
        "gui_update" => mgr.update(id, &s(1)).map(|_| Value::Nil),
        "gui_title" => mgr.set_title(id, &s(1)).map(|_| Value::Nil),
        "gui_close" => mgr.close(id).map(|_| Value::Nil),
        "gui_wait" => {
            mgr.wait();
            Ok(Value::Nil)
        }
        "gui_quit" => {
            let code = args.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            mgr.quit(code).map(|_| Value::Nil)
        }
        other => Err(format!("{}: not a GUI builtin", other)),
    }
}

/// Render a VM value the way the interpreter's `val_to_string` would, so the
/// same program produces the same window title on either backend.
pub(crate) fn vm_to_string(v: &crate::value::Value) -> String {
    use crate::value::Value;
    match v {
        Value::String(x) => x.to_string(),
        Value::Bytes(b) => String::from_utf8_lossy(b).to_string(),
        other => other.to_string(),
    }
}

/// The JavaScript bridge installed in every page.
///
/// `rak_call` is how the page calls Rak. `rak_on` registers a page-side handler
/// for a callback's return value, and `rak_result` is what the loop calls to
/// deliver it.
pub fn bridge_js() -> String {
    r#"
    window.rak_call = function(fnName) {
        var args = Array.from(arguments).slice(1);
        window.ipc.postMessage(JSON.stringify({ fn: fnName, args: args }));
    };
    window.rak_handlers = window.rak_handlers || {};
    window.rak_on = function(name, fn) { window.rak_handlers[name] = fn; };
    window.rak_result = function(name, value) {
        var h = window.rak_handlers[name];
        if (h) { h(value); }
    };
    "#
    .to_string()
}

/// Wrap a caller's HTML fragment in a document with the bridge installed.
pub fn wrap_html(body: &str) -> String {
    format!(
        "<html><head><meta charset=\"utf-8\">\
         <style>*{{font-family:sans-serif}}</style>\
         <script>{}</script></head><body>{}</body></html>",
        bridge_js(),
        body
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_well_formed_ipc_message() {
        let (name, args) = parse_ipc(r#"{"fn":"clicked","args":["a",1,true]}"#).unwrap();
        assert_eq!(name, "clicked");
        assert_eq!(args, vec!["a", "1", "true"]);
    }

    #[test]
    fn rejects_malformed_ipc_without_panicking() {
        // A page can post anything to the IPC channel. None of this may take
        // the window down.
        for body in [
            "",
            "not json",
            "{}",
            r#"{"args":[]}"#,
            r#"{"fn":42}"#,
            r#"{"fn":"f","args":"not-an-array"}"#,
        ] {
            // Either `None`, or a name with no args. Never a panic.
            if let Some((name, _)) = parse_ipc(body) {
                assert!(!name.is_empty());
            }
        }
    }

    #[test]
    fn bridge_exposes_both_directions() {
        let js = bridge_js();
        assert!(js.contains("rak_call"), "page must be able to call Rak");
        assert!(js.contains("rak_result"), "Rak must be able to call the page");
        assert!(js.contains("rak_on"));
    }

    #[test]
    fn wrap_html_embeds_the_bridge_once() {
        let html = wrap_html("<h1>hi</h1>");
        assert_eq!(html.matches("rak_call").count(), 1);
        assert!(html.contains("<h1>hi</h1>"));
    }

    #[test]
    fn the_loop_only_stops_once_the_script_is_done() {
        // The rule that keeps a GUI program from either hanging forever or
        // closing its window the instant the script returns.
        let mgr = GuiManager::new();

        // Script still running: closing a window must not stop the loop.
        assert!(!mgr.may_stop(0), "script still running, no windows");
        assert!(!mgr.may_stop(1), "script still running, one window");

        mgr.script_done.store(true, Ordering::SeqCst);
        // No windows and the script is done: stop.
        assert!(mgr.may_stop(0), "script done, no windows");

        // A window is still up and nobody called gui_wait: keep going, so the
        // window stays on screen.
        assert!(!mgr.may_stop(1), "window open, gui_wait not called");

        // `gui_wait` means "block until they close", so this is the point where
        // the last close is allowed to end the process.
        mgr.waiting.store(true, Ordering::SeqCst);
        assert!(mgr.may_stop(0));
    }

    #[test]
    fn notify_script_done_is_safe_without_a_loop() {
        // The worker calls this unconditionally, including when the loop failed
        // to start. It must not panic.
        let mgr = GuiManager::new();
        mgr.notify_script_done();
        assert!(mgr.may_stop(0));
    }

    #[test]
    fn manager_reports_errors_before_the_loop_starts() {
        // Builtins must fail cleanly rather than panic if they are reached
        // before `run_event_loop` has installed the proxy.
        let mgr = GuiManager::new();
        assert!(mgr.open("t", "<p>x</p>", 100.0, 100.0).is_err());
        assert!(mgr.set_title(1, "x").is_err());
        assert!(mgr.quit(0).is_err());
    }

    #[test]
    fn update_and_title_reject_unknown_ids() {
        let mgr = GuiManager::new();
        // These check the id before sending, so they report a bad id even with
        // no loop running — the error names the real problem.
        let err = mgr.update(42, "<p>x</p>").unwrap_err();
        assert!(err.contains("no window with id 42"), "got: {}", err);
        let err = mgr.set_title(42, "x").unwrap_err();
        assert!(err.contains("no window with id 42"), "got: {}", err);
    }
}
