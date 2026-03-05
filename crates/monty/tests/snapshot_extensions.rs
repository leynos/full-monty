//! Tests for snapshot extension byte round-trips.

use monty::{
    ExtFunctionResult, MontyObject, NoLimitTracker, PrintWriter, ReplProgress, RunProgress, SnapshotExtension,
};
use rstest::{fixture, rstest};
use snapshot_test_utils::{SnapshotBehavior, SnapshotProgressVariant, create_repl, create_run_progress};

#[expect(
    dead_code,
    reason = "snapshot extension tests share a support module but exercise only a subset locally"
)]
#[path = "support/snapshot_test_utils.rs"]
mod snapshot_test_utils;

trait ProgressSnapshotExt: Sized {
    fn attach_snapshot_extension(self, ext: Vec<u8>) -> Self;
    fn get_snapshot_extension(&self) -> Option<&[u8]>;
}

impl ProgressSnapshotExt for RunProgress<NoLimitTracker> {
    fn attach_snapshot_extension(self, snapshot_extension: Vec<u8>) -> Self {
        match self {
            Self::FunctionCall(call) => Self::FunctionCall(call.with_snapshot_extension(snapshot_extension)),
            Self::OsCall(call) => Self::OsCall(call.with_snapshot_extension(snapshot_extension)),
            Self::ResolveFutures(s) => Self::ResolveFutures(s.with_snapshot_extension(snapshot_extension)),
            Self::NameLookup(l) => Self::NameLookup(l.with_snapshot_extension(snapshot_extension)),
            Self::Complete(v) => Self::Complete(v),
        }
    }

    fn get_snapshot_extension(&self) -> Option<&[u8]> {
        match self {
            Self::FunctionCall(call) => call.snapshot_extension().map(SnapshotExtension::as_slice),
            Self::OsCall(call) => call.snapshot_extension().map(SnapshotExtension::as_slice),
            Self::ResolveFutures(s) => s.snapshot_extension().map(SnapshotExtension::as_slice),
            Self::NameLookup(l) => l.snapshot_extension().map(SnapshotExtension::as_slice),
            Self::Complete(_) => None,
        }
    }
}

impl ProgressSnapshotExt for ReplProgress<NoLimitTracker> {
    fn attach_snapshot_extension(self, snapshot_extension: Vec<u8>) -> Self {
        match self {
            Self::FunctionCall(call) => Self::FunctionCall(call.with_snapshot_extension(snapshot_extension)),
            Self::OsCall(call) => Self::OsCall(call.with_snapshot_extension(snapshot_extension)),
            Self::ResolveFutures(s) => Self::ResolveFutures(s.with_snapshot_extension(snapshot_extension)),
            Self::NameLookup(l) => Self::NameLookup(l.with_snapshot_extension(snapshot_extension)),
            Self::Complete { repl, value } => Self::Complete { repl, value },
        }
    }

    fn get_snapshot_extension(&self) -> Option<&[u8]> {
        match self {
            Self::FunctionCall(call) => call.snapshot_extension().map(SnapshotExtension::as_slice),
            Self::OsCall(call) => call.snapshot_extension().map(SnapshotExtension::as_slice),
            Self::ResolveFutures(s) => s.snapshot_extension().map(SnapshotExtension::as_slice),
            Self::NameLookup(l) => l.snapshot_extension().map(SnapshotExtension::as_slice),
            Self::Complete { .. } => None,
        }
    }
}

