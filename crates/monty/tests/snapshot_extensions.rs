//! Tests for snapshot extension byte round-trips.

use monty::{MontyObject, NoLimitTracker, ReplProgress, RunProgress};
use rstest::{fixture, rstest};
use snapshot_test_utils::{
    ProgressSnapshotExt, SnapshotBehavior, SnapshotProgressVariant, create_repl_progress_for_variant,
    create_run_progress_for_variant,
};

#[path = "support/snapshot_test_utils.rs"]
mod snapshot_test_utils;

/// Shared snapshot-extension payload used by round-trip tests.
///
/// The bytes are intentionally small and opaque because the tests only care
/// that serialization preserves exact binary metadata rather than interpreting
/// the payload.
#[fixture]
fn snapshot_extension() -> Vec<u8> {
    vec![1, 2, 3, 4]
}

/// Maps a progress variant to the snapshot-extension visibility expected after
/// dump/load.
///
/// Complete progress values do not expose snapshot metadata, while suspended
/// variants should preserve any attached bytes.
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

/// Asserts that observed snapshot bytes match the expected preservation
/// state.
///
/// Tests use this helper to keep the per-variant cases focused on setup while
/// centralizing the rule that absent metadata is only valid for completed
/// progress values.
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
        let completed = progress.complete_resolve_futures(&MontyObject::Int(1));
        assert_eq!(
            completed
                .into_complete()
                .expect("expected completion after resolving futures"),
            MontyObject::Int(1)
        );

        let completed_loaded = loaded.complete_resolve_futures(&MontyObject::Int(1));
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
        let completed = progress.complete_resolve_futures(&MontyObject::Int(1));
        assert_eq!(
            completed
                .into_complete()
                .expect("expected completion after resolving defaulted futures"),
            MontyObject::Int(1)
        );

        let completed_loaded = loaded.complete_resolve_futures(&MontyObject::Int(1));
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
        let completed = progress.complete_resolve_futures(&MontyObject::Int(3));
        let ReplProgress::Complete { value, .. } = completed else {
            panic!("expected completion after resolving REPL futures");
        };
        assert_eq!(value, MontyObject::Int(3));

        let completed_loaded = loaded.complete_resolve_futures(&MontyObject::Int(3));
        let ReplProgress::Complete { value, .. } = completed_loaded else {
            panic!("expected loaded completion after resolving REPL futures");
        };
        assert_eq!(value, MontyObject::Int(3));
    }
}

#[test]
fn corrupted_run_progress_payload_fails_to_load() {
    let progress = create_run_progress_for_variant(SnapshotProgressVariant::FunctionCall);
    let progress = progress.attach_snapshot_extension(vec![9, 8, 7]);
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
