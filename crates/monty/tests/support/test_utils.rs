//! Shared helpers for Track A integration tests.
//!
//! Each integration test crate compiles this shared helper module independently,
//! so some helpers are intentionally unused in a given crate.

use monty::{
    FunctionCall, MontyException, MontyObject, OsCall, OsFunction, ReplFunctionCall, ReplOsCall, ResourceTracker,
    RuntimeValueId,
};

trait FunctionCallFields {
    fn function_name(&self) -> &String;
    fn args(&self) -> &Vec<MontyObject>;
    fn kwargs(&self) -> &Vec<(MontyObject, MontyObject)>;
    fn call_id(&self) -> &u32;
    fn method_call(&self) -> &bool;
    fn arg_runtime_ids(&self) -> &Vec<RuntimeValueId>;
    fn kwarg_runtime_ids(&self) -> &Vec<(RuntimeValueId, RuntimeValueId)>;
}

impl<T: ResourceTracker> FunctionCallFields for FunctionCall<T> {
    fn function_name(&self) -> &String {
        &self.function_name
    }

    fn args(&self) -> &Vec<MontyObject> {
        &self.args
    }

    fn kwargs(&self) -> &Vec<(MontyObject, MontyObject)> {
        &self.kwargs
    }

    fn call_id(&self) -> &u32 {
        &self.call_id
    }

    fn method_call(&self) -> &bool {
        &self.method_call
    }

    fn arg_runtime_ids(&self) -> &Vec<RuntimeValueId> {
        &self.arg_runtime_ids
    }

    fn kwarg_runtime_ids(&self) -> &Vec<(RuntimeValueId, RuntimeValueId)> {
        &self.kwarg_runtime_ids
    }
}

impl<T: ResourceTracker> FunctionCallFields for ReplFunctionCall<T> {
    fn function_name(&self) -> &String {
        &self.function_name
    }

    fn args(&self) -> &Vec<MontyObject> {
        &self.args
    }

    fn kwargs(&self) -> &Vec<(MontyObject, MontyObject)> {
        &self.kwargs
    }

    fn call_id(&self) -> &u32 {
        &self.call_id
    }

    fn method_call(&self) -> &bool {
        &self.method_call
    }

    fn arg_runtime_ids(&self) -> &Vec<RuntimeValueId> {
        &self.arg_runtime_ids
    }

    fn kwarg_runtime_ids(&self) -> &Vec<(RuntimeValueId, RuntimeValueId)> {
        &self.kwarg_runtime_ids
    }
}

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
    fn from_call(call: &'a impl FunctionCallFields) -> Self {
        Self {
            function_name: call.function_name(),
            args: call.args(),
            kwargs: call.kwargs(),
            call_id: call.call_id(),
            method_call: call.method_call(),
            arg_runtime_ids: call.arg_runtime_ids(),
            kwarg_runtime_ids: call.kwarg_runtime_ids(),
        }
    }
}

/// Asserts that two external function-call suspensions expose the same public fields.
#[expect(
    private_bounds,
    reason = "shared helpers stay module-exported for sibling integration tests while the field access traits remain private"
)]
pub fn assert_function_calls_equal(left: &impl FunctionCallFields, right: &impl FunctionCallFields) {
    assert_eq!(FunctionCallKey::from_call(left), FunctionCallKey::from_call(right));
}

trait OsCallFields {
    fn function(&self) -> &OsFunction;
    fn args(&self) -> &Vec<MontyObject>;
    fn kwargs(&self) -> &Vec<(MontyObject, MontyObject)>;
    fn call_id(&self) -> &u32;
    fn arg_runtime_ids(&self) -> &Vec<RuntimeValueId>;
    fn kwarg_runtime_ids(&self) -> &Vec<(RuntimeValueId, RuntimeValueId)>;
}

impl<T: ResourceTracker> OsCallFields for OsCall<T> {
    fn function(&self) -> &OsFunction {
        &self.function
    }

    fn args(&self) -> &Vec<MontyObject> {
        &self.args
    }

    fn kwargs(&self) -> &Vec<(MontyObject, MontyObject)> {
        &self.kwargs
    }

    fn call_id(&self) -> &u32 {
        &self.call_id
    }

    fn arg_runtime_ids(&self) -> &Vec<RuntimeValueId> {
        &self.arg_runtime_ids
    }

    fn kwarg_runtime_ids(&self) -> &Vec<(RuntimeValueId, RuntimeValueId)> {
        &self.kwarg_runtime_ids
    }
}

impl<T: ResourceTracker> OsCallFields for ReplOsCall<T> {
    fn function(&self) -> &OsFunction {
        &self.function
    }

    fn args(&self) -> &Vec<MontyObject> {
        &self.args
    }

    fn kwargs(&self) -> &Vec<(MontyObject, MontyObject)> {
        &self.kwargs
    }

    fn call_id(&self) -> &u32 {
        &self.call_id
    }

    fn arg_runtime_ids(&self) -> &Vec<RuntimeValueId> {
        &self.arg_runtime_ids
    }

    fn kwarg_runtime_ids(&self) -> &Vec<(RuntimeValueId, RuntimeValueId)> {
        &self.kwarg_runtime_ids
    }
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
    fn from_call(call: &'a impl OsCallFields) -> Self {
        Self {
            function: call.function(),
            args: call.args(),
            kwargs: call.kwargs(),
            call_id: call.call_id(),
            arg_runtime_ids: call.arg_runtime_ids(),
            kwarg_runtime_ids: call.kwarg_runtime_ids(),
        }
    }
}

/// Asserts that two OS-call suspensions expose the same public fields.
#[expect(
    private_bounds,
    reason = "shared helpers stay module-exported for sibling integration tests while the field access traits remain private"
)]
pub fn assert_os_calls_equal(left: &impl OsCallFields, right: &impl OsCallFields) {
    assert_eq!(OsCallKey::from_call(left), OsCallKey::from_call(right));
}

/// Asserts that two exceptions expose the same observable type and message.
pub fn assert_exceptions_equal(left: &MontyException, right: &MontyException) {
    assert_eq!(left.exc_type(), right.exc_type());
    assert_eq!(left.message(), right.message());
    assert_eq!(left.to_string(), right.to_string());
}
