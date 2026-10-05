//! The native installer window.
//!
//! Pure Rust: `egui` draws with `glow`, so there is no HTML, no JS and no
//! WebView runtime to ship or to break. The window is the default when
//! `rak-setup` is run with no arguments; every flag still reaches the headless
//! paths, which is what CI and scripts use.
//!
//! All real work happens on a worker thread. The installer's global `status()`
//! and progress sinks forward into a channel that `update` drains each frame, so
//! the log pane and progress bar show the same information the CLI prints. There
//! is no second implementation of the install logic to drift out of sync.
//!
//! The state machine lives in [`Feed`] rather than in [`App`] so it can be tested
//! without a window or a GL context.

use anyhow::{anyhow, Result};
use eframe::egui;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use crate::update;
use crate::wizard;
use crate::{Component, Config, IdeMode, Scope};

/// The Rak logo, embedded at compile time: a 512px RGBA PNG.
///
/// The `.ico` beside it in `assets/` is for the shortcut and Start-menu entries
/// the installer writes, not for the window.
const LOGO_PNG: &[u8] = include_bytes!("../assets/rak-logo.png");

/// How many log lines to keep.
///
/// A long install can produce thousands; without a cap the window holds every
/// one for the life of the process.
const LOG_CAPACITY: usize = 2000;

/// Everything the UI needs to hear from its worker threads.
enum Msg {
    /// `(bytes_so_far, total_if_known)`
    Progress(u64, Option<u64>),
    /// An install or uninstall finished. `Err` carries the rendered error.
    Finished(std::result::Result<i32, String>),
    UpdateChecked(Vec<update::ReleaseRow>),
}

/// How a log line is coloured.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Plain,
    Good,
    Bad,
    Note,
}

struct Line {
    kind: Kind,
    text: String,
}

/// The window's log and progress state: everything the bottom pane draws.
///
/// Kept free of `egui` so it can be tested directly. `apply` is the whole state
/// machine -- every worker message goes through it, which is what makes it worth
/// testing.
#[derive(Default)]
struct Feed {
    log: VecDeque<Line>,
    autoscroll: bool,
    /// `(fraction, label)`. `fraction` is meaningless for an indeterminate
    /// download and the caller renders it as a moving bar.
    progress: Option<(f32, String)>,
}

impl Feed {
    fn push(&mut self, kind: Kind, text: impl Into<String>) {
        self.log.push_back(Line {
            kind,
            text: text.into(),
        });
        while self.log.len() > LOG_CAPACITY {
            self.log.pop_front();
        }
        self.autoscroll = true;
    }

    fn note(&mut self, text: impl Into<String>) {
        self.push(Kind::Note, text);
    }

    fn clear(&mut self) {
        self.log.clear();
        self.autoscroll = true;
    }

    fn start(&mut self, what: &str) {
        self.clear();
        self.progress = None;
        self.note(format!("Starting {}.", what));
    }

    /// Fold a worker message in. Returns true when it finished an operation, so
    /// the caller can re-read the manifest.
    fn apply(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Progress(done, total) => {
                let (frac, label) = match total {
                    Some(t) if t > 0 => (
                        (done as f64 / t as f64).clamp(0.0, 1.0) as f32,
                        format!("{} / {}", human_bytes(done), human_bytes(t)),
                    ),
                    // No Content-Length: report the bytes and let the caller
                    // show an indeterminate bar, rather than inventing a
                    // percentage that is not true.
                    _ => (0.5, format!("{} downloaded", human_bytes(done))),
                };
                self.progress = Some((frac, label));
                false
            }
            Msg::Finished(Ok(0)) => {
                self.progress = None;
                self.push(Kind::Good, "done.");
                true
            }
            Msg::Finished(Ok(code)) => {
                self.progress = None;
                self.push(Kind::Bad, format!("finished with exit code {}", code));
                true
            }
            Msg::Finished(Err(e)) => {
                self.progress = None;
                self.push(Kind::Bad, e);
                true
            }
            Msg::UpdateChecked(rows) => {
                for r in &rows {
                    match &r.error {
                        Some(e) => self.push(Kind::Bad, format!("{}: {}", r.repo, e)),
                        None if r.newer => self.note(format!("{} has {} (newer)", r.repo, r.tag)),
                        None => {
                            self.push(Kind::Plain, format!("{} is up to date ({})", r.repo, r.tag))
                        }
                    }
                }
                false
            }
        }
    }
}

