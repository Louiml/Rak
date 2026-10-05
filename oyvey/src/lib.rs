//! Oyvey — the Rak package manager and build system.
//!
//! Repository: <https://github.com/Louiml/oyvey>
//!
//! Oyvey is the official package manager for the Rak language. It is
//! Cargo-inspired: a `package.rak` manifest declares the project and its
//! dependencies, an `oyvey.lock` lockfile records the exact resolved revision
//! and checksum of every dependency, and a global git cache keeps clones so
//! builds are reproducible and work offline once resolved.
//!
//! ```text
//! oyvey new <project>     generate a new Rak project
//! oyvey init              initialize an existing directory
//! oyvey add <github-repo> add a GitHub package dependency
//! oyvey remove <package>  drop a dependency
//! oyvey install           resolve + vendor dependencies (writes oyvey.lock)
//! oyvey update            re-resolve within constraints
//! oyvey build             compile the entry point through rakc
//! oyvey run               run the entry point
//! oyvey test              run the project's tests
//! oyvey clean             remove build artifacts
//! oyvey list              list installed packages
//! ```
//!
//! The crate is split into small modules so the compiler, IDE, and fuzz
//! targets can reuse the manifest/lockfile logic without shelling out to the
//! `oyvey` binary:
//!
//! * [`manifest`] — parse the `package.rak` manifest (a Rak file with `let`
//!   bindings, matching Rak's existing package conventions).
//! * [`spec`] — parse `user/repo[@constraint][#rev]` dependency specs and
//!   semver constraint satisfaction.
//! * [`lock`] — read/write the `oyvey.lock` TOML lockfile.
//! * [`git`] — git operations against the global cache (clone/fetch/checkout).
//! * [`cache`] — the on-disk layout of the global cache.
//! * [`resolve`] — transitive dependency resolution.
//! * [`project`] — project discovery and `oyvey new` scaffolding.

pub mod cache;
pub mod git;
pub mod lock;
pub mod manifest;
pub mod project;
pub mod resolve;
pub mod spec;

pub use cache::Cache;
pub use lock::{LockEntry, LockFile};
pub use manifest::{Manifest, MANIFEST_FILE, ENTRY_DEFAULT};
pub use project::{find_project_root, packages_dir, vendor_packages};
pub use resolve::Resolver;
pub use spec::{parse_dep_spec, version_satisfies, DepSpec};

/// The lockfile file name.
pub const LOCK_FILE: &str = "oyvey.lock";

/// The directory (relative to the project root) where dependencies are
/// vendored. This matches the compiler's `./packages/` search convention, so
/// `import <pkg>` resolves to `packages/<pkg>` with no extra configuration.
pub const PACKAGES_DIR: &str = "packages";

/// The environment variable Oyvey prepends to `RAK_PATH` when invoking rakc,
/// so imports resolve against the vendored packages.
pub const RAK_PATH_ENV: &str = "RAK_PATH";

/// Oyvey's own home override (like `CARGO_HOME`). Defaults to `~/.oyvey`.
pub const OYVEY_HOME_ENV: &str = "OYVEY_HOME";

/// The version of the lockfile format this build writes/reads.
pub const LOCK_VERSION: i64 = 1;
