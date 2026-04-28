//! Structured logging that respects the runner's debug toggle.
//!
//! GitHub Actions runners set `RUNNER_DEBUG=1` when the consumer
//! enables debug logging on a workflow run. This module respects that
//! signal — calls to [`debug`] only emit when debug is on, regular
//! [`info`] / [`warn`] / [`error`] always emit.
//!
//! Output uses GitHub's `::debug::` workflow command for debug lines so
//! the runner UI groups them under the "Debug" section of the run log.

use std::sync::atomic::{AtomicBool, Ordering};

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);
static DEBUG_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Initialize the logger. Reads `RUNNER_DEBUG` once and caches the
/// boolean. Cheap to call multiple times. If not called explicitly,
/// debug detection happens lazily on first [`debug`] call.
pub fn init() {
    if DEBUG_INITIALIZED.swap(true, Ordering::Relaxed) {
        return;
    }
    let enabled = std::env::var("RUNNER_DEBUG").is_ok_and(|v| v == "1");
    DEBUG_ENABLED.store(enabled, Ordering::Relaxed);
}

fn ensure_init() {
    if !DEBUG_INITIALIZED.load(Ordering::Relaxed) {
        init();
    }
}

/// Returns true if debug logging is on (RUNNER_DEBUG=1 at init time).
pub fn is_debug() -> bool {
    ensure_init();
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

/// Override the debug toggle. Used by tests + by binaries that want
/// a `--verbose` flag to force debug regardless of the runner env.
pub fn set_debug(enabled: bool) {
    DEBUG_INITIALIZED.store(true, Ordering::Relaxed);
    DEBUG_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Debug-level log line. Emits only when debug is enabled. Format
/// matches GitHub's `::debug::` workflow command so the runner UI
/// groups them.
pub fn debug(message: &str) {
    if !is_debug() {
        return;
    }
    println!("::debug::{}", encode(message));
}

/// Info-level log line. Always emitted; plain stdout (no workflow
/// command prefix — the runner shows it as a regular log line).
pub fn info(message: &str) {
    println!("{message}");
}

/// Warning-level log line — emits as `::warning::` so the runner
/// flags it in the run UI annotations.
pub fn warn(message: &str) {
    println!("::warning::{}", encode(message));
}

/// Error-level log line — emits as `::error::`. The action's process
/// should typically also exit non-zero after emitting; this helper
/// alone does not exit (callers control flow).
pub fn error(message: &str) {
    println!("::error::{}", encode(message));
}

/// Encode a message for the workflow-command format (escape newlines
/// + percent-signs).
fn encode(s: &str) -> String {
    s.replace('%', "%25").replace('\n', "%0A").replace('\r', "%0D")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests that touch the global debug toggle must run serially —
    /// guarded by this lock.
    static SERIALIZE: Mutex<()> = Mutex::new(());

    #[test]
    fn set_debug_overrides_env() {
        let _g = SERIALIZE.lock().unwrap();
        set_debug(true);
        assert!(is_debug());
        set_debug(false);
        assert!(!is_debug());
    }

    #[test]
    fn encode_escapes_newlines_and_percent() {
        assert_eq!(encode("100% sure"), "100%25 sure");
        assert_eq!(encode("line one\nline two"), "line one%0Aline two");
        assert_eq!(
            encode("crlf\r\n"),
            // %25 first (so existing %0A in input would be encoded as %250A)
            "crlf%0D%0A"
        );
    }
}
