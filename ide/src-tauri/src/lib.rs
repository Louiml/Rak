use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

#[derive(serde::Serialize)]
struct FileEntry {
    name: String,
    path: String,
    is_dir: bool,
}

#[derive(serde::Serialize, Clone)]
struct RakLine {
    stream: String,
    text: String,
}

static RUNNING: Mutex<Option<Child>> = Mutex::new(None);
static SHELL: Mutex<Option<Child>> = Mutex::new(None);

fn rakc_binary() -> Option<String> {
    let ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
    let mut candidates = vec!["rakc".to_string()];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("rakc").to_string_lossy().to_string());
            candidates.push(parent.join(format!("rakc{}", ext)).to_string_lossy().to_string());
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/release/rakc{}", ext))
        .to_string_lossy().to_string());
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/debug/rakc{}", ext))
        .to_string_lossy().to_string());
    for c in candidates {
        if Command::new(&c)
            .arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
        {
            return Some(c);
        }
    }
    None
}

#[tauri::command]
fn rakc_version() -> Result<String, String> {
    match rakc_binary() {
        Some(b) => match Command::new(&b).arg("--version").output() {
            Ok(o) => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
            Err(e) => Err(e.to_string()),
        },
        None => Err("rakc binary not found".to_string()),
    }
}

#[tauri::command]
fn run_rak(app: AppHandle, mode: String, source: String) -> Result<(), String> {
    let cmd = match mode.as_str() {
        "vm" => "vm",
        "bench" => "bench",
        _ => "run",
    };
    let bin = rakc_binary().ok_or("rakc binary not found")?;
    let mut tmp = std::env::temp_dir();
    tmp.push(format!("rak_ide_{}.rak", std::process::id()));
    fs::write(&tmp, &source).map_err(|e| e.to_string())?;

    let mut child = Command::new(&bin)
        .arg(cmd)
        .arg(&tmp)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    {
        let mut guard = RUNNING.lock().unwrap();
        if let Some(mut prev) = guard.take() {
            let _ = prev.kill();
            let _ = prev.wait();
        }
        *guard = Some(child);
    }

    if let Some(out) = stdout {
        let app2 = app.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().flatten() {
                let _ = app2.emit("rak-output", RakLine { stream: "stdout".into(), text: line });
            }
        });
    }
    if let Some(err) = stderr {
        let app2 = app.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().flatten() {
                let _ = app2.emit("rak-output", RakLine { stream: "stderr".into(), text: line });
            }
        });
    }

    let app3 = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let finished = {
            let mut guard = RUNNING.lock().unwrap();
            match guard.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(_)) => { *guard = None; true }
                    Ok(None) => false,
                    Err(_) => { *guard = None; true }
                },
                None => true,
            }
        };
        if finished {
            let _ = app3.emit("rak-done", ());
            break;
        }
    });

    Ok(())
}

#[tauri::command]
fn stop_rak() -> Result<(), String> {
    let mut guard = RUNNING.lock().unwrap();
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

#[tauri::command]
fn run_shell(app: AppHandle, cmd: String, cwd: String) -> Result<(), String> {
    let shell = if cfg!(windows) { "cmd" } else { "sh" };
    let flag = if cfg!(windows) { "/C" } else { "-c" };

    let mut child = Command::new(shell)
        .arg(flag)
        .arg(&cmd)
        .current_dir(if cwd.is_empty() { "." } else { &cwd })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    {
        let mut guard = SHELL.lock().unwrap();
        if let Some(mut prev) = guard.take() {
            let _ = prev.kill();
            let _ = prev.wait();
        }
        *guard = Some(child);
    }

    if let Some(out) = stdout {
        let app2 = app.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().flatten() {
                let _ = app2.emit("shell-output", RakLine { stream: "stdout".into(), text: line });
            }
        });
    }
    if let Some(err) = stderr {
        let app2 = app.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().flatten() {
                let _ = app2.emit("shell-output", RakLine { stream: "stderr".into(), text: line });
            }
        });
    }

    let app3 = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let finished = {
            let mut guard = SHELL.lock().unwrap();
            match guard.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(_)) => { *guard = None; true }
                    Ok(None) => false,
                    Err(_) => { *guard = None; true }
                },
                None => true,
            }
        };
        if finished {
            let _ = app3.emit("shell-done", ());
            break;
        }
    });

    Ok(())
}

#[tauri::command]
fn shell_stop() -> Result<(), String> {
    let mut guard = SHELL.lock().unwrap();
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

#[tauri::command]
fn save_file(path: String, content: String) -> Result<(), String> {
    fs::write(&path, content).map_err(|e| e.to_string())
}

#[tauri::command]
fn read_file(path: String) -> Result<String, String> {
    fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_dir(path: String) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();
    let path = PathBuf::from(&path);

    if let Ok(read_dir) = fs::read_dir(&path) {
        for entry in read_dir.flatten() {
            if let Ok(metadata) = entry.metadata() {
                let name = entry.file_name().to_string_lossy().to_string();
                let entry_path = entry.path().to_string_lossy().to_string();
                if name.starts_with('.') || name == "node_modules" || name == "target" {
                    continue;
                }
                entries.push(FileEntry {
                    name,
                    path: entry_path,
                    is_dir: metadata.is_dir(),
                });
            }
        }
    }

    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(entries)
}

#[tauri::command]
fn list_files_recursive(path: String, ext: String) -> Result<Vec<FileEntry>, String> {
    let mut results = Vec::new();
    fn walk(dir: &PathBuf, ext: &str, results: &mut Vec<FileEntry>) {
        if let Ok(read_dir) = fs::read_dir(dir) {
            for entry in read_dir.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') || name == "node_modules" || name == "target" || name == "out" {
                        continue;
                    }
                    let path = entry.path();
                    if metadata.is_dir() {
                        walk(&path, ext, results);
                    } else if ext.is_empty() || name.ends_with(&ext) {
                        results.push(FileEntry {
                            name,
                            path: path.to_string_lossy().to_string(),
                            is_dir: false,
                        });
                    }
                }
            }
        }
    }
    walk(&PathBuf::from(&path), &ext, &mut results);
    results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(results)
}

#[tauri::command]
fn current_dir() -> Result<String, String> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn create_file(path: String) -> Result<(), String> {
    fs::write(&path, "").map_err(|e| e.to_string())
}

#[tauri::command]
fn create_dir(path: String) -> Result<(), String> {
    fs::create_dir_all(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_file(path: String) -> Result<(), String> {
    let path = PathBuf::from(&path);
    if path.is_dir() {
        fs::remove_dir_all(&path).map_err(|e| e.to_string())
    } else {
        fs::remove_file(&path).map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn rename_file(old_path: String, new_path: String) -> Result<(), String> {
    fs::rename(&old_path, &new_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn duplicate_file(src: String) -> Result<(), String> {
    let src_path = PathBuf::from(&src);
    let stem = src_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = src_path.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
    let parent = src_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let mut dst = parent.join(format!("{}_copy{}", stem, ext));
    let mut i = 2;
    while dst.exists() {
        dst = parent.join(format!("{}_copy{}{}", stem, i, ext));
        i += 1;
    }
    fs::copy(&src, &dst).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn open_in_explorer(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer.exe").arg(&path).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(&path).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(&path).spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                let _ = app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                );
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            run_rak, stop_rak, rakc_version,
            run_shell, shell_stop,
            save_file, read_file, list_dir, list_files_recursive, current_dir,
            create_file, create_dir, delete_file,
            rename_file, duplicate_file, open_in_explorer
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
