#![cfg(unix)]

//! Overhead benchmarks for Track A observer modes.

use std::{hint::black_box, time::Instant};

use monty::{MontyObject, MontyRun, PrintWriter};
use rstest::rstest;
use test_utils::ObserverMode;
use track_a_test_utils::{BenchmarkMode, build_run, start_run_with_mode, track_a_test_guard};

#[expect(
    dead_code,
    reason = "shared helper module defines utilities consumed by sibling integration tests"
)]
#[path = "support/test_utils.rs"]
mod test_utils;
#[path = "support/track_a_test_utils.rs"]
mod track_a_test_utils;

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
/// Warmup iterations discarded before measuring benchmark medians.
const BENCHMARK_WARMUP_RUNS: usize = 5;
/// Samples collected for each median benchmark estimate.
const BENCHMARK_SAMPLES: usize = 11;
/// Repeated median estimates used to stabilize benchmark noise.
const BENCHMARK_ATTEMPTS: usize = 3;
/// Disabled observer mode may add at most 20% overhead because it should stay close to baseline.
const DISABLED_OVERHEAD_MAX_PERCENT: u128 = 120;
/// No-op observer mode may add up to 140% overhead because every event still triggers callbacks
/// and `start_run_with_mode(...)` exercises the real `MontyRun::start_with_observer(...)` path,
/// which shows modest variance across feature-gated test runs in CI, especially under
/// `ref-count-return`.
const NOOP_OVERHEAD_MAX_PERCENT: u128 = 240;

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

/// Verifies each observer mode stays within its configured Track A overhead budget.
#[rstest]
#[case(ObserverMode::DisabledHandle, DISABLED_OVERHEAD_MAX_PERCENT)]
#[case(ObserverMode::NoopObserver, NOOP_OVERHEAD_MAX_PERCENT)]
fn track_a_overhead_within_budget(#[case] mode: ObserverMode, #[case] max_percent: u128) {
    let _guard = track_a_test_guard();
    let baseline = stable_median_ns(BenchmarkMode::Baseline);
    let observed = stable_median_ns(BenchmarkMode::Observer(mode));
    println!("track_a_overhead {mode:?} baseline_ns={baseline} observed_ns={observed}");
    assert!(observed * 100 <= baseline * max_percent);
}
