//! Track A-specific helpers for integration tests.

use monty::{FunctionCall, MontyException, OsCall, ReplFunctionCall, ResourceTracker};

/// Asserts that two external function-call suspensions expose the same public fields.
pub fn assert_function_calls_equal<T: ResourceTracker>(left: &FunctionCall<T>, right: &FunctionCall<T>) {
    assert_eq!(left.function_name, right.function_name);
    assert_eq!(left.args, right.args);
    assert_eq!(left.kwargs, right.kwargs);
    assert_eq!(left.call_id, right.call_id);
    assert_eq!(left.method_call, right.method_call);
    assert_eq!(left.arg_runtime_ids, right.arg_runtime_ids);
    assert_eq!(left.kwarg_runtime_ids, right.kwarg_runtime_ids);
}

/// Asserts that two OS-call suspensions expose the same public fields.
pub fn assert_os_calls_equal<T: ResourceTracker>(left: &OsCall<T>, right: &OsCall<T>) {
    assert_eq!(left.function, right.function);
    assert_eq!(left.args, right.args);
    assert_eq!(left.kwargs, right.kwargs);
    assert_eq!(left.call_id, right.call_id);
    assert_eq!(left.arg_runtime_ids, right.arg_runtime_ids);
    assert_eq!(left.kwarg_runtime_ids, right.kwarg_runtime_ids);
}

/// Asserts that two REPL function-call suspensions expose the same public fields.
pub fn assert_repl_function_calls_equal<T: ResourceTracker>(left: &ReplFunctionCall<T>, right: &ReplFunctionCall<T>) {
    assert_eq!(left.function_name, right.function_name);
    assert_eq!(left.args, right.args);
    assert_eq!(left.kwargs, right.kwargs);
    assert_eq!(left.call_id, right.call_id);
    assert_eq!(left.method_call, right.method_call);
    assert_eq!(left.arg_runtime_ids, right.arg_runtime_ids);
    assert_eq!(left.kwarg_runtime_ids, right.kwarg_runtime_ids);
}

/// Asserts that two exceptions expose the same observable type and message.
pub fn assert_exceptions_equal(left: &MontyException, right: &MontyException) {
    assert_eq!(left.exc_type(), right.exc_type());
    assert_eq!(left.message(), right.message());
    assert_eq!(left.to_string(), right.to_string());
}
