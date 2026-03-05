//! Shared helpers for snapshot-extension integration tests.

use monty::{
    ExtFunctionResult, MontyObject, MontyRepl, MontyRun, NoLimitTracker, PrintWriter, ReplProgress, RunProgress,
    SnapshotExtension,
};

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

/// Attaches snapshot-extension bytes to a run progress value when it carries a snapshot.
pub fn attach_run_snapshot_extension(
    progress: RunProgress<NoLimitTracker>,
    snapshot_extension: Vec<u8>,
) -> RunProgress<NoLimitTracker> {
    match progress {
        RunProgress::FunctionCall(call) => RunProgress::FunctionCall(call.with_snapshot_extension(snapshot_extension)),
        RunProgress::OsCall(call) => RunProgress::OsCall(call.with_snapshot_extension(snapshot_extension)),
        RunProgress::ResolveFutures(state) => {
            RunProgress::ResolveFutures(state.with_snapshot_extension(snapshot_extension))
        }
        RunProgress::NameLookup(lookup) => RunProgress::NameLookup(lookup.with_snapshot_extension(snapshot_extension)),
        RunProgress::Complete(value) => RunProgress::Complete(value),
    }
}

/// Attaches snapshot-extension bytes to a REPL progress value when it carries a snapshot.
pub fn attach_repl_snapshot_extension(
    progress: ReplProgress<NoLimitTracker>,
    snapshot_extension: Vec<u8>,
) -> ReplProgress<NoLimitTracker> {
    match progress {
        ReplProgress::FunctionCall(call) => {
            ReplProgress::FunctionCall(call.with_snapshot_extension(snapshot_extension))
        }
        ReplProgress::OsCall(call) => ReplProgress::OsCall(call.with_snapshot_extension(snapshot_extension)),
        ReplProgress::ResolveFutures(state) => {
            ReplProgress::ResolveFutures(state.with_snapshot_extension(snapshot_extension))
        }
        ReplProgress::NameLookup(lookup) => {
            ReplProgress::NameLookup(lookup.with_snapshot_extension(snapshot_extension))
        }
        ReplProgress::Complete { repl, value } => ReplProgress::Complete { repl, value },
    }
}

/// Reads snapshot-extension bytes from a run progress value when available.
pub fn run_progress_snapshot_extension(progress: &RunProgress<NoLimitTracker>) -> Option<&[u8]> {
    match progress {
        RunProgress::FunctionCall(call) => call.snapshot_extension().map(SnapshotExtension::as_slice),
        RunProgress::OsCall(call) => call.snapshot_extension().map(SnapshotExtension::as_slice),
        RunProgress::ResolveFutures(state) => state.snapshot_extension().map(SnapshotExtension::as_slice),
        RunProgress::NameLookup(lookup) => lookup.snapshot_extension().map(SnapshotExtension::as_slice),
        RunProgress::Complete(_) => None,
    }
}

/// Reads snapshot-extension bytes from a REPL progress value when available.
pub fn repl_progress_snapshot_extension(progress: &ReplProgress<NoLimitTracker>) -> Option<&[u8]> {
    match progress {
        ReplProgress::FunctionCall(call) => call.snapshot_extension().map(SnapshotExtension::as_slice),
        ReplProgress::OsCall(call) => call.snapshot_extension().map(SnapshotExtension::as_slice),
        ReplProgress::ResolveFutures(state) => state.snapshot_extension().map(SnapshotExtension::as_slice),
        ReplProgress::NameLookup(lookup) => lookup.snapshot_extension().map(SnapshotExtension::as_slice),
        ReplProgress::Complete { .. } => None,
    }
}

/// Drives a run progress value until it reaches `ResolveFutures`.
pub fn drive_to_resolve_futures(mut progress: RunProgress<NoLimitTracker>) -> RunProgress<NoLimitTracker> {
    loop {
        match progress {
            RunProgress::FunctionCall(call) => {
                progress = call
                    .resume_pending(&mut PrintWriter::Stdout)
                    .expect("run_pending should succeed");
            }
            RunProgress::ResolveFutures(_) => return progress,
            RunProgress::OsCall(call) => panic!("unexpected OsCall: {:?}", call.function),
            RunProgress::NameLookup(lookup) => panic!("unexpected NameLookup: {}", lookup.name),
            RunProgress::Complete(_) => panic!("unexpected Complete before ResolveFutures"),
        }
    }
}

/// Drives a REPL progress value until it reaches `ResolveFutures`.
pub fn drive_repl_to_resolve_futures(mut progress: ReplProgress<NoLimitTracker>) -> ReplProgress<NoLimitTracker> {
    loop {
        match progress {
            ReplProgress::FunctionCall(call) => {
                progress = call
                    .resume_pending(&mut PrintWriter::Stdout)
                    .expect("run_pending should succeed");
            }
            ReplProgress::ResolveFutures(_) => return progress,
            ReplProgress::OsCall(call) => panic!("unexpected OsCall: {:?}", call.function),
            ReplProgress::NameLookup(lookup) => panic!("unexpected NameLookup: {}", lookup.name),
            ReplProgress::Complete { .. } => panic!("unexpected Complete before ResolveFutures"),
        }
    }
}

/// Completes a `ResolveFutures` run progress with a repeated return value.
pub fn complete_resolve_futures(
    progress: RunProgress<NoLimitTracker>,
    return_value: &MontyObject,
) -> RunProgress<NoLimitTracker> {
    let RunProgress::ResolveFutures(state) = progress else {
        panic!("expected resolve futures progress");
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

/// Completes a `ResolveFutures` REPL progress with a repeated return value.
pub fn complete_repl_resolve_futures(
    progress: ReplProgress<NoLimitTracker>,
    return_value: &MontyObject,
) -> ReplProgress<NoLimitTracker> {
    let ReplProgress::ResolveFutures(state) = progress else {
        panic!("expected resolve futures progress");
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
