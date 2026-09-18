//! The shared Tokio runtime that powers the async event loop for both the
//! interpreter and the bytecode VM. Lazily started on first use.
//!
//! Also hosts concurrency primitives: a bounded semaphore that serialises
//! `async fn` bodies onto a small worker pool (so thousands of concurrent
//! logical operations share a handful of OS threads instead of spawning one
//! thread each), plus helpers the interpreter uses to drive deferred futures.

use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicU64, Ordering};

static ASYNC_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static NEXT_GROUP: AtomicU64 = AtomicU64::new(1);

/// Get the process-wide async runtime, starting it on first use.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    ASYNC_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(num_cpus_hint())
            .build()
            .expect("failed to start async runtime")
    })
}

fn num_cpus_hint() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).max(2)
}

/// A simple non-Tokio counting semaphore used to bound how many concurrent
/// `async fn` bodies / blocking work items run on OS threads at once. Because
/// the interpreter's `await` uses `block_on` (which must run on a plain thread,
/// not a Tokio worker), concurrent future driving uses plain OS threads rather
/// than Tokio tasks; this permit keeps the count bounded so "thousands of
/// concurrent operations" only ever run a handful of OS threads at a time.
pub struct Permit(std::sync::Mutex<usize>);

/// A held permit; releases its slot back into the pool on drop.
pub struct PermitGuard(Arc<Permit>);

impl Permit {
    /// Acquire a permit, blocking the current thread until one is free.
    pub fn acquire(self: &Arc<Permit>) -> PermitGuard {
        let mut guard = self.0.lock().unwrap();
        while *guard == 0 {
            drop(guard);
            std::thread::sleep(std::time::Duration::from_micros(200));
            guard = self.0.lock().unwrap();
        }
        *guard -= 1;
        drop(guard);
        PermitGuard(self.clone())
    }
}

impl Drop for PermitGuard {
    fn drop(&mut self) {
        *self.0 .0.lock().unwrap() += 1;
    }
}

static PERMIT_CELL: OnceLock<Arc<Permit>> = OnceLock::new();

/// Get the shared bounded-permit pool for concurrent future driving.
pub fn permit_count() -> Arc<Permit> {
    PERMIT_CELL
        .get_or_init(|| Arc::new(Permit(std::sync::Mutex::new(num_cpus_hint() * 16))))
        .clone()
}

/// Create a fresh permit pool with an explicit capacity (for `task_group`'s
/// per-group bounded concurrency limit).
pub fn permit_count_raw(cap: usize) -> Arc<Permit> {
    Arc::new(Permit(std::sync::Mutex::new(cap.max(1))))
}

/// Recommended default number of concurrent worker threads (for task groups).
pub fn num_workers_hint() -> u64 {
    num_cpus_hint() as u64
}

/// Allocate a fresh task-group id (used for task-group bookkeeping in the VM).
pub fn next_group_id() -> u64 {
    NEXT_GROUP.fetch_add(1, Ordering::Relaxed)
}