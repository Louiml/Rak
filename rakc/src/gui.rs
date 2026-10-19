//! Native GUI for Rak, built on egui (glow backend) -- no webview.
//!
//! Programs create windows and widgets; the Rust side maintains a retained
//! widget tree and renders it with egui each frame. The interpreter runs on a
//! worker thread; the main thread owns the event loop. Commands travel over a
//! channel from the interpreter to the UI thread, and callbacks are dispatched
//! back with the environment captured at registration time.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::interpreter::{Env, Interpreter, Value};

pub type LoopEvent = Command;

pub enum Command {
    Open {
        reply: std::sync::mpsc::Sender<std::result::Result<i64, String>>,
        title: String,
        width: f64,
        height: f64,
    },
    AddWidget {
        window: i64,
        kind: String,
        text: String,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        reply: std::sync::mpsc::Sender<std::result::Result<i64, String>>,
    },
    SetText {
        widget: i64,
        text: String,
    },
    GetText {
        widget: i64,
        reply: std::sync::mpsc::Sender<std::result::Result<String, String>>,
    },
    SetVisible {
        widget: i64,
        visible: bool,
    },
    OnClick {
        widget: i64,
        name: String,
    },
    Close {
        id: i64,
    },
    Quit {
        code: i32,
    },
    ScriptDone,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WidgetKind {
    Label,
    Button,
    Input,
    Checkbox,
    Select,
    Textarea,
}

impl WidgetKind {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "label" => Some(Self::Label),
            "button" => Some(Self::Button),
            "input" => Some(Self::Input),
            "checkbox" => Some(Self::Checkbox),
            "select" => Some(Self::Select),
            "textarea" => Some(Self::Textarea),
            _ => None,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Label => "label",
            Self::Button => "button",
            Self::Input => "input",
            Self::Checkbox => "checkbox",
            Self::Select => "select",
            Self::Textarea => "textarea",
        }
    }
}

#[derive(Clone)]
pub struct Widget {
    pub id: i64,
    pub kind: WidgetKind,
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub visible: bool,
    pub on_click: Option<String>,
    pub input_value: String,
    pub checked: bool,
    pub options: Vec<String>,
}

pub struct WindowState {
    pub id: i64,
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub widgets: Vec<Widget>,
    pub open: bool,
}

pub struct GuiManager {
    proxy: Mutex<Option<std::sync::mpsc::Sender<Command>>>,
    open: Arc<OpenWindows>,
    callbacks: Arc<Mutex<HashMap<String, Callback>>>,
    /// Callbacks registered from bytecode-compiled scripts. The interpreter's
    /// `Callback` holds a function value and a snapshot of the interpreter
    /// environment; a VM closure has neither, so the two registries are
    /// separate and `dispatch` tries each in turn.
    vm_callbacks: Arc<Mutex<HashMap<String, VmCallback>>>,
    waiting: Arc<AtomicBool>,
    script_done: Arc<AtomicBool>,
    opened_once: Arc<AtomicBool>,
}

/// A callback whose body lives in a bytecode chunk: the closure value, plus the
/// chunk globals it was registered with. Dispatch runs it on a fresh machine
/// seeded with those globals, because a VM native has no machine of its own.
#[derive(Clone)]
struct VmCallback {
    func: crate::value::Value,
    globals: HashMap<String, crate::value::Value>,
}

struct OpenWindows {
    ids: Mutex<Vec<i64>>,
    changed: Condvar,
}

#[derive(Clone)]
struct Callback {
    func: Value,
    env: Arc<Env>,
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
            vm_callbacks: Arc::new(Mutex::new(HashMap::new())),
            waiting: Arc::new(AtomicBool::new(false)),
            script_done: Arc::new(AtomicBool::new(false)),
            opened_once: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Whether the script has asked for a window yet.
    ///
    /// The event loop is not started until this is true, so a script that never
    /// touches the GUI never pays for a window system.
    pub fn ever_opened(&self) -> bool {
        self.opened_once.load(Ordering::SeqCst)
    }

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

    fn send(&self, cmd: Command) -> std::result::Result<(), String> {
        let guard = self.proxy.lock().unwrap();
        match guard.as_ref() {
            Some(tx) => tx
                .send(cmd)
                .map_err(|_| "GUI event loop is no longer running".to_string()),
            None => Err("GUI event loop is not running".to_string()),
        }
    }

