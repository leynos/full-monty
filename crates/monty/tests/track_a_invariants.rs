#![cfg(unix)]

//! Compatibility invariants for Track A observer modes.
//!
//! Overhead and benchmark coverage lives in `crates/monty/tests/track_a_benchmarks.rs`.

use monty::{
    ExtFunctionResult, MontyException, MontyObject, MontyRepl, NoLimitTracker, PrintWriter, ReplProgress,
    ReplStartError, RunProgress, RuntimeValueId,
};
use rstest::{fixture, rstest};
use test_utils::{
    ObserverMode, assert_exceptions_equal, assert_function_calls_equal, assert_os_calls_equal, init_repl,
};
use track_a_test_utils::{BenchmarkMode, TrackATestGuard, build_run, start_run_with_mode, track_a_test_guard};

#[path = "support/test_utils.rs"]
mod test_utils;
#[path = "support/track_a_test_utils.rs"]
mod track_a_test_utils;

/// Script that suspends at a single external function call before printing the result.
const FUNCTION_CALL_SCRIPT: &str = "print(ext_fn(1))";
/// Script used to compare exception propagation after resuming an external call with an error.
const ERROR_SCRIPT: &str = "ext_fn(1)";
/// Script that suspends at an OS-backed `pathlib.Path.exists()` call.
const OS_CALL_SCRIPT: &str = "from pathlib import Path\nprint(Path('/tmp/track-a').exists())";
/// REPL bootstrap snippet that seeds mutable session state for Track A scenarios.
const REPL_INIT_SCRIPT: &str = "seed = 10";
/// REPL snippet that completes immediately while mutating the shared `seed` state.
const REPL_COMPLETE_SNIPPET: &str = "seed = seed + 1\nseed";
/// REPL snippet that suspends at an external function call after reading session state.
const REPL_SNAPSHOT_SNIPPET: &str = "print(ext_fn(seed + 1))";
/// Snapshot extension bytes used to verify round-trip preservation through REPL dump/load.
const SNAPSHOT_EXTENSION_BYTES: &[u8] = &[1, 3, 5, 7];

/// Starts a REPL snippet in baseline or observer-aware mode and returns the first progress value.
fn start_repl_with_mode(
    repl: MontyRepl<NoLimitTracker>,
    snippet: &str,
    mode: BenchmarkMode,
    writer: PrintWriter<'_>,
) -> Result<ReplProgress<NoLimitTracker>, Box<ReplStartError<NoLimitTracker>>> {
    match mode {
        BenchmarkMode::Baseline => repl.feed_start(snippet, Vec::new(), writer),
        BenchmarkMode::Observer(observer_mode) => {
            repl.feed_start_with_observer(snippet, Vec::new(), writer, observer_mode.handle())
        }
    }
}

/// Asserts that a run completed successfully with the expected final value.
fn assert_complete_progress(progress: RunProgress<NoLimitTracker>, expected: &MontyObject) {
    let Some(value) = progress.into_complete() else {
        panic!("expected RunProgress::Complete");
    };
    assert_eq!(&value, expected);
}

/// Asserts that a REPL completed with the expected value and preserved the expected follow-up state.
fn assert_repl_complete_progress(
    progress: ReplProgress<NoLimitTracker>,
    expected_value: &MontyObject,
    follow_up: &str,
    expected_follow_up: &MontyObject,
) {
    let Some((mut repl, value)) = progress.into_complete() else {
        panic!("expected ReplProgress::Complete");
    };
    assert_eq!(&value, expected_value);
    assert_eq!(
        repl.feed_run(follow_up, Vec::new(), PrintWriter::Disabled)
            .expect("follow-up snippet should succeed"),
        *expected_follow_up
    );
}

/// Owned comparison key for verifying REPL function-call payloads survive dump/load unchanged.
#[derive(Debug, PartialEq)]
struct ReplFunctionCallKey {
    function_name: String,
    args: Vec<MontyObject>,
    kwargs: Vec<(MontyObject, MontyObject)>,
    call_id: u32,
    method_call: bool,
    arg_runtime_ids: Vec<RuntimeValueId>,
    kwarg_runtime_ids: Vec<(RuntimeValueId, RuntimeValueId)>,
    snapshot_extension: Option<Vec<u8>>,
}

impl ReplFunctionCallKey {
    /// Captures the observable REPL function-call payload and snapshot metadata for round-trip checks.
    fn from_call(call: &monty::ReplFunctionCall<NoLimitTracker>) -> Self {
        Self {
            function_name: call.function_name.clone(),
            args: call.args.clone(),
            kwargs: call.kwargs.clone(),
            call_id: call.call_id,
            method_call: call.method_call,
            arg_runtime_ids: call.arg_runtime_ids.clone(),
            kwarg_runtime_ids: call.kwarg_runtime_ids.clone(),
            snapshot_extension: call.snapshot_extension().map(|ext| ext.as_slice().to_vec()),
        }
    }
}

/// Creates the cross-process Track A test guard so each rstest case holds the lock for its body.
#[fixture]
fn track_a_guard() -> TrackATestGuard {
    track_a_test_guard()
}

