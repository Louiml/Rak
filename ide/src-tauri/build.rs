use std::fs;
use std::path::PathBuf;

fn main() {
    tauri_build::build();

    let ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let res_dir = manifest_dir.join("resources");
    let _ = fs::create_dir_all(&res_dir);

    let rakc_src = manifest_dir.join(format!("../../target/release/rakc{}", ext));
    let rakpkg_src = manifest_dir.join(format!("../../target/release/rakpkg{}", ext));

    if rakc_src.exists() {
        match fs::copy(&rakc_src, res_dir.join("rakc")) {
            Ok(_) => println!("cargo:warning=Copied rakc to resources/"),
            Err(e) => println!("cargo:warning=Failed to copy rakc: {}", e),
        }
    } else {
        println!("cargo:warning=rakc binary not found at {}", rakc_src.display());
    }

    if rakpkg_src.exists() {
        match fs::copy(&rakpkg_src, res_dir.join("rakpkg")) {
            Ok(_) => println!("cargo:warning=Copied rakpkg to resources/"),
            Err(e) => println!("cargo:warning=Failed to copy rakpkg: {}", e),
        }
    } else {
        println!("cargo:warning=rakpkg binary not found at {}", rakpkg_src.display());
    }
}