struct App {
    /// One checkbox per entry in `Component::all()`, same order.
    selected: Vec<bool>,
    scope: Scope,
    ide_mode: IdeMode,
    offline: bool,
    offline_path: String,

    feed: Feed,
    busy: bool,
    checking: bool,
    releases: Vec<update::ReleaseRow>,
    installed: Option<String>,
    logo: Option<egui::TextureHandle>,

    log_rx: Receiver<String>,
    msg_rx: Receiver<Msg>,
    tx: Sender<Msg>,
}

/// Open the window.
pub fn run() -> Result<()> {
    let logo = decode_logo();

    // egui wants raw RGBA for the window icon. 512x512 is a multiple of 4, which
    // is what `IconData` requires.
    let icon = logo.as_ref().map(|img| {
        Arc::new(egui::IconData {
            rgba: img.as_raw().clone(),
            width: img.width(),
            height: img.height(),
        })
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Rak installer {}", env!("CARGO_PKG_VERSION")))
            .with_inner_size([760.0, 660.0])
            .with_min_inner_size([600.0, 520.0])
            .with_icon(icon.unwrap_or_default()),
        ..Default::default()
    };

    // Console subsystem, so the CLI paths can still print. Hide the window
    // rather than switching the subsystem, which would silence `--help`,
    // `--list` and the `--yes` output that CI reads.
    hide_console();

    match eframe::run_native(
        "rak-setup",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(App::new(cc)))
        }),
    ) {
        Ok(()) => Ok(()),
        Err(e) => {
            // The console is hidden by now, so stderr goes somewhere the user
            // will never see. A message box is the channel left.
            show_error(&format!(
                "The installer window could not be opened.\n\n{}",
                e
            ));
            Err(anyhow!("GUI window failed: {}", e))
        }
    }
}

/// Hide this process's console window, if it has one.
#[cfg(windows)]
fn hide_console() {
    use windows_sys::Win32::System::Console::GetConsoleWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    // SAFETY: `GetConsoleWindow` takes no arguments and returns a borrowed
    // handle, and `ShowWindow` is being used on this process's own console.
    // A null handle means the process has no console at all -- launched from
    // Explorer, say -- and is skipped rather than passed a null window.
    unsafe {
        let hwnd = GetConsoleWindow();
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_HIDE);
        }
    }
}

#[cfg(not(windows))]
fn hide_console() {}

/// Show a native error box.
#[cfg(windows)]
fn show_error(text: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK};

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(Some(0))
            .collect()
    }

    // SAFETY: both strings are NUL-terminated by `wide`, and the Vecs holding
    // them live until after the call returns. MB_OK | MB_ICONERROR is a plain
    // OK dialog with an error glyph.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(text).as_ptr(),
            wide("Rak installer").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn show_error(text: &str) {
    eprintln!("{}", text);
}

fn decode_logo() -> Option<image::RgbaImage> {
    image::load_from_memory(LOGO_PNG)
        .ok()
        .map(|img| img.to_rgba8())
}

