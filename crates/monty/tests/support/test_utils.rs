//! Shared helpers for Track A integration tests.
//!
//! Each integration test crate compiles this shared helper module independently,
//! so some helpers are intentionally unused in a given crate.
use std::fmt::Debug;

use monty::{
    FunctionCall, MontyException, OsCall, ReplFunctionCall, ReplOsCall, ResourceTracker, RuntimeValueId,
};

macro_rules! assert_fields_equal {
    ($left:expr, $right:expr; $($field:ident),+ $(,)?) => {
        $(
            assert_eq!(
                $left.$field(),
                $right.$field(),
                "field `{}` did not match",
                stringify!($field)
            );
        )+
    };
}

/// Shared view over suspendable function-call payloads used by test assertions.
pub(crate) trait FunctionCallLike {
    type Args: PartialEq + Debug;
    type Kwargs: PartialEq + Debug;
    type CallId: PartialEq + Debug;
    type MethodCall: PartialEq + Debug;
    type ArgRuntimeIds: PartialEq + Debug;
    type KwargRuntimeIds: PartialEq + Debug;

    fn function_name(&self) -> &str;
    fn args(&self) -> &Self::Args;
    fn kwargs(&self) -> &Self::Kwargs;
    fn call_id(&self) -> &Self::CallId;
    fn method_call(&self) -> &Self::MethodCall;
    fn arg_runtime_ids(&self) -> &Self::ArgRuntimeIds;
    fn kwarg_runtime_ids(&self) -> &Self::KwargRuntimeIds;
}

/// Shared view over suspendable OS-call payloads used by test assertions.
pub(crate) trait OsCallLike {
    type Args: PartialEq + Debug;
    type Kwargs: PartialEq + Debug;
    type CallId: PartialEq + Debug;
    type ArgRuntimeIds: PartialEq + Debug;
    type KwargRuntimeIds: PartialEq + Debug;

    fn function(&self) -> &str;
    fn args(&self) -> &Self::Args;
    fn kwargs(&self) -> &Self::Kwargs;
    fn call_id(&self) -> &Self::CallId;
    fn arg_runtime_ids(&self) -> &Self::ArgRuntimeIds;
    fn kwarg_runtime_ids(&self) -> &Self::KwargRuntimeIds;
}

impl<T: ResourceTracker> FunctionCallLike for FunctionCall<T> {
    type Args = Vec<monty::MontyObject>;
    type Kwargs = Vec<(monty::MontyObject, monty::MontyObject)>;
    type CallId = u32;
    type MethodCall = bool;
    type ArgRuntimeIds = Vec<RuntimeValueId>;
    type KwargRuntimeIds = Vec<(RuntimeValueId, RuntimeValueId)>;

    fn function_name(&self) -> &str {
        &self.function_name
    }

    fn args(&self) -> &Self::Args {
        &self.args
    }

    fn kwargs(&self) -> &Self::Kwargs {
        &self.kwargs
    }

    fn call_id(&self) -> &Self::CallId {
        &self.call_id
    }

    fn method_call(&self) -> &Self::MethodCall {
        &self.method_call
    }

    fn arg_runtime_ids(&self) -> &Self::ArgRuntimeIds {
        &self.arg_runtime_ids
    }

    fn kwarg_runtime_ids(&self) -> &Self::KwargRuntimeIds {
        &self.kwarg_runtime_ids
    }
}

impl<T: ResourceTracker> FunctionCallLike for ReplFunctionCall<T> {
    type Args = Vec<monty::MontyObject>;
    type Kwargs = Vec<(monty::MontyObject, monty::MontyObject)>;
    type CallId = u32;
    type MethodCall = bool;
    type ArgRuntimeIds = Vec<RuntimeValueId>;
    type KwargRuntimeIds = Vec<(RuntimeValueId, RuntimeValueId)>;

    fn function_name(&self) -> &str {
        &self.function_name
    }

    fn args(&self) -> &Self::Args {
        &self.args
    }

    fn kwargs(&self) -> &Self::Kwargs {
        &self.kwargs
    }

    fn call_id(&self) -> &Self::CallId {
        &self.call_id
    }

    fn method_call(&self) -> &Self::MethodCall {
        &self.method_call
    }

    fn arg_runtime_ids(&self) -> &Self::ArgRuntimeIds {
        &self.arg_runtime_ids
    }

