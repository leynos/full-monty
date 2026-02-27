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
fn function_call_runtime_ids_match_for_reused_positional_object() {
    let runner = MontyRun::new(
        "x = []; ext_fn(x, x)".to_owned(),
        "test.py",
        vec![],
        vec!["ext_fn".to_owned()],
    )
    .expect("runner creation should succeed");

    let progress = runner
        .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
        .expect("run should pause at external call");
    let RunProgress::FunctionCall { arg_runtime_ids, .. } = progress else {
        panic!("expected function call");
    };

    assert_eq!(arg_runtime_ids.len(), 2);
    assert_eq!(
        arg_runtime_ids[0], arg_runtime_ids[1],
        "reusing the same runtime object should preserve ID identity"
    );
}

#[test]
fn function_call_runtime_ids_differ_for_equal_but_distinct_positional_objects() {
    let runner = MontyRun::new(
        "ext_fn([], [])".to_owned(),
        "test.py",
        vec![],
        vec!["ext_fn".to_owned()],
    )
    .expect("runner creation should succeed");

    let progress = runner
        .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
        .expect("run should pause at external call");
    let RunProgress::FunctionCall { arg_runtime_ids, .. } = progress else {
        panic!("expected function call");
    };

    assert_eq!(arg_runtime_ids.len(), 2);
    assert_ne!(
        arg_runtime_ids[0], arg_runtime_ids[1],
        "distinct runtime objects should not share an ID even when values compare equal"
    );
}

#[test]
fn function_call_kwarg_runtime_ids_match_kwargs_and_are_stable_across_dump_load() {
    let runner = MontyRun::new(
        "ext_fn(a=1, b=2)".to_owned(),
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
    } = &progress
    else {
        panic!("expected function call");
    };

    assert!(args.is_empty(), "expected keyword-only external call");
    assert!(arg_runtime_ids.is_empty(), "expected no positional runtime IDs");
    assert_eq!(kwargs.len(), 2, "expected two keyword arguments");
    assert_eq!(
        kwarg_runtime_ids.len(),
        kwargs.len(),
        "kwarg runtime IDs should align 1:1 with kwargs"
    );
    assert_eq!(
        kwargs,
        &vec![
            (MontyObject::String("a".to_owned()), MontyObject::Int(1)),
            (MontyObject::String("b".to_owned()), MontyObject::Int(2))
        ],
        "keyword arguments should preserve insertion order and payload"
    );

    let first_kwarg_runtime_ids: Vec<(usize, usize)> = kwarg_runtime_ids
        .iter()
        .map(|(key_id, value_id)| (key_id.raw(), value_id.raw()))
        .collect();
    assert_ne!(
        first_kwarg_runtime_ids[0], first_kwarg_runtime_ids[1],
        "distinct kwargs should have distinct (key, value) runtime IDs"
    );

    let bytes = progress.dump().expect("run progress dump should succeed");
    let loaded: RunProgress<NoLimitTracker> = RunProgress::load(&bytes).expect("run progress load should succeed");

    let RunProgress::FunctionCall {
        args,
        kwargs,
        arg_runtime_ids,
        kwarg_runtime_ids,
        ..
    } = loaded
    else {
        panic!("expected loaded function call");
    };

    assert!(args.is_empty(), "expected keyword-only external call after load");
    assert!(
        arg_runtime_ids.is_empty(),
        "expected no positional runtime IDs after load"
    );
    assert_eq!(
        kwargs,
        vec![
            (MontyObject::String("a".to_owned()), MontyObject::Int(1)),
            (MontyObject::String("b".to_owned()), MontyObject::Int(2))
        ],
        "keyword payload should remain stable after dump/load"
    );

    let second_kwarg_runtime_ids: Vec<(usize, usize)> = kwarg_runtime_ids
        .iter()
        .map(|(key_id, value_id)| (key_id.raw(), value_id.raw()))
        .collect();
    assert_eq!(
        second_kwarg_runtime_ids, first_kwarg_runtime_ids,
        "kwarg runtime IDs should remain stable across dump/load"
    );
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
fn runtime_ids_are_unavailable_for_non_call_progress() {
    let progress = RunProgress::<NoLimitTracker>::Complete(MontyObject::None);
    assert!(progress.runtime_ids().is_none());
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
    assert!(
        RunProgress::<NoLimitTracker>::load(&bytes).is_ok(),
        "unmodified run progress payload should load"
    );

    assert!(!bytes.is_empty(), "serialized run progress should not be empty");
    bytes[0] ^= 0xFF;

    assert!(RunProgress::<NoLimitTracker>::load(&bytes).is_err());
}
