//! Compatibility and overhead checks for Track A observer modes.

use std::{
    hint::black_box,
    sync::{Mutex, MutexGuard, OnceLock},
    time::Instant,
};

use monty::{
    ExtFunctionResult, MontyException, MontyObject, MontyRepl, MontyRun, NoLimitTracker, PrintWriter, ReplProgress,
    ReplStartError, RunProgress,
};
use rstest::rstest;
use test_utils::{
    ObserverMode, assert_exceptions_equal, assert_function_calls_equal, assert_os_calls_equal, init_repl,
};

#[path = "support/test_utils.rs"]
mod test_utils;

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
/// Benchmark script used to estimate observer overhead on a deterministic CPU-bound workload.
const BENCHMARK_SCRIPT: &str = r"
total = 0
for i in range(2_000):
    if i % 3 == 0:
        total = total + i
    else:
        total = total - 1
total
";
/// Snapshot extension bytes used to verify round-trip preservation through REPL dump/load.
const SNAPSHOT_EXTENSION_BYTES: &[u8] = &[1, 3, 5, 7];
/// Warmup iterations discarded before measuring benchmark medians.
const BENCHMARK_WARMUP_RUNS: usize = 5;
/// Samples collected for each median benchmark estimate.
const BENCHMARK_SAMPLES: usize = 11;
/// Repeated median estimates used to stabilize benchmark noise.
const BENCHMARK_ATTEMPTS: usize = 3;
/// Disabled observer mode may add at most 20% overhead because it should stay close to baseline.
const DISABLED_OVERHEAD_MAX_PERCENT: u128 = 120;
/// No-op observer mode may add up to 140% overhead because every event still triggers callbacks
/// and the identical `feed_start_with_observer` path shows modest variance across feature-gated
/// test runs in CI, especially under `ref-count-return`.
const NOOP_OVERHEAD_MAX_PERCENT: u128 = 240;

/// Execution mode used by the observer-overhead benchmark.
///
/// `Baseline` and `Observer` both go through `start(...).into_complete()` so the measurement
/// isolates observer cost rather than comparing two different execution APIs.
#[derive(Debug, Clone, Copy)]
enum BenchmarkMode {
    Baseline,
    Observer(ObserverMode),
}

/// Builds a fresh runner for the given Track A script using a stable test filename.
fn build_run(script: &str) -> MontyRun {
    MontyRun::new(script.to_owned(), "track_a.py", vec![]).expect("runner creation should succeed")
}

/// Starts a run in either baseline or observer-aware mode and returns its first progress value.
fn start_run_with_mode(run: &MontyRun, mode: BenchmarkMode, print: PrintWriter<'_>) -> RunProgress<NoLimitTracker> {
    match mode {
        BenchmarkMode::Baseline => run
            .clone()
            .start(vec![], NoLimitTracker, print)
            .expect("baseline start should succeed"),
        BenchmarkMode::Observer(observer_mode) => run
            .clone()
            .start_with_observer(vec![], NoLimitTracker, print, observer_mode.handle())
            .expect("observer-aware start should succeed"),
    }
}