    fn kwarg_runtime_ids(&self) -> &Self::KwargRuntimeIds {
        &self.kwarg_runtime_ids
    }
}

impl<T: ResourceTracker> OsCallLike for OsCall<T> {
    type Args = Vec<monty::MontyObject>;
    type Kwargs = Vec<(monty::MontyObject, monty::MontyObject)>;
    type CallId = u32;
    type ArgRuntimeIds = Vec<RuntimeValueId>;
    type KwargRuntimeIds = Vec<(RuntimeValueId, RuntimeValueId)>;

    fn function(&self) -> &str {
        os_function_name(self.function)
    }

    fn args(&self) -> &Self::Args {
        &self.args
    }

    fn kwargs(&self) -> &Self::Kwargs {
        &self.kwargs
    }

    fn call_id(&self) -> &Self::CallId {
        &self.call_id
    }

    fn arg_runtime_ids(&self) -> &Self::ArgRuntimeIds {
        &self.arg_runtime_ids
    }

    fn kwarg_runtime_ids(&self) -> &Self::KwargRuntimeIds {
        &self.kwarg_runtime_ids
    }
}

impl<T: ResourceTracker> OsCallLike for ReplOsCall<T> {
    type Args = Vec<monty::MontyObject>;
    type Kwargs = Vec<(monty::MontyObject, monty::MontyObject)>;
    type CallId = u32;
    type ArgRuntimeIds = Vec<RuntimeValueId>;
    type KwargRuntimeIds = Vec<(RuntimeValueId, RuntimeValueId)>;

    fn function(&self) -> &str {
        os_function_name(self.function)
    }

    fn args(&self) -> &Self::Args {
        &self.args
    }

    fn kwargs(&self) -> &Self::Kwargs {
        &self.kwargs
    }

    fn call_id(&self) -> &Self::CallId {
        &self.call_id
    }

    fn arg_runtime_ids(&self) -> &Self::ArgRuntimeIds {
        &self.arg_runtime_ids
    }

    fn kwarg_runtime_ids(&self) -> &Self::KwargRuntimeIds {
        &self.kwarg_runtime_ids
    }
}

/// Asserts that two function-call suspensions expose the same public fields.
pub fn assert_function_calls_equal<C: FunctionCallLike>(left: &C, right: &C) {
    assert_fields_equal!(
        left,
        right;
        function_name,
        args,
        kwargs,
        call_id,
        method_call,
        arg_runtime_ids,
        kwarg_runtime_ids
    );
}

/// Asserts that two OS-call suspensions expose the same public fields.
pub fn assert_os_calls_equal<C: OsCallLike>(left: &C, right: &C) {
    assert_fields_equal!(left, right; function, args, kwargs, call_id, arg_runtime_ids, kwarg_runtime_ids);
}

/// Asserts that two exceptions expose the same observable type and message.
pub fn assert_exceptions_equal(left: &MontyException, right: &MontyException) {
    assert_eq!(left.exc_type(), right.exc_type());
    assert_eq!(left.message(), right.message());
    assert_eq!(left.to_string(), right.to_string());
}

fn os_function_name(function: monty::OsFunction) -> &'static str {
    match function {
        monty::OsFunction::Exists => "Path.exists",
        monty::OsFunction::IsFile => "Path.is_file",
        monty::OsFunction::IsDir => "Path.is_dir",
        monty::OsFunction::IsSymlink => "Path.is_symlink",
        monty::OsFunction::ReadText => "Path.read_text",
        monty::OsFunction::ReadBytes => "Path.read_bytes",
        monty::OsFunction::WriteText => "Path.write_text",
        monty::OsFunction::WriteBytes => "Path.write_bytes",
        monty::OsFunction::Mkdir => "Path.mkdir",
        monty::OsFunction::Unlink => "Path.unlink",
        monty::OsFunction::Rmdir => "Path.rmdir",
        monty::OsFunction::Iterdir => "Path.iterdir",
        monty::OsFunction::Stat => "Path.stat",
        monty::OsFunction::Rename => "Path.rename",
        monty::OsFunction::Resolve => "Path.resolve",
        monty::OsFunction::Absolute => "Path.absolute",
        monty::OsFunction::Getenv => "os.getenv",
        monty::OsFunction::GetEnviron => "os.environ",
    }
}
