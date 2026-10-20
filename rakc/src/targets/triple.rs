//! Compilation target triples.
//!
//! A triple names the platform a compilation targets, the way `rustc --target`
//! does. Only triples from [`SUPPORTED`] are accepted: a typo or an invented
//! platform gets an error listing what exists, rather than a silently ignored
//! `--target` that builds for the host.

use std::fmt;

/// The CPU architecture a target runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
    X86,
    Wasm32,
}

impl Arch {
    pub fn as_str(&self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
            Arch::X86 => "x86",
            Arch::Wasm32 => "wasm32",
        }
    }
}

/// The operating system a target runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Linux,
    Macos,
    Wasi,
    /// A bare-metal / freestanding platform (`wasm32-unknown-unknown`).
    Unknown,
}

impl Os {
    pub fn as_str(&self) -> &'static str {
        match self {
            Os::Windows => "windows",
            Os::Linux => "linux",
            Os::Macos => "macos",
            Os::Wasi => "wasi",
            Os::Unknown => "unknown",
        }
    }
}

/// A compilation target: an architecture, an OS, and an optional environment
/// (the ABI: `msvc`, `gnu`, `musl`, `wasi`, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Triple {
    pub arch: Arch,
    pub os: Os,
    pub env: Option<&'static str>,
    /// The canonical triple string, e.g. `x86_64-pc-windows-msvc`.
    pub name: &'static str,
}

impl Triple {
    /// Whether the triple's executable format is PE/COFF (Windows).
    pub fn is_windows(&self) -> bool {
        self.os == Os::Windows
    }

    /// The executable-file suffix a linked artifact should carry on this target
    /// (empty on Unix, `.exe` on Windows).
    pub fn exe_suffix(&self) -> &'static str {
        if self.is_windows() {
            ".exe"
        } else {
            ""
        }
    }

    /// Whether this triple is a WebAssembly platform.
    pub fn is_wasm(&self) -> bool {
        self.arch == Arch::Wasm32
    }
}

impl fmt::Display for Triple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

/// Every target rakc accepts. New targets are added here and nowhere else, so
/// `--print target-list`, the error message, and the parsers cannot disagree.
pub const SUPPORTED: &[Triple] = &[
    Triple {
        arch: Arch::X86_64,
        os: Os::Windows,
        env: Some("msvc"),
        name: "x86_64-pc-windows-msvc",
    },
    Triple {
        arch: Arch::X86_64,
        os: Os::Windows,
        env: Some("gnu"),
        name: "x86_64-pc-windows-gnu",
    },
    Triple {
        arch: Arch::X86_64,
        os: Os::Linux,
        env: Some("gnu"),
        name: "x86_64-unknown-linux-gnu",
    },
    Triple {
        arch: Arch::X86_64,
        os: Os::Linux,
        env: Some("musl"),
        name: "x86_64-unknown-linux-musl",
    },
    Triple {
        arch: Arch::X86_64,
        os: Os::Macos,
        env: Some("darwin"),
        name: "x86_64-apple-darwin",
    },
    Triple {
        arch: Arch::Aarch64,
        os: Os::Macos,
        env: Some("darwin"),
        name: "aarch64-apple-darwin",
    },
    Triple {
        arch: Arch::Aarch64,
        os: Os::Linux,
        env: Some("gnu"),
        name: "aarch64-unknown-linux-gnu",
    },
    Triple {
        arch: Arch::Wasm32,
        os: Os::Unknown,
        env: None,
        name: "wasm32-unknown-unknown",
    },
    Triple {
        arch: Arch::Wasm32,
        os: Os::Wasi,
        env: Some("wasi"),
        name: "wasm32-wasi",
    },
];

/// The target this very binary was built for.
pub fn host_triple() -> Triple {
    let name = if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "windows", target_arch = "x86")) {
        "x86"
    } else {
        // The build host is something off the supported list. Compiling for the
        // host is still the default for every flag combination, so say so
        // plainly instead of guessing which supported triple it resembles.
        "unknown-host"
    };
    find(name).unwrap_or({
        // `find` only misses when `name` is "unknown-host", which the map above
        // deliberately produces for an unlisted host.
        Triple {
            arch: if cfg!(target_arch = "x86_64") {
                Arch::X86_64
            } else {
                Arch::X86
            },
            os: if cfg!(target_os = "windows") {
                Os::Windows
            } else if cfg!(target_os = "macos") {
                Os::Macos
            } else {
                Os::Linux
            },
            env: None,
            name: "unknown-host",
        }
    })
}

/// Look up a supported triple by its canonical name (case-insensitive).
pub fn find(name: &str) -> Option<Triple> {
    let lower = name.trim().to_ascii_lowercase();
    SUPPORTED.iter().copied().find(|t| t.name == lower)
}

/// The error for an unknown triple, naming what is accepted.
pub fn unknown_target_error(name: &str) -> String {
    let names: Vec<&str> = SUPPORTED.iter().map(|t| t.name).collect();
    format!(
        "unknown target triple '{}'\n\nsupported targets:\n{}\n\nRun `rakc build --print target-list` for the same list.",
        name,
        names
            .iter()
            .map(|n| format!("  {}", n))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_is_case_insensitive_and_trims() {
        assert_eq!(
            find("  X86_64-PC-Windows-Msvc ").map(|t| t.name),
            Some("x86_64-pc-windows-msvc")
        );
        assert!(find("x86_64-pc-solaris").is_none());
    }

    #[test]
    fn host_triple_resolves() {
        assert!(
            find(host_triple().name).is_some()
                || cfg!(not(any(
                    target_os = "windows",
                    target_os = "linux",
                    target_os = "macos"
                )))
        );
    }

    #[test]
    fn wasm_and_windows_flags() {
        let wasm = find("wasm32-wasi").unwrap();
        assert!(wasm.is_wasm());
        assert!(!wasm.is_windows());
        let win = find("x86_64-pc-windows-msvc").unwrap();
        assert!(win.is_windows());
        assert_eq!(win.exe_suffix(), ".exe");
        assert!(!find("x86_64-unknown-linux-gnu").unwrap().is_windows());
    }

    #[test]
    fn unknown_error_lists_supported() {
        let err = unknown_target_error("sparc64-sun-solaris");
        assert!(err.contains("sparc64-sun-solaris"));
        for t in SUPPORTED {
            assert!(err.contains(t.name));
        }
    }

    #[test]
    fn supported_list_has_unique_names() {
        let mut names: Vec<&str> = SUPPORTED.iter().map(|t| t.name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "duplicate triple in SUPPORTED");
    }
}
