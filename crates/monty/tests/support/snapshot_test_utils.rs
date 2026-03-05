//! Shared helpers for snapshot-extension integration tests.

use monty::{
    ExtFunctionResult, MontyObject, MontyRepl, MontyRun, NoLimitTracker, PrintWriter, ReplProgress, RunProgress,
    SnapshotExtension,
};

/// Test-focused API for attaching and reading optional snapshot-extension bytes
/// on progress snapshots.
pub trait ProgressSnapshotExt: Sized {
    fn attach_snapshot_extension(self, ext: Vec<u8>) -> Self;
    fn get_snapshot_extension(&self) -> Option<&[u8]>;
}

/// Generates `ProgressSnapshotExt` impls for progress enums that differ only in
/// the shape of their complete variant.
macro_rules! impl_progress_snapshot_ext {
    ($Progress:ident, $complete_pat:pat => $complete_expr:expr, $complete_get_pat:pat) => {
        impl ProgressSnapshotExt for $Progress<NoLimitTracker> {
            fn attach_snapshot_extension(self, snapshot_extension: Vec<u8>) -> Self {
                match self {
                    Self::FunctionCall(call) => Self::FunctionCall(call.with_snapshot_extension(snapshot_extension)),
                    Self::OsCall(call) => Self::OsCall(call.with_snapshot_extension(snapshot_extension)),
                    Self::ResolveFutures(state) => {
                        Self::ResolveFutures(state.with_snapshot_extension(snapshot_extension))
                    }
                    Self::NameLookup(lookup) => Self::NameLookup(lookup.with_snapshot_extension(snapshot_extension)),
                    $complete_pat => $complete_expr,
                }
            }

            fn get_snapshot_extension(&self) -> Option<&[u8]> {
                match self {
                    Self::FunctionCall(call) => call.snapshot_extension().map(SnapshotExtension::as_slice),
                    Self::OsCall(call) => call.snapshot_extension().map(SnapshotExtension::as_slice),
                    Self::ResolveFutures(state) => state.snapshot_extension().map(SnapshotExtension::as_slice),
                    Self::NameLookup(lookup) => lookup.snapshot_extension().map(SnapshotExtension::as_slice),
                    $complete_get_pat => None,
                }
            }
        }
    };
}

impl_progress_snapshot_ext!(
    RunProgress,
    Self::Complete(value) => Self::Complete(value),
    Self::Complete(_)
);
impl_progress_snapshot_ext!(
    ReplProgress,
    Self::Complete { repl, value } => Self::Complete { repl, value },
    Self::Complete { .. }
);

/// Progress variants relevant to snapshot-extension round-trip coverage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotProgressVariant {
    /// Suspension on a host function call.
    FunctionCall,
    /// Suspension on a host OS interaction.
    OsCall,
    /// Suspension while awaiting unresolved external futures.
    ResolveFutures,
    /// Completed execution with no suspension snapshot to decorate.
    Complete,
}

/// Expected snapshot-extension visibility for a progress variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotBehavior {
    /// The snapshot extension should be present and match the attached bytes.
    Preserved,
    /// The progress value has no suspendable snapshot, so no extension is visible.
    Absent,
}

const EXTERNAL_CALL_SCRIPT: &str = "ext_fn([])";
const OS_CALL_SCRIPT: &str = "from pathlib import Path; Path('/tmp/test.txt').exists()";
const COMPLETE_SCRIPT: &str = "1 + 2";
const RESOLVE_FUTURES_SCRIPT: &str = r"
import asyncio

async def main():
    return await foo()

await main()
";

/// Creates a suspendable `RunProgress` from Python source.
pub fn create_run_progress(script: &str) -> RunProgress<NoLimitTracker> {
    let runner = MontyRun::new(script.to_owned(), "test.py", vec![]).expect("runner creation should succeed");
    runner
        .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
        .expect("run should produce progress")
}

/// Creates a reusable REPL instance for snapshot-extension tests.
pub fn create_repl() -> MontyRepl<NoLimitTracker> {
    let (repl, _result) = MontyRepl::new(
        "pass".to_owned(),
        "init.py",
        vec![],
        vec![],
        NoLimitTracker,
        &mut PrintWriter::Stdout,
    )
    .expect("repl creation should succeed");
    repl
}

