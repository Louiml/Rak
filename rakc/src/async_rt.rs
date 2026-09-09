//! The shared Tokio runtime that powers the async event loop for both the
//! interpreter and the bytecode VM. Lazily started on first use.

use std::sync::OnceLock;

static ASYNC_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Get the process-wide async runtime, starting it on first call.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    ASYNC_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to start async runtime")
    })
}