/// Format a byte count the way a person would read it.
fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < UNITS.len() {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", n, UNITS[i])
    } else {
        format!("{:.1} {}", v, UNITS[i])
    }
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, msg_rx) = mpsc::channel::<Msg>();
        let (log_tx, log_rx) = mpsc::channel::<String>();

        // Hand the installer's global sinks to this window. The log sink is a
        // `Sender<String>` so `main.rs` does not have to know about `Msg`.
        let progress_tx = tx.clone();
        crate::attach_sinks(
            log_tx,
            Arc::new(move |done, total| {
                let _ = progress_tx.send(Msg::Progress(done, total));
            }),
        );

        let logo = decode_logo().map(|img| {
            let (w, h) = (img.width(), img.height());
            let pixels =
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
            cc.egui_ctx
                .load_texture("rak-logo", pixels, egui::TextureOptions::LINEAR)
        });

        let mut app = App {
            selected: vec![true; Component::all().len()],
            scope: Scope::User,
            ide_mode: IdeMode::Portable,
            offline: false,
            offline_path: String::new(),
            feed: Feed::default(),
            busy: false,
            checking: false,
            releases: Vec::new(),
            installed: None,
            logo,
            log_rx,
            msg_rx,
            tx,
        };
        app.refresh_install();
        app
    }

    /// Non-blocking drain of both channels.
    fn drain(&mut self) {
        while let Ok(line) = self.log_rx.try_recv() {
            self.feed.push(Kind::Plain, line);
        }
        while let Ok(msg) = self.msg_rx.try_recv() {
            let finished = self.feed.apply(msg);
            if finished {
                self.busy = false;
                self.refresh_install();
            }
        }
    }

    fn refresh_install(&mut self) {
        self.installed = match crate::manifest::Manifest::load(self.scope) {
            Ok(Some(m)) => Some(format!(
                "Installed: rak v{} — {} recorded actions, bin at {}",
                m.version,
                m.actions.len(),
                m.bin_dir.display()
            )),
            Ok(None) => None,
            Err(e) => Some(format!("Install manifest unreadable: {:#}", e)),
        };
    }

    fn chosen(&self) -> Vec<Component> {
        Component::all()
            .iter()
            .copied()
            .zip(self.selected.iter())
            .filter(|(_, on)| **on)
            .map(|(c, _)| c)
            .collect()
    }

    /// Build the same `Config` the CLI would build for these choices, so the
    /// GUI and `--yes` install the same thing in the same places.
    fn build_config(&mut self) -> Option<Config> {
        let comps = self.chosen();
        if comps.is_empty() {
            self.feed.push(Kind::Bad, "Select at least one component.");
            return None;
        }
        let offline = self.offline_path.trim();
        let bundle = if self.offline && !offline.is_empty() {
            Some(PathBuf::from(offline))
        } else {
            None
        };
        if self.offline && bundle.is_none() {
            self.feed
                .push(Kind::Bad, "Offline mode is on but no bundle path is set.");
            return None;
        }
        let keys: Vec<String> = comps.iter().map(|c| c.key().to_string()).collect();
        match wizard::config_from_flags(
            Some(keys),
            Some(
                match self.scope {
                    Scope::User => "user",
                    Scope::System => "system",
                }
                .to_string(),
            ),
            None,
            None,
            Some(
                match self.ide_mode {
                    IdeMode::Portable => "portable",
                    IdeMode::System => "system",
                }
                .to_string(),
            ),
            bundle,
        ) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                self.feed.push(Kind::Bad, format!("{:#}", e));
                None
            }
        }
    }

    fn start_install(&mut self) {
        let Some(cfg) = self.build_config() else {
            return;
        };
        self.busy = true;
        self.feed.start("install");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = crate::install::run(&cfg, true).map_err(|e| format!("{:#}", e));
            let _ = tx.send(Msg::Finished(r));
        });
    }

    fn start_uninstall(&mut self) {
        self.busy = true;
        self.feed.start("uninstall");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = crate::uninstall::run(true).map_err(|e| format!("{:#}", e));
            let _ = tx.send(Msg::Finished(r));
        });
    }

    fn start_update_check(&mut self) {
        self.checking = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let rows = vec![
                update::latest_for(crate::REPO),
                update::latest_for(crate::OYVEY_REPO),
            ];
            let _ = tx.send(Msg::UpdateChecked(rows));
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain();
        if self.busy || self.checking {
            // Nothing else would trigger a repaint, so the progress bar would
            // freeze partway through a download.
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        self.pick_up_dropped_file(ctx);

        egui::TopBottomPanel::top("header").show(ctx, |ui| self.header(ui));
        egui::TopBottomPanel::bottom("log")
            .resizable(true)
            .default_height(200.0)
            .min_height(80.0)
            .show(ctx, |ui| self.log_pane(ui));
        egui::CentralPanel::default().show(ctx, |ui| self.body(ui));
    }
}

