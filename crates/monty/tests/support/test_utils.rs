//! Shared helpers for runtime-observer integration tests.

use monty::{ResourceTracker, RunProgress};

/// Extracts a function-call progress variant with a contextual panic message.
///
/// # Panics
/// Panics when `progress` is not `RunProgress::FunctionCall`.
pub fn as_function_call<T: ResourceTracker>(progress: RunProgress<T>, context: &str) -> monty::FunctionCall<T> {
    match progress {
        RunProgress::FunctionCall(call) => call,
        other => panic!("{context}: expected function-call progress, got {other:?}"),
    }
}

/// Extracts an OS-call progress variant with a contextual panic message.
///
/// # Panics
/// Panics when `progress` is not `RunProgress::OsCall`.
pub fn as_os_call<T: ResourceTracker>(progress: RunProgress<T>, context: &str) -> monty::OsCall<T> {
    match progress {
        RunProgress::OsCall(call) => call,
        other => panic!("{context}: expected OS-call progress, got {other:?}"),
    }
}
