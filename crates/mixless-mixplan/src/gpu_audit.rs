//! Opt-in offline Metal comparison. CPU scores alone reach planner decisions.
//! Run in an isolated auditor process. Complete mode uses bounded backpressure;
//! best-effort mode reports any omitted diagnostics as incomplete coverage.
/// Complete audits may wait for the GPU; best-effort audits may omit queries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Complete,
    BestEffort,
}

#[cfg(target_os = "macos")]
mod runtime;
#[cfg(target_os = "macos")]
pub use runtime::compare;
#[cfg(target_os = "macos")]
pub(crate) use runtime::{Capture, Input};

/// Unsupported platforms preserve the CPU result without loading GPU resources.
#[cfg(not(target_os = "macos"))]
pub fn compare<T>(_mode: Mode, run: impl FnOnce() -> T) -> (T, serde_json::Value) {
    (
        run(),
        serde_json::json!({"status":if cfg!(feature="trace") {"disabled_for_trace"} else {"unsupported_platform"}}),
    )
}
