//! Host C toolchain discovery.
//!
//! The `c`, `wasm`, `obj` and `link` emit kinds compile generated C with a
//! C compiler on `PATH` ג€” the same model rustc uses (it emits code and invokes
//! the platform linker). The difference is that rakc also *ships* the C source,
//! so a missing compiler degrades to "you still have the `.c` file" rather
//! than to nothing.

use std::path::PathBuf;
use std::process::Command;

/// Which C compiler front-end was found. The distinction matters only for
/// argument spelling (`-o file` vs `/Fe file`) and diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CcKind {
    Clang,
    Gcc,
    Msvc,
}

impl CcKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CcKind::Clang => "clang",
            CcKind::Gcc => "gcc",
            CcKind::Msvc => "cl",
        }
    }
}

/// A discovered C compiler.
#[derive(Clone, Debug)]
pub struct Cc {
    pub path: PathBuf,
    pub kind: CcKind,
}

impl Cc {
    /// Run the compiler, returning its output. A non-zero exit is an error
    /// carrying stderr: a compile error in generated C is a rakc bug until
    /// proven otherwise, and the reader needs the compiler's own words.
    pub fn run(&self, args: &[String]) -> Result<(), String> {
        let out = Command::new(&self.path)
            .args(args)
            .output()
            .map_err(|e| format!("running {} failed: {}", self.path.display(), e))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            return Err(format!(
                "{} failed ({})\n{}{}",
                self.kind.as_str(),
                args.join(" "),
                stderr.trim(),
                if stdout.trim().is_empty() {
                    String::new()
                } else {
                    format!("\n{}", stdout.trim())
                }
            ));
        }
        Ok(())
    }
}

/// The environment variables consulted before searching `PATH`, in order.
/// `RAKC_CC` names the rakc-specific override; `CC` is the conventional one.
pub fn find_cc() -> Option<Cc> {
    for var in ["RAKC_CC", "CC"] {
        if let Ok(value) = std::env::var(var) {
            if !value.trim().is_empty() {
                return cc_from_path(PathBuf::from(value.trim()));
            }
        }
    }
    let candidates = if cfg!(windows) {
        vec!["clang", "clang-cl", "gcc", "cc", "cl"]
    } else {
        vec!["clang", "cc", "gcc"]
    };
    for name in candidates {
        if let Some(p) = which(name) {
            return cc_from_path(p);
        }
    }
    None
}

/// Classify a compiler path by its file name.
fn cc_from_path(path: PathBuf) -> Option<Cc> {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let kind = if name.starts_with("clang-cl") {
        CcKind::Msvc
    } else if name.starts_with("clang") {
        CcKind::Clang
    } else if name == "cl" || name == "cl.exe" {
        CcKind::Msvc
    } else if name.starts_with("gcc") || name == "cc" {
        CcKind::Gcc
    } else {
        // An explicitly configured compiler (RAKC_CC=/path/to/mycpp) is assumed
        // to take gcc-compatible flags; that is the convention `CC` follows.
        CcKind::Gcc
    };
    Some(Cc { path, kind })
}
/// The error used when a link/object step is requested without a compiler.
/// The emitted source is still on disk, so the message says where.
pub fn missing_cc_error(source_path: &std::path::Path) -> String {
    format!(
        "no C compiler found for the link step.\n\n\
         The generated C source is at {} and can be compiled by hand:\n  \
         clang -O2 {} -o <output>\n\n\
         Install clang or gcc, or set RAKC_CC=/path/to/compiler.",
        source_path.display(),
        source_path.display()
    )
}

/// Locate an executable on `PATH` (cross-platform, no `which` dependency).
pub fn which(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        vec![".exe".into(), ".cmd".into(), ".bat".into(), "".into()]
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for ext in &exts {
            let candidate = dir.join(format!("{}{}", name, ext));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_finds_an_interpreter_on_path() {
        // `git` is required for oyvey and is on PATH in every environment this
        // repo's tests run in; it is also not a compiler, which keeps the test
        // honest about what `which` does.
        assert!(which("git").is_some());
        assert!(which("definitely-not-a-real-binary-xyz").is_none());
    }

    #[test]
    fn cc_from_path_classifies_by_name() {
        assert_eq!(
            cc_from_path(PathBuf::from("C:/llvm/bin/clang.exe"))
                .unwrap()
                .kind,
            CcKind::Clang
        );
        assert_eq!(
            cc_from_path(PathBuf::from("/usr/bin/gcc")).unwrap().kind,
            CcKind::Gcc
        );
        assert_eq!(
            cc_from_path(PathBuf::from("C:/VS/VC/bin/cl.exe"))
                .unwrap()
                .kind,
            CcKind::Msvc
        );
        // An unknown binary is assumed gcc-compatible, like `CC`.
        assert_eq!(
            cc_from_path(PathBuf::from("/opt/mycc")).unwrap().kind,
            CcKind::Gcc
        );
    }

    #[test]
    fn missing_cc_error_names_the_source_and_the_fix() {
        let err = missing_cc_error(std::path::Path::new("out/main.c"));
        assert!(err.contains("out/main.c"), "{}", err);
        assert!(err.contains("RAKC_CC"), "{}", err);
    }
}