/// Creates a `RunProgress` for the requested variant.
pub fn create_run_progress_for_variant(variant: SnapshotProgressVariant) -> RunProgress<NoLimitTracker> {
    match variant {
        SnapshotProgressVariant::FunctionCall => create_run_progress(EXTERNAL_CALL_SCRIPT),
        SnapshotProgressVariant::OsCall => create_run_progress(OS_CALL_SCRIPT),
        SnapshotProgressVariant::ResolveFutures => {
            drive_to_resolve_futures(create_run_progress(RESOLVE_FUTURES_SCRIPT))
        }
        SnapshotProgressVariant::Complete => {
            let runner =
                MontyRun::new(COMPLETE_SCRIPT.to_owned(), "test.py", vec![]).expect("runner creation should succeed");
            runner
                .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
                .expect("run should complete")
        }
    }
}

/// Creates a `ReplProgress` for the requested variant.
pub fn create_repl_progress_for_variant(variant: SnapshotProgressVariant) -> ReplProgress<NoLimitTracker> {
    let repl = create_repl();
    let snippet = match variant {
        SnapshotProgressVariant::FunctionCall => EXTERNAL_CALL_SCRIPT,
        SnapshotProgressVariant::OsCall => OS_CALL_SCRIPT,
        SnapshotProgressVariant::ResolveFutures => RESOLVE_FUTURES_SCRIPT,
        SnapshotProgressVariant::Complete => COMPLETE_SCRIPT,
    };
    let progress = repl
        .start(snippet, &mut PrintWriter::Stdout)
        .expect("repl should produce progress");
    if variant == SnapshotProgressVariant::ResolveFutures {
        drive_repl_to_resolve_futures(progress)
    } else {
        progress
    }
}

/// Generates helpers that drive progress values forward until they suspend on
/// `ResolveFutures`.
macro_rules! impl_drive_to_resolve_futures {
    ($fn_name:ident, $Progress:ident) => {
        #[doc = "Drives a progress value forward until it reaches `ResolveFutures`."]
        pub fn $fn_name(mut progress: $Progress<NoLimitTracker>) -> $Progress<NoLimitTracker> {
            loop {
                match progress {
                    $Progress::FunctionCall(call) => {
                        progress = call
                            .resume_pending(&mut PrintWriter::Stdout)
                            .expect("resume_pending should succeed");
                    }
                    $Progress::ResolveFutures(_) => return progress,
                    $Progress::OsCall(call) => panic!("unexpected OsCall: {:?}", call.function),
                    $Progress::NameLookup(lookup) => panic!("unexpected NameLookup: {}", lookup.name),
                    _ => panic!("unexpected Complete before ResolveFutures"),
                }
            }
        }
    };
}

impl_drive_to_resolve_futures!(drive_to_resolve_futures, RunProgress);
impl_drive_to_resolve_futures!(drive_repl_to_resolve_futures, ReplProgress);

/// Generates helpers that resume `ResolveFutures` progress values with the same
/// return value for every pending call.
macro_rules! impl_complete_resolve_futures {
    ($fn_name:ident, $Progress:ident) => {
        #[doc = "Completes a `ResolveFutures` progress value with a repeated return value."]
        pub fn $fn_name(progress: $Progress<NoLimitTracker>, return_value: &MontyObject) -> $Progress<NoLimitTracker> {
            let $Progress::ResolveFutures(state) = progress else {
                panic!("expected ResolveFutures progress");
            };
            let results = state
                .pending_call_ids()
                .iter()
                .map(|call_id| (*call_id, ExtFunctionResult::Return(return_value.clone())))
                .collect();
            state
                .resume(results, &mut PrintWriter::Stdout)
                .expect("resume should succeed")
        }
    };
}

impl_complete_resolve_futures!(complete_resolve_futures, RunProgress);
impl_complete_resolve_futures!(complete_repl_resolve_futures, ReplProgress);
