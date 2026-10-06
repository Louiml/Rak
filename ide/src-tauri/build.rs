use std::fs;
use std::path::PathBuf;

fn main() {
    tauri_build::build();

    let ext = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let res_dir = manifest_dir.join("resources");
    let _ = fs::create_dir_all(&res_dir);

    let rakc_src = manifest_dir.join(format!("../../target/release/rakc{}", ext));

    if rakc_src.exists() {
        match fs::copy(&rakc_src, res_dir.join("rakc")) {
            Ok(_) => println!("cargo:warning=Copied rakc to resources/"),
            Err(e) => println!("cargo:warning=Failed to copy rakc: {}", e),
        }
    } else {
        println!(
            "cargo:warning=rakc binary not found at {}",
            rakc_src.display()
        );
    }

    // No oyvey is bundled. It is a separate repository with its own releases, so it is
    // absent here by construction -- and a stale copy into `resources/` would be worse than
    // none, because `tauri.conf.json` only globs `resources/rakc*`, so the file would sit
    // in the source tree looking bundled while never reaching a user. Nothing in the IDE
    // spawns it.
}
