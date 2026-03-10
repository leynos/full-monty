//! Shared helpers for Track A integration tests.
//!
//! Each integration test crate compiles this shared helper module independently,
//! so some helpers are intentionally unused in a given crate.

use monty::{
    FunctionCall, MontyException, MontyObject, OsCall, OsFunction, ReplFunctionCall, ReplOsCall, ResourceTracker,
    RuntimeValueId,
};

#[derive(Debug, PartialEq)]
struct FunctionCallKey<'a> {
    function_name: &'a String,
    args: &'a Vec<MontyObject>,
    kwargs: &'a Vec<(MontyObject, MontyObject)>,
    call_id: &'a u32,
    method_call: &'a bool,
    arg_runtime_ids: &'a Vec<RuntimeValueId>,
    kwarg_runtime_ids: &'a Vec<(RuntimeValueId, RuntimeValueId)>,
}

impl<'a> FunctionCallKey<'a> {
    fn from_call<T: ResourceTracker>(call: &'a FunctionCall<T>) -> Self {
        Self {
            function_name: &call.function_name,
            args: &call.args,
            kwargs: &call.kwargs,
            call_id: &call.call_id,
            method_call: &call.method_call,
            arg_runtime_ids: &call.arg_runtime_ids,
            kwarg_runtime_ids: &call.kwarg_runtime_ids,
        }
    }
}

/// Asserts that two external function-call suspensions expose the same public fields.
pub fn assert_function_calls_equal<T: ResourceTracker>(left: &FunctionCall<T>, right: &FunctionCall<T>) {
    assert_eq!(FunctionCallKey::from_call(left), FunctionCallKey::from_call(right));
}

#[derive(Debug, PartialEq)]
struct OsCallKey<'a> {
    function: &'a OsFunction,
    args: &'a Vec<MontyObject>,
    kwargs: &'a Vec<(MontyObject, MontyObject)>,
    call_id: &'a u32,
    arg_runtime_ids: &'a Vec<RuntimeValueId>,
    kwarg_runtime_ids: &'a Vec<(RuntimeValueId, RuntimeValueId)>,
}

impl<'a> OsCallKey<'a> {
    fn from_call<T: ResourceTracker>(call: &'a OsCall<T>) -> Self {
        Self {
            function: &call.function,
            args: &call.args,
            kwargs: &call.kwargs,
            call_id: &call.call_id,
            arg_runtime_ids: &call.arg_runtime_ids,
            kwarg_runtime_ids: &call.kwarg_runtime_ids,
        }
    }
}

/// Asserts that two OS-call suspensions expose the same public fields.
pub fn assert_os_calls_equal<T: ResourceTracker>(left: &OsCall<T>, right: &OsCall<T>) {
    assert_eq!(OsCallKey::from_call(left), OsCallKey::from_call(right));
}

#[derive(Debug, PartialEq)]
struct ReplFunctionCallKey<'a> {
    function_name: &'a String,
    args: &'a Vec<MontyObject>,
    kwargs: &'a Vec<(MontyObject, MontyObject)>,
    call_id: &'a u32,
    method_call: &'a bool,
    arg_runtime_ids: &'a Vec<RuntimeValueId>,
    kwarg_runtime_ids: &'a Vec<(RuntimeValueId, RuntimeValueId)>,
}

impl<'a> ReplFunctionCallKey<'a> {
    fn from_call<T: ResourceTracker>(call: &'a ReplFunctionCall<T>) -> Self {
        Self {
            function_name: &call.function_name,
            args: &call.args,
            kwargs: &call.kwargs,
            call_id: &call.call_id,
            method_call: &call.method_call,
            arg_runtime_ids: &call.arg_runtime_ids,
            kwarg_runtime_ids: &call.kwarg_runtime_ids,
        }
    }
}

/// Asserts that two REPL function-call suspensions expose the same public fields.
pub fn assert_repl_function_calls_equal<T: ResourceTracker>(left: &ReplFunctionCall<T>, right: &ReplFunctionCall<T>) {
    assert_eq!(ReplFunctionCallKey::from_call(left), ReplFunctionCallKey::from_call(right));
}

#[derive(Debug, PartialEq)]
struct ReplOsCallKey<'a> {
    function: &'a OsFunction,
    args: &'a Vec<MontyObject>,
    kwargs: &'a Vec<(MontyObject, MontyObject)>,
    call_id: &'a u32,
    arg_runtime_ids: &'a Vec<RuntimeValueId>,
    kwarg_runtime_ids: &'a Vec<(RuntimeValueId, RuntimeValueId)>,
}

impl<'a> ReplOsCallKey<'a> {
    fn from_call<T: ResourceTracker>(call: &'a ReplOsCall<T>) -> Self {
        Self {
            function: &call.function,
            args: &call.args,
            kwargs: &call.kwargs,
            call_id: &call.call_id,
            arg_runtime_ids: &call.arg_runtime_ids,
            kwarg_runtime_ids: &call.kwarg_runtime_ids,
        }
    }
}

/// Asserts that two REPL OS-call suspensions expose the same public fields.
#[expect(
    dead_code,
    reason = "repl OS-call equality helper remains available for future test coverage"
)]
pub fn assert_repl_os_calls_equal<T: ResourceTracker>(left: &ReplOsCall<T>, right: &ReplOsCall<T>) {
    assert_eq!(ReplOsCallKey::from_call(left), ReplOsCallKey::from_call(right));
}

/// Asserts that two exceptions expose the same observable type and message.
pub fn assert_exceptions_equal(left: &MontyException, right: &MontyException) {
    assert_eq!(left.exc_type(), right.exc_type());
    assert_eq!(left.message(), right.message());
    assert_eq!(left.to_string(), right.to_string());
}
