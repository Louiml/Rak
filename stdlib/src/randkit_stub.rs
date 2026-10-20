//! Randomness builtins, in a build without the `rand` crate.
//!
//! The `rand` crate needs entropy, and on the web that means `getrandom`,
//! whose browser backend pulls in `wasm-bindgen` and a pile of generated glue.
//! The browser build of Rak does not need reproducible randomness, so the
//! dependency is left out and these functions say so instead of inventing a
//! value.
//!
//! The real implementation is `randkit.rs`; this file exists so the module
//! keeps the same shape either way, and a program that calls `rand_int` in the
//! browser gets a clear error rather than "Unknown function".

/// The error every function here reports.
fn no_rand() -> String {
    "rand: this build has no random number generator (the `rand` feature is off)".to_string()
}

/// Deterministically reseed the generator (`rand_seed(n)`).
pub fn reseed(_seed: i64) {}

/// Uniform integer in `[lo, hi)`.
pub fn gen_int(_lo: i64, _hi: i64) -> Result<i64, String> {
    Err(no_rand())
}

/// Float in `[0.0, 1.0)`.
pub fn gen_float() -> f64 {
    0.0
}

/// `n` random bytes.
pub fn gen_bytes(_n: usize) -> Vec<u8> {
    Vec::new()
}

/// `n` lowercase hex chars.
pub fn gen_hex(_n: usize) -> String {
    String::new()
}

/// Random index into `len` items.
pub fn gen_index(_len: usize) -> Option<usize> {
    None
}

/// In-place Fisher-Yates shuffle.
pub fn gen_shuffle<T>(_v: &mut [T]) {}