/// Starts a REPL snippet with the requested observer mode and returns the first progress value.
fn start_repl_with_mode(
    repl: MontyRepl<NoLimitTracker>,
    snippet: &str,
    mode: ObserverMode,
) -> Result<ReplProgress<NoLimitTracker>, Box<ReplStartError<NoLimitTracker>>> {
    repl.feed_start_with_observer(snippet, Vec::new(), PrintWriter::Disabled, mode.handle())
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

/// Serializes access to Track A tests that would otherwise contend on shared runtime state.
fn track_a_test_guard() -> MutexGuard<'static, ()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Measures the median nanoseconds for one benchmark attempt in the requested execution mode.
fn median_ns(mode: BenchmarkMode) -> u128 {
    let run = build_run(BENCHMARK_SCRIPT);
    let mut durations = Vec::with_capacity(BENCHMARK_SAMPLES);

    for _ in 0..BENCHMARK_WARMUP_RUNS {
        let _ = black_box(run_benchmark_iteration(&run, mode));
    }

    for _ in 0..BENCHMARK_SAMPLES {
        let start = Instant::now();
        let result = run_benchmark_iteration(&run, mode);
        let elapsed = start.elapsed().as_nanos();
        assert_eq!(result, MontyObject::Int(665_000));
        durations.push(elapsed);
    }

    durations.sort_unstable();
    durations[durations.len() / 2]
}

/// Repeats `median_ns` and returns the median of medians to reduce noise in CI environments.
fn stable_median_ns(mode: BenchmarkMode) -> u128 {
    let mut medians = Vec::with_capacity(BENCHMARK_ATTEMPTS);
    for _ in 0..BENCHMARK_ATTEMPTS {
        medians.push(median_ns(mode));
    }
    medians.sort_unstable();
    medians[medians.len() / 2]
}

/// Executes one benchmark iteration through the start/resume path used by Track A comparisons.
fn run_benchmark_iteration(run: &MontyRun, mode: BenchmarkMode) -> MontyObject {
    let progress = start_run_with_mode(run, mode, PrintWriter::Disabled);
    let Some(value) = progress.into_complete() else {
        panic!("benchmark script should complete without suspension");
    };
    value
}

#[rstest]
#[case(ObserverMode::DisabledHandle)]
#[case(ObserverMode::NoopObserver)]
fn run_observer_modes_match_baseline_function_call_and_completion(#[case] mode: ObserverMode) {
    let _guard = track_a_test_guard();
    let run = build_run(FUNCTION_CALL_SCRIPT);
    let mut baseline_output = String::new();
    let mut baseline_print = PrintWriter::Collect(&mut baseline_output);
    let baseline_progress = start_run_with_mode(&run, BenchmarkMode::Baseline, baseline_print.reborrow());
    let baseline_call = baseline_progress
        .into_function_call()
        .expect("baseline should suspend at function call");

    let mut observer_output = String::new();
    let mut observer_print = PrintWriter::Collect(&mut observer_output);
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
    assert_eq!(baseline_output.clone(), observer_output.clone());
}

#[rstest]
#[case(ObserverMode::DisabledHandle)]
#[case(ObserverMode::NoopObserver)]
fn run_observer_modes_match_baseline_error_path(#[case] mode: ObserverMode) {
    let _guard = track_a_test_guard();
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
}

#[rstest]
#[case(ObserverMode::DisabledHandle)]
#[case(ObserverMode::NoopObserver)]
fn run_observer_modes_match_baseline_os_call_path(#[case] mode: ObserverMode) {
    let _guard = track_a_test_guard();
    let run = build_run(OS_CALL_SCRIPT);
    let mut baseline_output = String::new();
    let mut baseline_print = PrintWriter::Collect(&mut baseline_output);
    let baseline_progress = start_run_with_mode(&run, BenchmarkMode::Baseline, baseline_print.reborrow());
    let baseline_call = baseline_progress
        .into_os_call()
        .expect("baseline should suspend at OS call");

    let mut observer_output = String::new();
    let mut observer_print = PrintWriter::Collect(&mut observer_output);
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
    assert_eq!(baseline_output.clone(), observer_output.clone());
}

#[rstest]
#[case(ObserverMode::DisabledHandle)]
#[case(ObserverMode::NoopObserver)]
fn repl_observer_modes_match_baseline_completion(#[case] mode: ObserverMode) {
    let _guard = track_a_test_guard();
    let baseline_repl = init_repl("track_a_repl.py", REPL_INIT_SCRIPT);
    let baseline_progress = baseline_repl
        .feed_start(REPL_COMPLETE_SNIPPET, Vec::new(), PrintWriter::Disabled)
        .expect("baseline REPL start should succeed");

    let observer_repl = init_repl("track_a_repl.py", REPL_INIT_SCRIPT);
    let observer_progress = start_repl_with_mode(observer_repl, REPL_COMPLETE_SNIPPET, mode)
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
}

#[rstest]
#[case(ObserverMode::DisabledHandle)]
#[case(ObserverMode::NoopObserver)]
fn repl_snapshot_round_trip_matches_baseline(#[case] mode: ObserverMode) {
    let _guard = track_a_test_guard();
    let baseline_repl = init_repl("track_a_repl.py", REPL_INIT_SCRIPT);
    let mut baseline_output = String::new();
    let mut baseline_print = PrintWriter::Collect(&mut baseline_output);
    let baseline_progress = baseline_repl
        .feed_start(REPL_SNAPSHOT_SNIPPET, Vec::new(), baseline_print.reborrow())
        .expect("baseline REPL should suspend at function call");
    let baseline_call = baseline_progress
        .into_function_call()
        .expect("baseline should suspend at function call");

    let observer_repl = init_repl("track_a_repl.py", REPL_INIT_SCRIPT);
    let mut observer_output = String::new();
    let mut observer_print = PrintWriter::Collect(&mut observer_output);
    let observer_progress = observer_repl
        .feed_start_with_observer(
            REPL_SNAPSHOT_SNIPPET,
            Vec::new(),
            observer_print.reborrow(),
            mode.handle(),
        )
        .expect("observer-aware REPL should suspend at function call");
    let observer_call = observer_progress
        .into_function_call()
        .expect("observer-aware REPL should suspend at function call");

    assert_function_calls_equal(&baseline_call, &observer_call);

    let baseline_bytes =
        ReplProgress::FunctionCall(baseline_call.with_snapshot_extension(SNAPSHOT_EXTENSION_BYTES.to_vec()))
            .dump()
            .expect("baseline progress should dump");
    let baseline_loaded = ReplProgress::<NoLimitTracker>::load(&baseline_bytes).expect("baseline load should succeed");
    let baseline_loaded_call = baseline_loaded
        .into_function_call()
        .expect("loaded baseline should stay suspended");
    assert_eq!(
        baseline_loaded_call
            .snapshot_extension()
            .map(monty::SnapshotExtension::as_slice),
        Some(SNAPSHOT_EXTENSION_BYTES)
    );

    let observer_bytes =
        ReplProgress::FunctionCall(observer_call.with_snapshot_extension(SNAPSHOT_EXTENSION_BYTES.to_vec()))
            .dump()
            .expect("observer-aware progress should dump");
    let observer_loaded = ReplProgress::<NoLimitTracker>::load(&observer_bytes).expect("observer load should succeed");
    let observer_loaded_call = observer_loaded
        .into_function_call()
        .expect("loaded observer-aware progress should stay suspended");
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
    assert_eq!(baseline_output.clone(), observer_output.clone());
}

#[test]
fn track_a_overhead_disabled_within_budget() {
    let _guard = track_a_test_guard();
    let baseline = stable_median_ns(BenchmarkMode::Baseline);
    let disabled = stable_median_ns(BenchmarkMode::Observer(ObserverMode::DisabledHandle));
    println!("track_a_overhead disabled baseline_ns={baseline} observed_ns={disabled}");
    assert!(disabled * 100 <= baseline * DISABLED_OVERHEAD_MAX_PERCENT);
}

#[test]
fn track_a_overhead_noop_within_budget() {
    let _guard = track_a_test_guard();
    let baseline = stable_median_ns(BenchmarkMode::Baseline);
    let noop = stable_median_ns(BenchmarkMode::Observer(ObserverMode::NoopObserver));
    println!("track_a_overhead noop baseline_ns={baseline} observed_ns={noop}");
    assert!(noop * 100 <= baseline * NOOP_OVERHEAD_MAX_PERCENT);
}