/// Verifies observer run modes suspend on the same function call and resume to the same output.
#[rstest]
#[case(ObserverMode::DisabledHandle)]
#[case(ObserverMode::NoopObserver)]
fn run_observer_modes_match_baseline_function_call_and_completion(
    #[case] mode: ObserverMode,
    track_a_guard: TrackATestGuard,
) {
    let run = build_run(FUNCTION_CALL_SCRIPT);
    let mut baseline_output = String::new();
    let mut baseline_print = PrintWriter::CollectString(&mut baseline_output);
    let baseline_progress = start_run_with_mode(&run, BenchmarkMode::Baseline, baseline_print.reborrow());
    let baseline_call = baseline_progress
        .into_function_call()
        .expect("baseline should suspend at function call");

    let mut observer_output = String::new();
    let mut observer_print = PrintWriter::CollectString(&mut observer_output);
    let observer_progress = start_run_with_mode(&run, BenchmarkMode::Observer(mode), observer_print.reborrow());
    let observer_call = observer_progress
        .into_function_call()
        .expect("observer-aware mode should suspend at function call");

    assert_function_calls_equal(&baseline_call, &observer_call);

    let baseline_resume = baseline_call
        .resume(MontyObject::Int(7), baseline_print.reborrow())
        .expect("baseline resume should succeed");
    let observer_resume = observer_call
        .resume(MontyObject::Int(7), observer_print.reborrow())
        .expect("observer-aware resume should succeed");

    assert_complete_progress(baseline_resume, &MontyObject::None);
    assert_complete_progress(observer_resume, &MontyObject::None);
    assert_eq!(baseline_output, observer_output);
    drop(track_a_guard);
}

/// Verifies observer run modes propagate resumed external-call errors exactly like baseline runs.
#[rstest]
#[case(ObserverMode::DisabledHandle)]
#[case(ObserverMode::NoopObserver)]
fn run_observer_modes_match_baseline_error_path(#[case] mode: ObserverMode, track_a_guard: TrackATestGuard) {
    let run = build_run(ERROR_SCRIPT);
    let baseline_progress = start_run_with_mode(&run, BenchmarkMode::Baseline, PrintWriter::Disabled);
    let baseline_call = baseline_progress
        .into_function_call()
        .expect("baseline should suspend at function call");

    let observer_progress = start_run_with_mode(&run, BenchmarkMode::Observer(mode), PrintWriter::Disabled);
    let observer_call = observer_progress
        .into_function_call()
        .expect("observer-aware mode should suspend at function call");

    assert_function_calls_equal(&baseline_call, &observer_call);

    let exception = MontyException::new(monty::ExcType::RuntimeError, Some("track-a failure".to_owned()));
    let baseline_error = baseline_call
        .resume(ExtFunctionResult::Error(exception.clone()), PrintWriter::Disabled)
        .expect_err("baseline resume should error");
    let observer_error = observer_call
        .resume(ExtFunctionResult::Error(exception), PrintWriter::Disabled)
        .expect_err("observer-aware resume should error");

    assert_exceptions_equal(&baseline_error, &observer_error);
    drop(track_a_guard);
}

/// Verifies observer run modes expose identical OS-call suspensions and resumed output.
#[rstest]
#[case(ObserverMode::DisabledHandle)]
#[case(ObserverMode::NoopObserver)]
fn run_observer_modes_match_baseline_os_call_path(#[case] mode: ObserverMode, track_a_guard: TrackATestGuard) {
    let run = build_run(OS_CALL_SCRIPT);
    let mut baseline_output = String::new();
    let mut baseline_print = PrintWriter::CollectString(&mut baseline_output);
    let baseline_progress = start_run_with_mode(&run, BenchmarkMode::Baseline, baseline_print.reborrow());
    let baseline_call = baseline_progress
        .into_os_call()
        .expect("baseline should suspend at OS call");

    let mut observer_output = String::new();
    let mut observer_print = PrintWriter::CollectString(&mut observer_output);
    let observer_progress = start_run_with_mode(&run, BenchmarkMode::Observer(mode), observer_print.reborrow());
    let observer_call = observer_progress
        .into_os_call()
        .expect("observer-aware mode should suspend at OS call");

    assert_os_calls_equal(&baseline_call, &observer_call);

    let baseline_resume = baseline_call
        .resume(MontyObject::Bool(false), baseline_print.reborrow())
        .expect("baseline resume should succeed");
    let observer_resume = observer_call
        .resume(MontyObject::Bool(false), observer_print.reborrow())
        .expect("observer-aware resume should succeed");

    assert_complete_progress(baseline_resume, &MontyObject::None);
    assert_complete_progress(observer_resume, &MontyObject::None);
    assert_eq!(baseline_output, observer_output);
    drop(track_a_guard);
}

