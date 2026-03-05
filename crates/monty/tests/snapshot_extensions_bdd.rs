//! Behavioural coverage for snapshot extension byte persistence.

use monty::{MontyRun, NoLimitTracker, PrintWriter, ReplProgress, RunProgress, SnapshotExtension};
use rstest::fixture;
use rstest_bdd_macros::{given, scenario, then, when};
use snapshot_test_utils::create_repl;

trait ProgressSnapshotExt: Sized {
    fn attach_snapshot_extension(self, ext: Vec<u8>) -> Self;
    fn get_snapshot_extension(&self) -> Option<&[u8]>;
}

macro_rules! impl_progress_snapshot_ext {
    ($Progress:ident, $complete_pat:pat => $complete_expr:expr) => {
        impl ProgressSnapshotExt for $Progress<NoLimitTracker> {
            fn attach_snapshot_extension(self, snapshot_extension: Vec<u8>) -> Self {
                match self {
                    Self::FunctionCall(call) => Self::FunctionCall(call.with_snapshot_extension(snapshot_extension)),
                    Self::OsCall(call) => Self::OsCall(call.with_snapshot_extension(snapshot_extension)),
                    Self::ResolveFutures(state) => {
                        Self::ResolveFutures(state.with_snapshot_extension(snapshot_extension))
                    }
                    Self::NameLookup(lookup) => Self::NameLookup(lookup.with_snapshot_extension(snapshot_extension)),
                    $complete_pat => $complete_expr,
                }
            }

            fn get_snapshot_extension(&self) -> Option<&[u8]> {
                match self {
                    Self::FunctionCall(call) => call.snapshot_extension().map(SnapshotExtension::as_slice),
                    Self::OsCall(call) => call.snapshot_extension().map(SnapshotExtension::as_slice),
                    Self::ResolveFutures(state) => state.snapshot_extension().map(SnapshotExtension::as_slice),
                    Self::NameLookup(lookup) => lookup.snapshot_extension().map(SnapshotExtension::as_slice),
                    _ => None,
                }
            }
        }
    };
}

impl_progress_snapshot_ext!(
    RunProgress,
    Self::Complete(value) => Self::Complete(value)
);
impl_progress_snapshot_ext!(
    ReplProgress,
    Self::Complete { repl, value } => Self::Complete { repl, value }
);

#[expect(
    dead_code,
    reason = "BDD scenarios share a helper module with non-BDD snapshot tests"
)]
#[path = "support/snapshot_test_utils.rs"]
mod snapshot_test_utils;

#[derive(Default)]
struct SnapshotExtensionsWorld {
    script: String,
    repl_snippet: String,
    snapshot_extension: Vec<u8>,
    loaded_snapshot_extension: Option<Vec<u8>>,
    load_failed: bool,
}

#[fixture]
fn world() -> SnapshotExtensionsWorld {
    SnapshotExtensionsWorld::default()
}

#[given("a suspendable script with one external call")]
fn given_suspendable_script(world: &mut SnapshotExtensionsWorld) {
    "ext_fn([])".clone_into(&mut world.script);
}

#[given("a REPL snippet with one external call")]
fn given_repl_snippet(world: &mut SnapshotExtensionsWorld) {
    "ext_fn([])".clone_into(&mut world.repl_snippet);
}

#[given("snapshot extension bytes")]
fn given_snapshot_extension_bytes(world: &mut SnapshotExtensionsWorld) {
    world.snapshot_extension = vec![1, 3, 5, 7];
}

#[when("run progress is dumped and loaded with snapshot extension bytes")]
fn when_run_progress_dumped_and_loaded(world: &mut SnapshotExtensionsWorld) {
    let runner = MontyRun::new(world.script.clone(), "test.py", vec![]).expect("runner creation should succeed");
    let progress = runner
        .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
        .expect("run should suspend")
        .attach_snapshot_extension(world.snapshot_extension.clone());

    let bytes = progress.dump().expect("run progress dump should succeed");
    let loaded: RunProgress<NoLimitTracker> = RunProgress::load(&bytes).expect("run progress load should succeed");
    world.loaded_snapshot_extension = loaded.get_snapshot_extension().map(<[u8]>::to_vec);
}

#[when("run progress payload is corrupted")]
fn when_run_progress_payload_corrupted(world: &mut SnapshotExtensionsWorld) {
    let runner = MontyRun::new(world.script.clone(), "test.py", vec![]).expect("runner creation should succeed");
    let progress = runner
        .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
        .expect("run should suspend")
        .attach_snapshot_extension(world.snapshot_extension.clone());

    let mut bytes = progress
        .dump()
        .expect("run progress dump with snapshot extension should succeed");
    bytes.pop();

    world.load_failed = RunProgress::<NoLimitTracker>::load(&bytes).is_err();
}

#[when("REPL progress is dumped and loaded with snapshot extension bytes")]
fn when_repl_progress_dumped_and_loaded(world: &mut SnapshotExtensionsWorld) {
    let repl = create_repl();
    let progress = repl
        .start(&world.repl_snippet, &mut PrintWriter::Stdout)
        .expect("repl should suspend")
        .attach_snapshot_extension(world.snapshot_extension.clone());

    let bytes = progress.dump().expect("repl progress dump should succeed");
    let loaded: ReplProgress<NoLimitTracker> = ReplProgress::load(&bytes).expect("repl progress load should succeed");

    world.loaded_snapshot_extension = loaded.get_snapshot_extension().map(<[u8]>::to_vec);
}

#[then("the loaded snapshot extension bytes match")]
fn then_loaded_snapshot_extension_matches(world: &SnapshotExtensionsWorld) {
    assert_eq!(
        world.loaded_snapshot_extension.as_deref(),
        Some(world.snapshot_extension.as_slice()),
        "expected snapshot extension bytes to round-trip"
    );
}

#[then("loading the run progress fails")]
fn then_loading_run_progress_fails(world: &SnapshotExtensionsWorld) {
    assert!(world.load_failed, "expected corrupted payload to fail load");
}

#[scenario(
    path = "tests/features/snapshot_extensions.feature",
    name = "Run progress preserves snapshot extension bytes across dump/load"
)]
fn run_snapshot_extension_round_trip(world: SnapshotExtensionsWorld) {
    drop(world);
}

#[scenario(
    path = "tests/features/snapshot_extensions.feature",
    name = "Corrupted run progress payload fails to load"
)]
fn corrupted_run_progress_payload(world: SnapshotExtensionsWorld) {
    drop(world);
}

#[scenario(
    path = "tests/features/snapshot_extensions.feature",
    name = "REPL progress preserves snapshot extension bytes across dump/load"
)]
fn repl_snapshot_extension_round_trip(world: SnapshotExtensionsWorld) {
    drop(world);
}
