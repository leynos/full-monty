//! Tests for snapshot extension byte round-trips.

#[path = "support/snapshot_test_utils.rs"]
mod snapshot_test_utils;

use monty::{MontyObject, NoLimitTracker, ReplProgress, RunProgress};
use rstest::{fixture, rstest};
use snapshot_test_utils::{
    SnapshotBehavior, SnapshotProgressVariant, attach_repl_snapshot_extension, attach_run_snapshot_extension,
    complete_repl_resolve_futures, complete_resolve_futures, create_repl_progress_for_variant,
    create_run_progress_for_variant, repl_progress_snapshot_extension, run_progress_snapshot_extension,
};

/// Shared snapshot extension payload used by round-trip tests.
#[fixture]
fn snapshot_extension() -> Vec<u8> {
    vec![1, 2, 3, 4]
}

/// Maps a run progress variant to its expected snapshot-extension visibility.
fn run_variant_case(variant: SnapshotProgressVariant) -> (SnapshotProgressVariant, SnapshotBehavior) {
    (
        variant,
        if variant == SnapshotProgressVariant::Complete {
            SnapshotBehavior::Absent
        } else {
            SnapshotBehavior::Preserved
        },
    )
}

/// Maps a REPL progress variant to its expected snapshot-extension visibility.
fn repl_variant_case(variant: SnapshotProgressVariant) -> (SnapshotProgressVariant, SnapshotBehavior) {
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

#[rstest]
#[case::function_call(SnapshotProgressVariant::FunctionCall)]
#[case::os_call(SnapshotProgressVariant::OsCall)]
#[case::resolve_futures(SnapshotProgressVariant::ResolveFutures)]
#[case::complete(SnapshotProgressVariant::Complete)]
fn run_progress_snapshot_extension_round_trips(#[case] variant: SnapshotProgressVariant, snapshot_extension: Vec<u8>) {
    let (fixture_variant, expected_behavior) = run_variant_case(variant);
    assert_eq!(fixture_variant, variant, "fixture should describe the active variant");

    let progress = create_run_progress_for_variant(variant);
    let progress = attach_run_snapshot_extension(progress, snapshot_extension.clone());
    let bytes = progress.dump().expect("run progress dump should succeed");
    let loaded: RunProgress<NoLimitTracker> = RunProgress::load(&bytes).expect("run progress load should succeed");

    assert_snapshot_behavior(
        run_progress_snapshot_extension(&loaded),
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

    assert_snapshot_behavior(run_progress_snapshot_extension(&loaded), &[], SnapshotBehavior::Absent);

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
    let (fixture_variant, expected_behavior) = repl_variant_case(variant);
    assert_eq!(fixture_variant, variant, "fixture should describe the active variant");

    let progress = create_repl_progress_for_variant(variant);
    let progress = attach_repl_snapshot_extension(progress, snapshot_extension.clone());
    let bytes = progress.dump().expect("repl progress dump should succeed");
    let loaded: ReplProgress<NoLimitTracker> = ReplProgress::load(&bytes).expect("repl progress load should succeed");

    assert_snapshot_behavior(
        repl_progress_snapshot_extension(&loaded),
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
    let progress = attach_run_snapshot_extension(progress, snapshot_extension);
    let mut bytes = progress.dump().expect("run progress dump should succeed");

    bytes.pop();

    assert!(RunProgress::<NoLimitTracker>::load(&bytes).is_err());
}
