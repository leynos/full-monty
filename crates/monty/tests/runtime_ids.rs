//! Tests for host-facing runtime IDs on suspendable execution payloads.
//!
//! These tests validate that runtime IDs are available to the host and remain
//! stable across pause/resume and dump/load boundaries.

use std::collections::HashSet;

use monty::{MontyObject, MontyRun, NoLimitTracker, PrintWriter, RunProgress};

#[test]
fn function_call_runtime_ids_are_unique_for_distinct_positional_arguments() {
    let runner = MontyRun::new(
        "ext_fn(1, 2, 'three', [4])".to_owned(),
        "test.py",
        vec![],
        vec!["ext_fn".to_owned()],
    )
    .expect("runner creation should succeed");

    let progress = runner
        .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
        .expect("run should pause at external call");
    let RunProgress::FunctionCall {
        args,
        kwargs,
        arg_runtime_ids,
        kwarg_runtime_ids,
        ..
    } = progress
    else {
        panic!("expected function call");
    };

    assert_eq!(args.len(), arg_runtime_ids.len());
    assert!(kwargs.is_empty());
    assert!(kwarg_runtime_ids.is_empty());

    let unique_ids: HashSet<usize> = arg_runtime_ids.iter().map(|id| id.raw()).collect();
    assert_eq!(unique_ids.len(), arg_runtime_ids.len());
}

#[test]
fn runtime_ids_round_trip_with_run_progress_dump_load() {
    let runner = MontyRun::new("ext_fn([])".to_owned(), "test.py", vec![], vec!["ext_fn".to_owned()])
        .expect("runner creation should succeed");

    let progress = runner
        .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
        .expect("run should pause at external call");
    let expected_ids: Vec<usize> = progress
        .runtime_ids()
        .expect("function call should expose runtime ids")
        .0
        .iter()
        .map(|id| id.raw())
        .collect();

    let bytes = progress.dump().expect("run progress dump should succeed");
    let loaded: RunProgress<NoLimitTracker> = RunProgress::load(&bytes).expect("run progress load should succeed");
    let loaded_ids: Vec<usize> = loaded
        .runtime_ids()
        .expect("loaded function call should expose runtime ids")
        .0
        .iter()
        .map(|id| id.raw())
        .collect();

    assert_eq!(loaded_ids, expected_ids);
}

#[test]
fn runtime_ids_remain_stable_across_resume_boundaries() {
    let runner = MontyRun::new(
        "x = []; ext_fn(x); ext_fn(x)".to_owned(),
        "test.py",
        vec![],
        vec!["ext_fn".to_owned()],
    )
    .expect("runner creation should succeed");

    let progress = runner
        .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
        .expect("run should pause at first external call");
    let RunProgress::FunctionCall {
        arg_runtime_ids, state, ..
    } = progress
    else {
        panic!("expected first function call");
    };
    let first_id = arg_runtime_ids
        .first()
        .expect("first call should include one arg id")
        .raw();

    let progress = state
        .run(MontyObject::None, &mut PrintWriter::Stdout)
        .expect("resume should reach second external call");
    let RunProgress::FunctionCall {
        arg_runtime_ids, state, ..
    } = progress
    else {
        panic!("expected second function call");
    };
    let second_id = arg_runtime_ids
        .first()
        .expect("second call should include one arg id")
        .raw();

    assert_eq!(first_id, second_id);

    let completion = state
        .run(MontyObject::None, &mut PrintWriter::Stdout)
        .expect("final resume should complete");
    assert!(matches!(completion, RunProgress::Complete(_)));
}

#[test]
fn runtime_ids_remain_stable_across_run_progress_dump_load_and_resume() {
    let runner = MontyRun::new(
        "x = []; ext_fn(x); ext_fn(x)".to_owned(),
        "test.py",
        vec![],
        vec!["ext_fn".to_owned()],
    )
    .expect("runner creation should succeed");

    let progress = runner
        .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
        .expect("run should pause at first external call");
    let RunProgress::FunctionCall {
        ref arg_runtime_ids, ..
    } = progress
    else {
        panic!("expected first function call");
    };
    let first_id = arg_runtime_ids
        .first()
        .expect("first call should include one arg id")
        .raw();

    let bytes = progress.dump().expect("run progress dump should succeed");
    let loaded_progress: RunProgress<NoLimitTracker> =
        RunProgress::load(&bytes).expect("run progress load should succeed");

    let RunProgress::FunctionCall { state, .. } = loaded_progress else {
        panic!("expected loaded function call");
    };

    let progress = state
        .run(MontyObject::None, &mut PrintWriter::Stdout)
        .expect("resumed state should reach second external call");
    let RunProgress::FunctionCall {
        arg_runtime_ids, state, ..
    } = progress
    else {
        panic!("expected second function call");
    };
    let second_id = arg_runtime_ids
        .first()
        .expect("second call should include one arg id")
        .raw();

    assert_eq!(first_id, second_id);

    let completion = state
        .run(MontyObject::None, &mut PrintWriter::Stdout)
        .expect("final resume should complete");
    assert!(matches!(completion, RunProgress::Complete(_)));
}

#[test]
fn corrupted_run_progress_payload_fails_to_load() {
    let runner = MontyRun::new("ext_fn([])".to_owned(), "test.py", vec![], vec!["ext_fn".to_owned()])
        .expect("runner creation should succeed");

    let progress = runner
        .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
        .expect("run should pause at external call");
    let mut bytes = progress.dump().expect("run progress dump should succeed");

    assert!(!bytes.is_empty(), "serialized run progress should not be empty");
    bytes[0] ^= 0xFF;

    assert!(RunProgress::<NoLimitTracker>::load(&bytes).is_err());
}
