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

/// Asserts that two function-call snapshots are equivalent across all
/// externally-visible fields used by observer integration tests.
pub fn assert_function_calls_equal<T: ResourceTracker>(left: &monty::FunctionCall<T>, right: &monty::FunctionCall<T>) {
    assert_eq!(left.function_name, right.function_name);
    assert_eq!(left.args, right.args);
    assert_eq!(left.kwargs, right.kwargs);
    assert_eq!(left.call_id, right.call_id);
    assert_eq!(left.method_call, right.method_call);
    assert_eq!(left.arg_runtime_ids, right.arg_runtime_ids);
    assert_eq!(left.kwarg_runtime_ids, right.kwarg_runtime_ids);
}
