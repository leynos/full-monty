//! Shared helpers for Track A observer integration tests and benchmarks.

use std::{
    env::temp_dir,
    fs::{File, OpenOptions},
    path::PathBuf,
    sync::OnceLock,
};

use monty::{MontyRun, NoLimitTracker, PrintWriter, RunProgress};
use nix::fcntl::{Flock, FlockArg};

use crate::test_utils::ObserverMode;

/// Execution mode shared by Track A observer tests and benchmarks.
///
/// `Baseline` starts runs with `MontyRun::start`, while `Observer` uses
/// `MontyRun::start_with_observer`. Both variants are compared through the same progress APIs so
/// tests can isolate observer-specific behavior from unrelated setup differences.
#[derive(Debug, Clone, Copy)]
pub(crate) enum BenchmarkMode {
    Baseline,
    Observer(ObserverMode),
}

/// Cross-process guard used in tests to hold the Track A benchmark lock file for the current
/// scope.
///
/// The guard wraps the acquired `Flock<File>` and relies on RAII to release the exclusive lock
/// when the test scope ends, preventing concurrent benchmark executions from separate test
/// binaries from contaminating each other.
pub(crate) struct TrackATestGuard {
    /// Held exclusive lock; dropping the guard releases the benchmark lock file.
    _file: Flock<File>,
}

/// Builds a fresh runner for the given Track A script using a stable test filename.
pub(crate) fn build_run(script: &str) -> MontyRun {
    MontyRun::new(script.to_owned(), "track_a.py", vec![]).expect("runner creation should succeed")
}

/// Starts a run in either baseline or observer-aware mode and returns its first progress value.
pub(crate) fn start_run_with_mode(
    run: &MontyRun,
    mode: BenchmarkMode,
    print: PrintWriter<'_>,
) -> RunProgress<NoLimitTracker> {
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

/// Returns the stable lock-file path used to serialize Track A tests across processes.
fn track_a_lock_path() -> &'static PathBuf {
    static LOCK_PATH: OnceLock<PathBuf> = OnceLock::new();
    LOCK_PATH.get_or_init(|| temp_dir().join("full-monty-track-a.lock"))
}

/// Serializes Track A tests across threads and test binaries to keep benchmark runs isolated.
pub(crate) fn track_a_test_guard() -> TrackATestGuard {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(track_a_lock_path())
        .expect("track-a lock file should open");
    let file = Flock::lock(file, FlockArg::LockExclusive)
        .map_err(|(_, err)| err)
        .expect("track-a lock file should lock exclusively");
    TrackATestGuard { _file: file }
}