macro_rules! impl_complete_resolve_futures {
    ($fn_name:ident, $Progress:ident) => {
        fn $fn_name(progress: $Progress<NoLimitTracker>, return_value: &MontyObject) -> $Progress<NoLimitTracker> {
            let $Progress::ResolveFutures(state) = progress else {
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
    };
}

impl_complete_resolve_futures!(complete_resolve_futures, RunProgress);
impl_complete_resolve_futures!(complete_repl_resolve_futures, ReplProgress);

const EXTERNAL_CALL_SCRIPT: &str = "ext_fn([])";
const OS_CALL_SCRIPT: &str = "from pathlib import Path; Path('/tmp/test.txt').exists()";
const COMPLETE_SCRIPT: &str = "1 + 2";
const RESOLVE_FUTURES_SCRIPT: &str = r"
import asyncio

async def main():
    return await foo()

await main()
";

/// Shared snapshot extension payload used by round-trip tests.
#[fixture]
fn snapshot_extension() -> Vec<u8> {
    vec![1, 2, 3, 4]
}

/// Maps a progress variant to its expected snapshot-extension visibility.
fn variant_case(variant: SnapshotProgressVariant) -> (SnapshotProgressVariant, SnapshotBehavior) {
    (
        variant,
        if variant == SnapshotProgressVariant::Complete {
            SnapshotBehavior::Absent
        } else {
            SnapshotBehavior::Preserved
        },
    )
}

/// Asserts the observed snapshot bytes match the expected visibility.
fn assert_snapshot_behavior(actual: Option<&[u8]>, snapshot_extension: &[u8], expected: SnapshotBehavior) {
    match expected {
        SnapshotBehavior::Preserved => {
            assert_eq!(
                actual,
                Some(snapshot_extension),
                "expected snapshot extension bytes to round-trip"
            );
        }
        SnapshotBehavior::Absent => {
            assert!(actual.is_none(), "expected no visible snapshot extension");
        }
    }
}

macro_rules! impl_drive_to_resolve_futures {
    ($fn_name:ident, $Progress:ident) => {
        fn $fn_name(mut progress: $Progress<NoLimitTracker>) -> $Progress<NoLimitTracker> {
            loop {
                match progress {
                    $Progress::FunctionCall(call) => {
                        progress = call
                            .resume_pending(&mut PrintWriter::Stdout)
                            .expect("run_pending should succeed");
                    }
                    $Progress::ResolveFutures(_) => return progress,
                    $Progress::OsCall(call) => {
                        panic!("unexpected OsCall: {:?}", call.function)
                    }
                    $Progress::NameLookup(lookup) => {
                        panic!("unexpected NameLookup: {}", lookup.name)
                    }
                    _ => panic!("unexpected Complete before ResolveFutures"),
                }
            }
        }
    };
}

impl_drive_to_resolve_futures!(drive_to_resolve_futures, RunProgress);
impl_drive_to_resolve_futures!(drive_repl_to_resolve_futures, ReplProgress);

fn create_run_progress_for_variant(variant: SnapshotProgressVariant) -> RunProgress<NoLimitTracker> {
    match variant {
        SnapshotProgressVariant::FunctionCall => create_run_progress(EXTERNAL_CALL_SCRIPT),
        SnapshotProgressVariant::OsCall => create_run_progress(OS_CALL_SCRIPT),
        SnapshotProgressVariant::ResolveFutures => {
            drive_to_resolve_futures(create_run_progress(RESOLVE_FUTURES_SCRIPT))
        }
        SnapshotProgressVariant::Complete => create_run_progress(COMPLETE_SCRIPT),
    }
}

fn create_repl_progress_for_variant(variant: SnapshotProgressVariant) -> ReplProgress<NoLimitTracker> {
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

#[rstest]
#[case::function_call(SnapshotProgressVariant::FunctionCall)]
#[case::os_call(SnapshotProgressVariant::OsCall)]
#[case::resolve_futures(SnapshotProgressVariant::ResolveFutures)]
#[case::complete(SnapshotProgressVariant::Complete)]
fn run_progress_snapshot_extension_round_trips(#[case] variant: SnapshotProgressVariant, snapshot_extension: Vec<u8>) {
    let (fixture_variant, expected_behavior) = variant_case(variant);
    assert_eq!(fixture_variant, variant, "fixture should describe the active variant");

    let progress = create_run_progress_for_variant(variant);
    let progress = progress.attach_snapshot_extension(snapshot_extension.clone());
    let bytes = progress.dump().expect("run progress dump should succeed");
    let loaded: RunProgress<NoLimitTracker> = RunProgress::load(&bytes).expect("run progress load should succeed");

    assert_snapshot_behavior(
        loaded.get_snapshot_extension(),
        snapshot_extension.as_slice(),
        expected_behavior,
    );

    if variant == SnapshotProgressVariant::ResolveFutures {
        let completed = complete_resolve_futures(progress, &MontyObject::Int(1));
        assert_eq!(
            completed
                .into_complete()
                .expect("expected completion after resolving futures"),
            MontyObject::Int(1)
        );

        let completed_loaded = complete_resolve_futures(loaded, &MontyObject::Int(1));
        assert_eq!(
            completed_loaded
                .into_complete()
                .expect("expected loaded completion after resolving futures"),
            MontyObject::Int(1)
        );
    }
}

#[rstest]
#[case::function_call(SnapshotProgressVariant::FunctionCall)]
#[case::os_call(SnapshotProgressVariant::OsCall)]
#[case::resolve_futures(SnapshotProgressVariant::ResolveFutures)]
#[case::complete(SnapshotProgressVariant::Complete)]
fn run_progress_snapshot_extension_defaults_to_none(#[case] variant: SnapshotProgressVariant) {
    let progress = create_run_progress_for_variant(variant);
    let bytes = progress.dump().expect("run progress dump should succeed");
    let loaded: RunProgress<NoLimitTracker> = RunProgress::load(&bytes).expect("run progress load should succeed");

    assert_snapshot_behavior(loaded.get_snapshot_extension(), &[], SnapshotBehavior::Absent);

    if variant == SnapshotProgressVariant::ResolveFutures {
        let completed = complete_resolve_futures(progress, &MontyObject::Int(1));
        assert_eq!(
            completed
                .into_complete()
                .expect("expected completion after resolving defaulted futures"),
            MontyObject::Int(1)
        );

        let completed_loaded = complete_resolve_futures(loaded, &MontyObject::Int(1));
        assert_eq!(
            completed_loaded
                .into_complete()
                .expect("expected loaded completion after resolving defaulted futures"),
            MontyObject::Int(1)
        );
    }
}

#[rstest]
#[case::function_call(SnapshotProgressVariant::FunctionCall)]
#[case::os_call(SnapshotProgressVariant::OsCall)]
#[case::resolve_futures(SnapshotProgressVariant::ResolveFutures)]
#[case::complete(SnapshotProgressVariant::Complete)]
fn repl_progress_snapshot_extension_round_trips(#[case] variant: SnapshotProgressVariant, snapshot_extension: Vec<u8>) {
    let (fixture_variant, expected_behavior) = variant_case(variant);
    assert_eq!(fixture_variant, variant, "fixture should describe the active variant");

    let progress = create_repl_progress_for_variant(variant);
    let progress = progress.attach_snapshot_extension(snapshot_extension.clone());
    let bytes = progress.dump().expect("repl progress dump should succeed");
    let loaded: ReplProgress<NoLimitTracker> = ReplProgress::load(&bytes).expect("repl progress load should succeed");

    assert_snapshot_behavior(
        loaded.get_snapshot_extension(),
        snapshot_extension.as_slice(),
        expected_behavior,
    );

    if variant == SnapshotProgressVariant::ResolveFutures {
        let completed = complete_repl_resolve_futures(progress, &MontyObject::Int(3));
        let ReplProgress::Complete { value, .. } = completed else {
            panic!("expected completion after resolving REPL futures");
        };
        assert_eq!(value, MontyObject::Int(3));

        let completed_loaded = complete_repl_resolve_futures(loaded, &MontyObject::Int(3));
        let ReplProgress::Complete { value, .. } = completed_loaded else {
            panic!("expected loaded completion after resolving REPL futures");
        };
        assert_eq!(value, MontyObject::Int(3));
    }
}

#[rstest]
#[case::function_call(SnapshotProgressVariant::FunctionCall)]
fn corrupted_run_progress_payload_fails_to_load(#[case] variant: SnapshotProgressVariant, snapshot_extension: Vec<u8>) {
    let progress = create_run_progress_for_variant(variant);
    let progress = progress.attach_snapshot_extension(snapshot_extension);
    let mut bytes = progress.dump().expect("run progress dump should succeed");

    bytes.pop();

    assert!(RunProgress::<NoLimitTracker>::load(&bytes).is_err());
}

#[test]
fn corrupted_repl_progress_payload_fails_to_load() {
    let progress = create_repl_progress_for_variant(SnapshotProgressVariant::FunctionCall);
    let progress = progress.attach_snapshot_extension(vec![9, 8, 7]);
    let mut bytes = progress.dump().expect("repl progress dump should succeed");

    bytes.pop();

    assert!(ReplProgress::<NoLimitTracker>::load(&bytes).is_err());
}
