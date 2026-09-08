use std::fs;
use std::path::PathBuf;

fn main() {
    tauri_build::build();

    let ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
    let res_dir = PathBuf::from("resources");
    let _ = fs::create_dir_all(&res_dir);

    let rakc_src = PathBuf::from(format!("../../target/release/rakc{}", ext));
    let rakpkg_src = PathBuf::from(format!("../../target/release/rakpkg{}", ext));

    if rakc_src.exists() {
        let _ = fs::copy(&rakc_src, res_dir.join("rakc"));
        println!("cargo:rerun-if-changed={}", rakc_src.display());
    }
    if rakpkg_src.exists() {
        let _ = fs::copy(&rakpkg_src, res_dir.join("rakpkg"));
        println!("cargo:rerun-if-changed={}", rakpkg_src.display());
    }
}