impl App {
    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(tex) = &self.logo {
                ui.add(
                    egui::Image::new(egui::load::SizedTexture::new(
                        tex.id(),
                        egui::vec2(44.0, 44.0),
                    ))
                    .sense(egui::Sense::hover()),
                );
            }
            ui.vertical(|ui| {
                ui.heading("Rak installer");
                ui.label(
                    egui::RichText::new(format!("rak-setup {}", env!("CARGO_PKG_VERSION")))
                        .small()
                        .weak(),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.busy {
                    "Working..."
                } else if self.checking {
                    "Checking..."
                } else {
                    "Ready"
                };
                ui.label(egui::RichText::new(label).small());
                ui.add_space(6.0);
            });
        });
        ui.separator();
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(4.0);

            egui::Grid::new("components")
                .num_columns(2)
                .spacing([24.0, 6.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Install").strong());
                    ui.end_row();
                    for (i, c) in Component::all().iter().enumerate() {
                        ui.add(egui::Checkbox::new(&mut self.selected[i], c.label()));
                        ui.end_row();
                    }
                });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Scope").strong());
                ui.selectable_value(&mut self.scope, Scope::User, "user (no privileges)");
                ui.selectable_value(&mut self.scope, Scope::System, "system-wide (sudo/UAC)");
            });

            let ide_index = Component::all()
                .iter()
                .position(|c| *c == Component::Ide)
                .unwrap_or(usize::MAX);
            if self.selected.get(ide_index).copied().unwrap_or(false) {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("IDE").strong());
                    ui.selectable_value(&mut self.ide_mode, IdeMode::Portable, "portable dir");
                    ui.selectable_value(&mut self.ide_mode, IdeMode::System, "system location");
                });
            }

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.offline, "Offline bundle");
                if self.offline {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.offline_path)
                            .desired_width(260.0)
                            .hint_text("path to rak-bundle-<os>-x86_64.tar.gz"),
                    );
                    if ui.button("Browse").clicked() {
                        self.feed.note(
                            "Drop the bundle onto this window, or pass --offline on the command line.",
                        );
                    }
                }
            });

            ui.add_space(6.0);
            if let Some(text) = &self.installed {
                ui.label(egui::RichText::new(text).small().weak());
            }

            // Show the resolved destination, because "install for user" and
            // "install system-wide" land in very different places and a wrong
            // click here is annoying to undo.
            match crate::platform::default_bin_dir(self.scope) {
                Ok(dir) => ui.label(
                    egui::RichText::new(format!("Binaries will go to {}", dir.display()))
                        .small()
                        .weak(),
                ),
                Err(e) => ui.label(
                    egui::RichText::new(format!("Cannot resolve bin dir: {:#}", e))
                        .small()
                        .weak(),
                ),
            };

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let label = if self.installed.is_some() {
                    "Install / update"
                } else {
                    "Install"
                };
                let any = self.selected.iter().any(|s| *s);
                if ui
                    .add_enabled(
                        !self.busy && any,
                        egui::Button::new(egui::RichText::new(label).size(15.0))
                            .min_size(egui::vec2(150.0, 34.0)),
                    )
                    .clicked()
                {
                    self.start_install();
                }

                let have = self.installed.is_some();
                if ui
                    .add_enabled(
                        !self.busy && have,
                        egui::Button::new(egui::RichText::new("Uninstall").size(15.0))
                            .min_size(egui::vec2(110.0, 34.0)),
                    )
                    .clicked()
                {
                    self.start_uninstall();
                }

                if ui
                    .add_enabled(
                        !self.busy && !self.checking,
                        egui::Button::new(if self.checking {
                            "Checking..."
                        } else {
                            "Check for updates"
                        })
                        .min_size(egui::vec2(150.0, 34.0)),
                    )
                    .clicked()
                {
                    self.start_update_check();
                }
            });

            if !self.releases.is_empty() {
                ui.add_space(10.0);
                for row in &self.releases {
                    let text = match &row.error {
                        Some(e) => format!("{}: {}", row.repo, e),
                        None => format!(
                            "{} {}",
                            row.repo,
                            if row.tag.is_empty() {
                                "(no release)"
                            } else {
                                &row.tag
                            }
                        ),
                    };
                    ui.horizontal(|ui| {
                        if row.newer {
                            ui.label(
                                egui::RichText::new("update available")
                                    .color(egui::Color32::LIGHT_YELLOW),
                            );
                        }
                        if ui.hyperlink_to(text, &row.url).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::same_tab(&row.url));
                        }
                    });
                }
            }
        });
    }

    fn log_pane(&mut self, ui: &mut egui::Ui) {
        if let Some((frac, label)) = &self.feed.progress {
            ui.add(egui::ProgressBar::new(*frac).desired_width(ui.available_width()));
            ui.label(egui::RichText::new(label).small());
        }
        ui.separator();

        let autoscroll = self.feed.autoscroll;
        // `stick_to_bottom` follows the newest line, and `autoscroll` is cleared
        // by the user dragging the scrollbar -- yanking the view down
        // mid-install is worse than briefly missing the newest line.
        egui::ScrollArea::vertical()
            .stick_to_bottom(autoscroll)
            .auto_shrink([false, false])
            .max_height(f32::INFINITY)
            .show(ui, |ui| {
                for line in &self.feed.log {
                    let colour = match line.kind {
                        Kind::Plain => egui::Color32::GRAY,
                        Kind::Good => egui::Color32::LIGHT_GREEN,
                        Kind::Bad => egui::Color32::LIGHT_RED,
                        Kind::Note => egui::Color32::LIGHT_BLUE,
                    };
                    ui.label(egui::RichText::new(&line.text).monospace().color(colour));
                }
            });
    }

    /// Accept a bundle dropped onto the window.
    ///
    /// The text field is otherwise the only way to name a file, and typing a
    /// path is the part of an offline install that goes wrong most often.
    fn pick_up_dropped_file(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .next()
        });
        let Some(path) = dropped else { return };
        self.offline = true;
        self.offline_path = path.to_string_lossy().into_owned();
        self.feed
            .note(format!("Using bundle {}", self.offline_path));
    }
}

