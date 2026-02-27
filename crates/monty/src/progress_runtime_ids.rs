//! Shared helpers for exposing runtime-ID slices from progress enums.

use crate::runtime_id::RuntimeValueId;

pub(crate) type RuntimeIdSlices<'a> = (&'a [RuntimeValueId], &'a [(RuntimeValueId, RuntimeValueId)]);

macro_rules! progress_runtime_ids {
    ($progress:expr) => {
        match $progress {
            Self::FunctionCall {
                arg_runtime_ids,
                kwarg_runtime_ids,
                ..
            }
            | Self::OsCall {
                arg_runtime_ids,
                kwarg_runtime_ids,
                ..
            } => Some((arg_runtime_ids.as_slice(), kwarg_runtime_ids.as_slice())),
            _ => None,
        }
    };
}

pub(crate) use progress_runtime_ids;
