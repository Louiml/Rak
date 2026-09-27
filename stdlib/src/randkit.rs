//! Randomness builtins for Rak (`rand_int`, `rand_bytes`, `rand_seed`, ...).
//!
//! A process-global `StdRng` behind a `Mutex`, entropy-seeded on first use
//! and re-seedable for reproducible scripts/tests (`rand_seed`).

use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};
use std::sync::{Mutex, OnceLock};

static RNG: OnceLock<Mutex<StdRng>> = OnceLock::new();

fn rng() -> &'static Mutex<StdRng> {
    RNG.get_or_init(|| Mutex::new(StdRng::from_entropy()))
}

/// Run `f` with the global generator (locks it for the duration).
pub fn with_rng<R>(f: impl FnOnce(&mut StdRng) -> R) -> R {
    let mut g = rng().lock().unwrap_or_else(|p| p.into_inner());
    f(&mut g)
}

/// Deterministically reseed the generator (`rand_seed(n)`).
pub fn reseed(seed: i64) {
    let mut g = rng().lock().unwrap_or_else(|p| p.into_inner());
    *g = StdRng::seed_from_u64(seed as u64);
}

/// Uniform integer in `[lo, hi)` — errors when `lo >= hi` (caller checks).
pub fn gen_int(lo: i64, hi: i64) -> Result<i64, String> {
    if lo >= hi {
        return Err(format!("rand_int: empty range [{}..{})", lo, hi));
    }
    Ok(with_rng(|g| g.gen_range(lo..hi)))
}

/// Float in `[0.0, 1.0)`.
pub fn gen_float() -> f64 {
    with_rng(|g| g.gen::<f64>())
}

/// `n` random bytes.
pub fn gen_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    with_rng(|g| g.fill_bytes(&mut buf));
    buf
}

/// `n` lowercase hex chars (n/2 random bytes, rounded up).
pub fn gen_hex(n: usize) -> String {
    let bytes = gen_bytes(n.div_ceil(2));
    let mut s = String::with_capacity(n);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s.truncate(n);
    s
}

/// Random index into `len` items (for `rand_choice`).
pub fn gen_index(len: usize) -> Option<usize> {
    if len == 0 {
        None
    } else {
        Some(with_rng(|g| g.gen_range(0..len)))
    }
}

/// In-place Fisher-Yates shuffle (for `rand_shuffle`).
pub fn gen_shuffle<T>(v: &mut [T]) {
    use rand::seq::SliceRandom;
    with_rng(|g| v.shuffle(g));
}

#[cfg(test)]
mod tests {
    use super::*;
    // The generator is process-global; serialize the tests that reseed it.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn seeded_ints_are_reproducible() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reseed(42);
        let a = (0..5).map(|_| gen_int(0, 100).unwrap()).collect::<Vec<_>>();
        reseed(42);
        let b = (0..5).map(|_| gen_int(0, 100).unwrap()).collect::<Vec<_>>();
        assert_eq!(a, b);
    }

    #[test]
    fn ranges_hold() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reseed(7);
        for _ in 0..200 {
            let v = gen_int(-5, 5).unwrap();
            assert!((-5..5).contains(&v));
        }
        assert!(gen_int(5, 5).is_err());
        let f = gen_float();
        assert!((0.0..1.0).contains(&f));
    }

    #[test]
    fn bytes_and_hex_lengths() {
        let b = gen_bytes(16);
        assert_eq!(b.len(), 16);
        let h = gen_hex(9);
        assert_eq!(h.len(), 9);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
}