    pub fn open(&self, title: &str, width: f64, height: f64) -> std::result::Result<i64, String> {
        // Set before the send, because the reply below blocks until the event
        // loop is running, and the event loop waits for this flag.
        self.opened_once.store(true, Ordering::SeqCst);
        let (tx, rx) = std::sync::mpsc::channel();
        self.send(Command::Open {
            reply: tx,
            title: title.to_string(),
            width,
            height,
        })?;
        rx.recv()
            .map_err(|_| "GUI event loop closed before the window opened".to_string())?
    }

    pub fn add_widget(
        &self,
        window: i64,
        kind: &str,
        text: &str,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    ) -> std::result::Result<i64, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.send(Command::AddWidget {
            window,
            kind: kind.to_string(),
            text: text.to_string(),
            x,
            y,
            w,
            h,
            reply: tx,
        })?;
        rx.recv()
            .map_err(|_| "GUI event loop closed before the widget was added".to_string())?
    }

    pub fn set_text(&self, widget: i64, text: &str) -> std::result::Result<(), String> {
        self.send(Command::SetText {
            widget,
            text: text.to_string(),
        })
    }

    pub fn get_text(&self, widget: i64) -> std::result::Result<String, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.send(Command::GetText { widget, reply: tx })?;
        rx.recv()
            .map_err(|_| "GUI event loop closed before the widget replied".to_string())?
    }

    pub fn set_visible(&self, widget: i64, visible: bool) -> std::result::Result<(), String> {
        self.send(Command::SetVisible { widget, visible })
    }

    pub fn on_click(&self, widget: i64, name: &str) -> std::result::Result<(), String> {
        self.send(Command::OnClick {
            widget,
            name: name.to_string(),
        })
    }

    pub fn close(&self, id: i64) -> std::result::Result<(), String> {
        self.send(Command::Close { id })
    }

    /// Block until every window the script opened has been closed.
    pub fn wait(&self) {
        self.waiting.store(true, Ordering::SeqCst);
        let mut ids = self.open.ids.lock().unwrap();
        while !ids.is_empty() {
            ids = self.open.changed.wait(ids).unwrap();
        }
        self.waiting.store(false, Ordering::SeqCst);
    }

    pub fn quit(&self, code: i32) -> std::result::Result<(), String> {
        self.send(Command::Quit { code })
    }

    pub fn notify_script_done(&self) {
        self.script_done.store(true, Ordering::SeqCst);
        let _ = self.send(Command::ScriptDone);
    }

    /// The loop stops once the script is done and nothing is left driving it.
    ///
    /// `script_done` is checked first on purpose. Before it is set the loop must
    /// keep running even with zero windows, because the interpreter may be about
    /// to send the first `gui_window` -- stopping on "no windows" alone would
    /// race the script's startup and close the window as it opened.
    fn may_stop(&self, open_windows: usize) -> bool {
        if !self.script_done.load(Ordering::SeqCst) {
            return false;
        }
        // A script blocked in `gui_wait` has not finished yet; it only returns
        // once every window is gone.
        !self.waiting.load(Ordering::SeqCst) || open_windows == 0
    }

    fn dispatch(&self, name: &str, args: &[String]) -> Option<String> {
        // Interpreter callbacks carry their own environment snapshot.
        if let Some(cb) = self.callbacks.lock().unwrap().get(name).cloned() {
            return crate::interpreter::call_in_env(
                cb.func.clone(),
                (*cb.env).clone(),
                args.iter().map(|a| Value::String(a.clone())).collect(),
            )
            .ok()
            .map(|v| v.to_string());
        }
        // A bytecode-compiled script's callback runs on a fresh machine seeded
        // with the globals it was registered with. The event-loop thread owns
        // no machine, so one is created per dispatch - a click handler is not
        // hot code.
        let cb = self.vm_callbacks.lock().unwrap().get(name).cloned()?;
        crate::vm::run_closure_standalone(&cb.func, cb.globals).map(|v| v.to_string())
    }
}

impl OpenWindows {
    fn add(&self, id: i64) {
        self.ids.lock().unwrap().push(id);
        self.changed.notify_all();
    }
    fn remove(&self, id: i64) {
        self.ids.lock().unwrap().retain(|x| *x != id);
        self.changed.notify_all();
    }
    fn count(&self) -> usize {
        self.ids.lock().unwrap().len()
    }
}