/// Verifies observer-aware REPL completion preserves the same state transitions as baseline REPLs.
#[rstest]
#[case(ObserverMode::DisabledHandle)]
#[case(ObserverMode::NoopObserver)]
fn repl_observer_modes_match_baseline_completion(#[case] mode: ObserverMode, track_a_guard: TrackATestGuard) {
    let baseline_repl = init_repl("track_a_repl.py", REPL_INIT_SCRIPT);
    let baseline_progress = start_repl_with_mode(
        baseline_repl,
        REPL_COMPLETE_SNIPPET,
        BenchmarkMode::Baseline,
        PrintWriter::Disabled,
    )
    .expect("baseline REPL start should succeed");

    let observer_repl = init_repl("track_a_repl.py", REPL_INIT_SCRIPT);
    let observer_progress = start_repl_with_mode(
        observer_repl,
        REPL_COMPLETE_SNIPPET,
        BenchmarkMode::Observer(mode),
        PrintWriter::Disabled,
    )
    .expect("observer-aware REPL start should succeed");

    assert_repl_complete_progress(
        baseline_progress,
        &MontyObject::Int(11),
        "seed + 1",
        &MontyObject::Int(12),
    );
    assert_repl_complete_progress(
        observer_progress,
        &MontyObject::Int(11),
        "seed + 1",
        &MontyObject::Int(12),
    );
    drop(track_a_guard);
}

/// Verifies REPL snapshot dump/load preserves observer-visible function-call state and output.
#[rstest]
#[case(ObserverMode::DisabledHandle)]
#[case(ObserverMode::NoopObserver)]
fn repl_snapshot_round_trip_matches_baseline(#[case] mode: ObserverMode, track_a_guard: TrackATestGuard) {
    let baseline_repl = init_repl("track_a_repl.py", REPL_INIT_SCRIPT);
    let mut baseline_output = String::new();
    let mut baseline_print = PrintWriter::CollectString(&mut baseline_output);
    let baseline_progress = start_repl_with_mode(
        baseline_repl,
        REPL_SNAPSHOT_SNIPPET,
        BenchmarkMode::Baseline,
        baseline_print.reborrow(),
    )
    .expect("baseline REPL should suspend at function call");
    let baseline_call = baseline_progress
        .into_function_call()
        .expect("baseline should suspend at function call");

    let observer_repl = init_repl("track_a_repl.py", REPL_INIT_SCRIPT);
    let mut observer_output = String::new();
    let mut observer_print = PrintWriter::CollectString(&mut observer_output);
    let observer_progress = start_repl_with_mode(
        observer_repl,
        REPL_SNAPSHOT_SNIPPET,
        BenchmarkMode::Observer(mode),
        observer_print.reborrow(),
    )
    .expect("observer-aware REPL should suspend at function call");
    let observer_call = observer_progress
        .into_function_call()
        .expect("observer-aware REPL should suspend at function call");

    assert_function_calls_equal(&baseline_call, &observer_call);

    let baseline_dump_call = baseline_call.with_snapshot_extension(SNAPSHOT_EXTENSION_BYTES.to_vec());
    let baseline_dump_key = ReplFunctionCallKey::from_call(&baseline_dump_call);
    let baseline_bytes = ReplProgress::FunctionCall(baseline_dump_call)
        .dump()
        .expect("baseline progress should dump");
    let baseline_loaded = ReplProgress::<NoLimitTracker>::load(&baseline_bytes).expect("baseline load should succeed");
    let baseline_loaded_call = baseline_loaded
        .into_function_call()
        .expect("loaded baseline should stay suspended");
    assert_eq!(ReplFunctionCallKey::from_call(&baseline_loaded_call), baseline_dump_key);
    assert_eq!(
        baseline_loaded_call
            .snapshot_extension()
            .map(monty::SnapshotExtension::as_slice),
        Some(SNAPSHOT_EXTENSION_BYTES)
    );

    let observer_dump_call = observer_call.with_snapshot_extension(SNAPSHOT_EXTENSION_BYTES.to_vec());
    let observer_dump_key = ReplFunctionCallKey::from_call(&observer_dump_call);
    let observer_bytes = ReplProgress::FunctionCall(observer_dump_call)
        .dump()
        .expect("observer-aware progress should dump");
    let observer_loaded = ReplProgress::<NoLimitTracker>::load(&observer_bytes).expect("observer load should succeed");
    let observer_loaded_call = observer_loaded
        .into_function_call()
        .expect("loaded observer-aware progress should stay suspended");
    assert_eq!(ReplFunctionCallKey::from_call(&observer_loaded_call), observer_dump_key);
    assert_eq!(
        observer_loaded_call
            .snapshot_extension()
            .map(monty::SnapshotExtension::as_slice),
        Some(SNAPSHOT_EXTENSION_BYTES)
    );

    let baseline_resume = baseline_loaded_call
        .resume(MontyObject::Int(11), baseline_print.reborrow())
        .expect("baseline resume should succeed");
    let observer_resume = observer_loaded_call
        .resume(MontyObject::Int(11), observer_print.reborrow())
        .expect("observer-aware resume should succeed");

    assert_repl_complete_progress(baseline_resume, &MontyObject::None, "seed", &MontyObject::Int(10));
    assert_repl_complete_progress(observer_resume, &MontyObject::None, "seed", &MontyObject::Int(10));
    assert_eq!(baseline_output, observer_output);
    drop(track_a_guard);
}
