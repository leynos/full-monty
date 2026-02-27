//! Behavioural coverage for stable host-facing runtime IDs.

use monty::{MontyObject, MontyRun, NoLimitTracker, PrintWriter, RunProgress};
use rstest::fixture;
use rstest_bdd_macros::{given, scenario, then, when};

#[derive(Default)]
struct RuntimeIdsWorld {
    script: String,
    first_runtime_ids: Vec<usize>,
    second_runtime_ids: Vec<usize>,
    load_failed: bool,
}

#[fixture]
fn world() -> RuntimeIdsWorld {
    RuntimeIdsWorld::default()
}

#[given("a suspendable script with repeated object external calls")]
fn given_repeated_object_calls(world: &mut RuntimeIdsWorld) {
    "x = []; ext_fn(x); ext_fn(x)".clone_into(&mut world.script);
}

#[given("a suspendable script with one external call")]
fn given_one_external_call(world: &mut RuntimeIdsWorld) {
    "ext_fn([])".clone_into(&mut world.script);
}

#[when("execution pauses and resumes through both external calls")]
fn when_pause_and_resume_twice(world: &mut RuntimeIdsWorld) {
    let runner = MontyRun::new(world.script.clone(), "test.py", vec![], vec!["ext_fn".to_owned()])
        .expect("runner creation should succeed");
    let progress = runner
        .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
        .expect("first run should pause at external call");

    let RunProgress::FunctionCall {
        arg_runtime_ids, state, ..
    } = progress
    else {
        panic!("expected first function call pause");
    };

    world.first_runtime_ids = arg_runtime_ids.iter().map(|id| id.raw()).collect();

    let progress = state
        .run(MontyObject::None, &mut PrintWriter::Stdout)
        .expect("resume should reach second function call");
    let RunProgress::FunctionCall {
        arg_runtime_ids, state, ..
    } = progress
    else {
        panic!("expected second function call pause");
    };

    world.second_runtime_ids = arg_runtime_ids.iter().map(|id| id.raw()).collect();

    let completion = state
        .run(MontyObject::None, &mut PrintWriter::Stdout)
        .expect("final resume should complete");
    assert!(matches!(completion, RunProgress::Complete(_)));
}

#[when("run progress is dumped and loaded")]
fn when_progress_is_dumped_and_loaded(world: &mut RuntimeIdsWorld) {
    let runner = MontyRun::new(world.script.clone(), "test.py", vec![], vec!["ext_fn".to_owned()])
        .expect("runner creation should succeed");
    let progress = runner
        .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
        .expect("run should pause at external call");

    world.first_runtime_ids = progress
        .runtime_ids()
        .expect("function call should provide runtime ids")
        .0
        .iter()
        .map(|id| id.raw())
        .collect();

    let bytes = progress.dump().expect("run progress dump should succeed");
    let loaded: RunProgress<NoLimitTracker> = RunProgress::load(&bytes).expect("run progress load should succeed");

    world.second_runtime_ids = loaded
        .runtime_ids()
        .expect("loaded function call should provide runtime ids")
        .0
        .iter()
        .map(|id| id.raw())
        .collect();
}

#[when("the serialized run progress payload is corrupted before load")]
fn when_progress_payload_is_corrupted(world: &mut RuntimeIdsWorld) {
    let runner = MontyRun::new(world.script.clone(), "test.py", vec![], vec!["ext_fn".to_owned()])
        .expect("runner creation should succeed");
    let progress = runner
        .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
        .expect("run should pause at external call");

    let mut bytes = progress.dump().expect("run progress dump should succeed");
    assert!(!bytes.is_empty(), "serialized run progress should not be empty");
    bytes[0] ^= 0xFF;

    world.load_failed = RunProgress::<NoLimitTracker>::load(&bytes).is_err();
}

#[then("the first and second runtime ID vectors are equal")]
fn then_runtime_ids_match(world: &RuntimeIdsWorld) {
    assert_eq!(world.first_runtime_ids, world.second_runtime_ids);
}

#[then("the runtime ID vectors are non-empty")]
fn then_runtime_ids_non_empty(world: &RuntimeIdsWorld) {
    assert!(!world.first_runtime_ids.is_empty());
    assert!(!world.second_runtime_ids.is_empty());
}

#[then("loading the progress payload fails")]
fn then_payload_load_fails(world: &RuntimeIdsWorld) {
    assert!(world.load_failed, "expected corrupted payload load to fail");
}

#[scenario(
    path = "tests/features/runtime_ids.feature",
    name = "Runtime IDs remain stable across resume for persistent objects"
)]
fn runtime_ids_are_stable_across_resume(world: RuntimeIdsWorld) {
    drop(world);
}

#[scenario(
    path = "tests/features/runtime_ids.feature",
    name = "Runtime IDs survive run progress dump and load"
)]
fn runtime_ids_survive_dump_and_load(world: RuntimeIdsWorld) {
    drop(world);
}

#[scenario(
    path = "tests/features/runtime_ids.feature",
    name = "Corrupted run progress payload fails load"
)]
fn corrupted_payload_fails_load(world: RuntimeIdsWorld) {
    drop(world);
}
