use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct GuiManager {
    windows: Arc<Mutex<HashMap<i64, ()>>>,
    next_id: Arc<Mutex<i64>>,
    callbacks: Arc<Mutex<HashMap<String, String>>>,
}

impl GuiManager {
    pub fn new() -> Self {
        GuiManager {
            windows: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
            callbacks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn open(&self, title: &str, html: &str, width: u32, height: u32) -> i64 {
        let mut id_lock = self.next_id.lock().unwrap();
        let id = *id_lock;
        *id_lock += 1;
        drop(id_lock);

        let inject_js = self.build_bridge_js();
        let full_html = format!(
            "<html><head><meta charset=\"utf-8\"><style>*{{font-family:sans-serif}}</style>\
             <script>{}</script></head><body>{}</body></html>",
            inject_js, html
        );

        let win_id = id;
        let win_title = title.to_string();
        let win_html = full_html.clone();
        let windows_clone = self.windows.clone();

        std::thread::spawn(move || {
            use tao::event::{Event, WindowEvent};
            use tao::event_loop::{ControlFlow, EventLoopBuilder};
            use tao::platform::windows::EventLoopBuilderExtWindows;
            use tao::window::WindowBuilder;
            use wry::WebViewBuilder;

            let mut builder = EventLoopBuilder::<()>::new();
            builder.with_any_thread(true);
            let event_loop = builder.build();
            let window = WindowBuilder::new()
                .with_title(win_title.clone())
                .with_inner_size(tao::dpi::LogicalSize::new(width, height))
                .build(&event_loop)
                .expect("Failed to create window");

            let webview = WebViewBuilder::new()
                .with_html(win_html.clone())
                .with_ipc_handler(move |request: wry::http::Request<String>| {
                    let _body = request.body();
                    // JS -> Rak bridge: messages arrive in request body
                })
                .build(&window)
                .expect("Failed to create webview");

            let _ = webview;

            event_loop.run(move |event, _, control_flow| {
                *control_flow = ControlFlow::Wait;
                match event {
                    Event::WindowEvent {
                        event: WindowEvent::CloseRequested,
                        ..
                    } => {
                        let mut wins = windows_clone.lock().unwrap();
                        wins.remove(&win_id);
                        *control_flow = ControlFlow::Exit;
                    }
                    _ => {}
                }
            });
        });

        self.windows.lock().unwrap().insert(id, ());
        id
    }

    pub fn update(&self, id: i64, html: &str) {
        let wins = self.windows.lock().unwrap();
        if wins.contains_key(&id) {
            let inject_js = self.build_bridge_js();
            let full_html = format!(
                "<html><head><meta charset=\"utf-8\"><style>*{{font-family:sans-serif}}</style>\
                 <script>{}</script></head><body>{}</body></html>",
                inject_js, html
            );
            let _ = full_html;
        }
    }

    pub fn set_title(&self, _id: i64, _title: &str) {
        // Title updates require access to the window handle; deferred for now
    }

    pub fn close(&self, id: i64) {
        self.windows.lock().unwrap().remove(&id);
    }

    pub fn wait(&self) {
        loop {
            let wins = self.windows.lock().unwrap();
            if wins.is_empty() {
                break;
            }
            drop(wins);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    pub fn register_callback(&self, name: &str) {
        self.callbacks.lock().unwrap().insert(name.to_string(), name.to_string());
    }

    fn build_bridge_js(&self) -> String {
        r#"
        window.rak_call = function(fnName) {
            var args = Array.from(arguments).slice(1);
            var msg = JSON.stringify({ fn: fnName, args: args });
            // Use postMessage for IPC (wry's default channel)
            window.ipc.postMessage(msg);
        };
        "#.to_string()
    }
}