pub fn eval_in_cli_with_gui(
    source: &str,
    base_dir: &str,
    argv: &[String],
    max_depth: Option<u32>,
) -> crate::Result<(Vec<String>, i32)> {
    let manager = Arc::new(GuiManager::new());
    set_current(manager.clone());
    let src = source.to_string();
    let dir = base_dir.to_string();
    let args = argv.to_vec();
    let for_worker = manager.clone();

    let script = std::thread::Builder::new()
        .name("rak-interp".to_string())
        .stack_size(crate::INTERPRETER_STACK)
        .spawn(move || {
            let mut interpreter = Interpreter::with_base_dir(dir);
            interpreter.attach_gui(for_worker.clone());
            // Same setup as `eval_in_cli`: without these the program loses
            // `argv`, and `--max-depth` is silently ignored.
            interpreter.set_program_argv(&args);
            interpreter.set_max_depth(max_depth.unwrap_or(crate::interpreter::DEFAULT_MAX_DEPTH));
            let result = (|| -> crate::Result<(Vec<String>, i32)> {
                // The output snapshot must be taken after `run_main`, not before.
                interpreter.run_source(&src)?;
                let code = interpreter.run_main(&args)?;
                Ok((interpreter.take_output(), code))
            })();
            // Run whether or not the program succeeded, so the event loop always
            // learns that nothing more is coming.
            for_worker.notify_script_done();
            result
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

    // Start the window system only once the script actually asks for a window.
    //
    // Every `rakc run` goes through here when the feature is on, so opening an
    // eframe viewport for a script that never draws anything is pure overhead --
    // enough to push the long examples and the benchmarks past their timeouts.
    let gui_code = loop {
        if manager.ever_opened() {
            break run_event_loop(manager);
        }
        if script.is_finished() {
            break 0;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    };

    let joined: crate::Result<(Vec<String>, i32)> = match script.join() {
        Ok(result) => result,
        Err(_) => Ok((Vec::new(), gui_code)),
    };
    match joined {
        Ok((output, code)) => {
            let code = if code != 0 { code } else { gui_code };
            Ok((output, code))
        }
        Err(e) => Err(e),
    }
}

pub fn run_event_loop(manager: Arc<GuiManager>) -> i32 {
    // The script finished before we got here and never opened a window, so
    // there is nothing to draw. Skipping eframe keeps a plain `rakc run` from
    // flashing an empty window on its way out.
    if manager.may_stop(0) {
        return 0;
    }

    let state = Arc::new(Mutex::new(AppState {
        windows: HashMap::new(),
        next_widget_id: 1,
        next_window_id: 1,
        manager: manager.clone(),
        quit_code: None,
    }));

    let (tx, rx) = std::sync::mpsc::channel::<Command>();
    *manager.proxy.lock().unwrap() = Some(tx.clone());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("Rak GUI"),
        ..Default::default()
    };

    let app_state = state.clone();
    let creator: eframe::AppCreator<'static> =
        Box::new(move |_cc| -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>>
        {
            Ok(Box::new(RakApp {
                state: app_state.clone(),
                rx,
            }))
        });

    if eframe::run_native("Rak GUI", options, creator).is_err() {
        return 1;
    }
    let quit_code = state.lock().unwrap().quit_code;
    quit_code.unwrap_or(0)
}

struct AppState {
    windows: HashMap<i64, WindowState>,
    next_widget_id: i64,
    next_window_id: i64,
    manager: Arc<GuiManager>,
    quit_code: Option<i32>,
}

struct RakApp {
    state: Arc<Mutex<AppState>>,
    rx: std::sync::mpsc::Receiver<Command>,
}

impl eframe::App for RakApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Drain commands from the interpreter thread.
        while let Ok(cmd) = self.rx.try_recv() {
            let mut state = self.state.lock().unwrap();
            match cmd {
                Command::Open {
                    reply,
                    title,
                    width,
                    height,
                } => {
                    let id = state.next_window_id;
                    state.next_window_id += 1;
                    state.windows.insert(
                        id,
                        WindowState {
                            id,
                            title,
                            width,
                            height,
                            widgets: Vec::new(),
                            open: true,
                        },
                    );
                    state.manager.open.add(id);
                    let _ = reply.send(Ok(id));
                }
                Command::AddWidget {
                    window,
                    kind,
                    text,
                    x,
                    y,
                    w,
                    h,
                    reply,
                } => {
                    let kind = WidgetKind::from_str(&kind).unwrap_or(WidgetKind::Label);
                    let id = state.next_widget_id;
                    state.next_widget_id += 1;
                    let options = if kind == WidgetKind::Select {
                        text.split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    };
                    let text = if kind == WidgetKind::Select {
                        options.first().cloned().unwrap_or_default()
                    } else {
                        text
                    };
                    let input_value = if kind == WidgetKind::Input || kind == WidgetKind::Textarea {
                        text.clone()
                    } else {
                        String::new()
                    };
                    if let Some(win) = state.windows.get_mut(&window) {
                        win.widgets.push(Widget {
                            id,
                            kind,
                            text,
                            x,
                            y,
                            w,
                            h,
                            visible: true,
                            on_click: None,
                            input_value,
                            checked: false,
                            options,
                        });
                    }
                    let _ = reply.send(Ok(id));
                }
                Command::SetText { widget, text } => {
                    for win in state.windows.values_mut() {
                        for w in win.widgets.iter_mut() {
                            if w.id == widget {
                                w.text = text.clone();
                                if w.kind == WidgetKind::Input || w.kind == WidgetKind::Textarea {
                                    w.input_value = text.clone();
                                }
                            }
                        }
                    }
                }
                Command::GetText { widget, reply } => {
                    let mut found = None;
                    for win in state.windows.values() {
                        for w in win.widgets.iter() {
                            if w.id == widget {
                                found = Some(match w.kind {
                                    WidgetKind::Input | WidgetKind::Textarea => {
                                        w.input_value.clone()
                                    }
                                    WidgetKind::Checkbox => w.checked.to_string(),
                                    _ => w.text.clone(),
                                });
                            }
                        }
                    }
                    let _ = reply.send(match found {
                        Some(t) => Ok(t),
                        None => Err(format!("no widget with id {}", widget)),
                    });
                }
                Command::SetVisible { widget, visible } => {
                    for win in state.windows.values_mut() {
                        for w in win.widgets.iter_mut() {
                            if w.id == widget {
                                w.visible = visible;
                            }
                        }
                    }
                }
                Command::OnClick { widget, name } => {
                    for win in state.windows.values_mut() {
                        for w in win.widgets.iter_mut() {
                            if w.id == widget {
                                w.on_click = Some(name.clone());
                            }
                        }
                    }
                }
                Command::Close { id } => {
                    if let Some(win) = state.windows.get_mut(&id) {
                        win.open = false;
                    }
                    state.manager.open.remove(id);
                }
                Command::Quit { code } => {
                    state.quit_code = Some(code);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Command::ScriptDone => {}
            }
        }

        // Render.
        let mut state = self.state.lock().unwrap();
        let mgr = state.manager.clone();
        let ids: Vec<i64> = state.windows.keys().copied().collect();

        for id in ids {
            let (title, width, height) = match state.windows.get(&id) {
                Some(win) => (win.title.clone(), win.width, win.height),
                None => continue,
            };
            let mut open = state.windows.get(&id).map(|w| w.open).unwrap_or(false);
            let mut clicked: Option<String> = None;

            egui::Window::new(title)
                .default_size([width as f32, height as f32])
                .open(&mut open)
                .show(ctx, |ui| {
                    let canvas = ui.max_rect();
                    let win = match state.windows.get_mut(&id) {
                        Some(w) => w,
                        None => return,
                    };
                    for w in win.widgets.iter_mut() {
                        if !w.visible {
                            continue;
                        }
                        let rect = egui::Rect::from_min_size(
                            canvas.min + egui::vec2(w.x as f32, w.y as f32),
                            egui::vec2(w.w as f32, w.h as f32),
                        );
                        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                            match w.kind {
                                WidgetKind::Label => {
                                    ui.label(&w.text);
                                }
                                WidgetKind::Button => {
                                    if ui.button(&w.text).clicked() {
                                        clicked = w.on_click.clone();
                                    }
                                }
                                WidgetKind::Input => {
                                    ui.add_sized(
                                        [w.w as f32, w.h as f32],
                                        egui::TextEdit::singleline(&mut w.input_value),
                                    );
                                }
                                WidgetKind::Checkbox => {
                                    ui.checkbox(&mut w.checked, &w.text);
                                }
                                WidgetKind::Select => {
                                    let opts = w.options.clone();
                                    if opts.is_empty() {
                                        ui.label(&w.text);
                                    } else {
                                        let mut sel =
                                            opts.iter().position(|o| *o == w.text).unwrap_or(0);
                                        egui::ComboBox::from_id_salt(w.id)
                                            .selected_text(w.text.clone())
                                            .show_ui(ui, |ui| {
                                                for (i, o) in opts.iter().enumerate() {
                                                    ui.selectable_value(&mut sel, i, o.clone());
                                                }
                                            });
                                        if let Some(o) = opts.get(sel) {
                                            w.text = o.clone();
                                        }
                                    }
                                }
                                WidgetKind::Textarea => {
                                    ui.add_sized(
                                        [w.w as f32, w.h as f32],
                                        egui::TextEdit::multiline(&mut w.input_value),
                                    );
                                }
                            }
                        });
                    }
                });

            if let Some(name) = clicked {
                let _ = mgr.dispatch(&name, &[]);
            }
            if !open || !state.windows.get(&id).map(|w| w.open).unwrap_or(false) {
                if let Some(win) = state.windows.get_mut(&id) {
                    win.open = false;
                }
                mgr.open.remove(id);
            }
        }

        if state.quit_code.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let open_count = state.windows.values().filter(|w| w.open).count();
        if mgr.may_stop(open_count) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

static CURRENT: std::sync::OnceLock<Arc<GuiManager>> = std::sync::OnceLock::new();

pub(crate) fn set_current(manager: Arc<GuiManager>) {
    let _ = CURRENT.set(manager);
}

pub(crate) fn current_manager() -> Option<Arc<GuiManager>> {
    CURRENT.get().cloned()
}

/// Register a callback whose body is a bytecode-compiled closure.
///
/// Called from `Vm::call_value`'s `gui_callback` interception: a native cannot
/// reach the machine, and dispatching later needs the closure *and* the chunk
/// globals it was registered with, so both are handed over here. If the GUI
/// was never started the registration is a no-op, matching the interpreter's
/// `gui_callback`, which stores into a manager nobody dispatches from.
pub fn register_vm_callback(
    name: &str,
    func: crate::value::Value,
    globals: HashMap<String, crate::value::Value>,
) {
    if let Some(mgr) = current_manager() {
        mgr.vm_callbacks
            .lock()
            .unwrap()
            .insert(name.to_string(), VmCallback { func, globals });
    }
}

pub(crate) fn vm_native(
    name: &str,
    args: &[crate::value::Value],
) -> Result<crate::value::Value, String> {
    use crate::value::Value;
    let mgr = current_manager().ok_or_else(|| "GUI event loop is not running".to_string())?;
    let s = |i: usize| -> String { args.get(i).map(vm_to_string).unwrap_or_default() };
    match name {
        "gui_window" => {
            let title = s(0);
            let width = args.get(1).and_then(|v| v.as_i64()).unwrap_or(400) as f64;
            let height = args.get(2).and_then(|v| v.as_i64()).unwrap_or(300) as f64;
            mgr.open(&title, width, height).map(Value::I64)
        }
        "gui_add" => {
            let window = args.get(0).and_then(|v| v.as_i64()).unwrap_or(-1);
            let kind = s(1);
            let text = s(2);
            let x = args.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as f64;
            let y = args.get(4).and_then(|v| v.as_i64()).unwrap_or(0) as f64;
            let w = args.get(5).and_then(|v| v.as_i64()).unwrap_or(120) as f64;
            let h = args.get(6).and_then(|v| v.as_i64()).unwrap_or(28) as f64;
            mgr.add_widget(window, &kind, &text, x, y, w, h)
                .map(Value::I64)
        }
        "gui_set_text" => {
            let widget = args.get(0).and_then(|v| v.as_i64()).unwrap_or(-1);
            let text = s(1);
            mgr.set_text(widget, &text).map(|_| Value::Nil)
        }
        "gui_get_text" => {
            let widget = args.get(0).and_then(|v| v.as_i64()).unwrap_or(-1);
            let text = mgr.get_text(widget)?;
            Ok(Value::String(text.into()))
        }
        "gui_set_visible" => {
            let widget = args.get(0).and_then(|v| v.as_i64()).unwrap_or(-1);
            let visible = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
            mgr.set_visible(widget, visible).map(|_| Value::Nil)
        }
        "gui_on_click" => {
            let widget = args.get(0).and_then(|v| v.as_i64()).unwrap_or(-1);
            let name = s(1);
            mgr.on_click(widget, &name).map(|_| Value::Nil)
        }
        "gui_close" => {
            let id = args.first().and_then(|v| v.as_i64()).unwrap_or(-1);
            mgr.close(id).map(|_| Value::Nil)
        }
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

pub(crate) fn vm_to_string(v: &crate::value::Value) -> String {
    use crate::value::Value;
    match v {
        Value::String(x) => x.to_string(),
        Value::Bytes(b) => String::from_utf8_lossy(b).to_string(),
        other => other.to_string(),
    }
}