#[cfg(test)]
mod tests {
    use super::{human_bytes, Feed, Kind, Msg, LOG_CAPACITY};
    use crate::update::ReleaseRow;

    fn texts(f: &Feed) -> Vec<&str> {
        f.log.iter().map(|l| l.text.as_str()).collect()
    }

    #[test]
    fn byte_counts_are_readable() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
        // Past the last unit, stop rather than index off the end.
        assert_eq!(human_bytes(1024u64.pow(4)), "1024.0 GiB");
    }

    #[test]
    fn determinate_progress_is_a_real_fraction() {
        let mut f = Feed::default();
        f.apply(Msg::Progress(512, Some(1024)));
        let (frac, label) = f.progress.unwrap();
        assert!((frac - 0.5).abs() < 1e-6, "got {}", frac);
        assert_eq!(label, "512 B / 1.0 KiB");
    }

    #[test]
    fn progress_cannot_exceed_full() {
        // A server that reports fewer bytes than it sends must not push the bar
        // past the end, which would render as an overfull bar.
        let mut f = Feed::default();
        f.apply(Msg::Progress(2000, Some(1024)));
        assert_eq!(f.progress.unwrap().0, 1.0);
    }

    #[test]
    fn unknown_total_reports_bytes_not_a_percentage() {
        let mut f = Feed::default();
        f.apply(Msg::Progress(2048, None));
        assert_eq!(f.progress.unwrap().1, "2.0 KiB downloaded");
    }

    #[test]
    fn finishing_clears_the_progress_bar() {
        let mut f = Feed::default();
        f.apply(Msg::Progress(1024, Some(1024)));
        assert!(f.progress.is_some());
        let finished = f.apply(Msg::Finished(Ok(0)));
        assert!(finished, "the caller needs to know to refresh the manifest");
        assert!(f.progress.is_none(), "a stale bar would never move again");
        assert_eq!(texts(&f).last().copied(), Some("done."));
        assert_eq!(f.log.back().unwrap().kind, Kind::Good);
    }

    #[test]
    fn a_failed_install_is_visible_and_red() {
        let mut f = Feed::default();
        f.apply(Msg::Finished(Err("download failed for rakc".into())));
        assert!(f.progress.is_none());
        let last = f.log.back().unwrap();
        assert_eq!(last.kind, Kind::Bad);
        assert!(last.text.contains("download failed"), "{}", last.text);
    }

    #[test]
    fn a_nonzero_exit_code_is_an_error() {
        // `install::run` reports failure through the exit code, not `Err`, so a
        // code other than 0 must not be reported as success.
        let mut f = Feed::default();
        f.apply(Msg::Finished(Ok(1)));
        assert_eq!(f.log.back().unwrap().kind, Kind::Bad);
    }

    #[test]
    fn update_results_are_distinguished_from_failures() {
        let mut f = Feed::default();
        f.apply(Msg::UpdateChecked(vec![
            ReleaseRow {
                repo: "Louiml/Rak",
                tag: "v0.10.0".into(),
                url: "u".into(),
                newer: true,
                error: None,
            },
            ReleaseRow {
                repo: "Louiml/oyvey",
                tag: String::new(),
                url: "u".into(),
                newer: false,
                error: Some("network unreachable".into()),
            },
        ]));
        assert_eq!(f.log[0].kind, Kind::Note, "an update should stand out");
        assert!(f.log[0].text.contains("v0.10.0"));
        // A failed check is shown, never quietly reported as up to date.
        assert_eq!(f.log[1].kind, Kind::Bad);
        assert!(f.log[1].text.contains("network unreachable"));
    }

    #[test]
    fn an_update_check_is_not_treated_as_a_finished_install() {
        // Otherwise the caller clears `busy` and re-reads the manifest for an
        // operation that never ran.
        let mut f = Feed::default();
        assert!(!f.apply(Msg::UpdateChecked(vec![])));
        assert!(!f.apply(Msg::Progress(1, Some(2))));
    }

    #[test]
    fn starting_clears_the_previous_run() {
        let mut f = Feed::default();
        f.push(Kind::Plain, "old line");
        f.start("install");
        assert!(texts(&f).iter().all(|t| !t.contains("old line")));
        assert_eq!(texts(&f), vec!["Starting install."]);
    }

    #[test]
    fn the_log_is_bounded() {
        let mut f = Feed::default();
        for i in 0..(LOG_CAPACITY + 250) {
            f.push(Kind::Plain, format!("line {}", i));
        }
        assert_eq!(f.log.len(), LOG_CAPACITY);
        // The newest survive, the oldest are dropped.
        let newest = (LOG_CAPACITY + 249).to_string();
        assert!(
            f.log.back().unwrap().text.ends_with(&newest),
            "the newest line should have survived"
        );
    }
}
