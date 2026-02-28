//! Behavioural coverage for generic runtime observer events.

use std::sync::{Arc, Mutex};

use monty::{
    ExcType, ExternalCallKind, ExternalCallReturnKind, MontyException, MontyObject, MontyRun, NoLimitTracker,
    OpInputIds, PrintWriter, RunProgress, RuntimeObserver, RuntimeObserverEvent, RuntimeObserverHandle,
};
use rstest::fixture;
use rstest_bdd_macros::{given, scenario, then, when};

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecordedEvent {
    ExternalCallRequested { call_id: u32, kind: ExternalCallKind },
    ExternalCallReturned { call_id: u32, kind: ExternalCallReturnKind },
    OpResult { input_count: usize },
    ControlCondition,
}

impl RecordedEvent {
    fn from_runtime_event(event: RuntimeObserverEvent<'_>) -> Option<Self> {
        match event {
            RuntimeObserverEvent::ExternalCallRequested(call_event) => Some(Self::ExternalCallRequested {
                call_id: call_event.call_id,
                kind: call_event.kind,
            }),
            RuntimeObserverEvent::ExternalCallReturned(call_event) => Some(Self::ExternalCallReturned {
                call_id: call_event.call_id,
                kind: call_event.kind,
            }),
            RuntimeObserverEvent::ControlCondition(_) => Some(Self::ControlCondition),
            RuntimeObserverEvent::OpResult(op_event) => {
                let input_count = match op_event.inputs {
                    OpInputIds::None => 0,
                    OpInputIds::One(_) => 1,
                    OpInputIds::Two(_, _) => 2,
                };
                Some(Self::OpResult { input_count })
            }
            RuntimeObserverEvent::ValueCreated(_) => None,
        }
    }
}

#[derive(Clone)]
struct RecordingObserver {
    events: Arc<Mutex<Vec<RecordedEvent>>>,
}

impl RecordingObserver {
    fn new(events: Arc<Mutex<Vec<RecordedEvent>>>) -> Self {
        Self { events }
    }
}

impl RuntimeObserver for RecordingObserver {
    fn on_event(&mut self, event: RuntimeObserverEvent<'_>) {
        if let Some(record) = RecordedEvent::from_runtime_event(event) {
            self.events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(record);
        }
    }
}

#[derive(Default)]
struct RuntimeObserverWorld {
    script: String,
    events: Vec<RecordedEvent>,
    call_id: Option<u32>,
}

#[fixture]
fn world() -> RuntimeObserverWorld {
    RuntimeObserverWorld::default()
}

#[given("a suspendable script with one external function call")]
fn given_external_function_script(world: &mut RuntimeObserverWorld) {
    "ext_fn(1)".clone_into(&mut world.script);
}

#[given("a script with arithmetic and branch control flow")]
fn given_branching_script(world: &mut RuntimeObserverWorld) {
    "if x > 0:\n    y = x + 2\nelse:\n    y = x - 2\ny".clone_into(&mut world.script);
}

#[when("execution starts with a recording observer and resumes with integer return value")]
fn when_start_and_resume_with_return(world: &mut RuntimeObserverWorld) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let observer = RuntimeObserverHandle::new(RecordingObserver::new(Arc::clone(&events)));

    let run = MontyRun::new(world.script.clone(), "test.py", vec![], vec!["ext_fn".to_owned()])
        .expect("runner creation should succeed");

    let progress = run
        .start_with_observer(vec![], NoLimitTracker, &mut PrintWriter::Stdout, observer)
        .expect("start should pause at external call");

    let RunProgress::FunctionCall { call_id, state, .. } = progress else {
        panic!("expected function call progress");
    };

    world.call_id = Some(call_id);

    let completion = state
        .run(MontyObject::Int(9), &mut PrintWriter::Stdout)
        .expect("resume should complete");
    assert!(matches!(completion, RunProgress::Complete(MontyObject::Int(9))));

    world
        .events
        .clone_from(&events.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
}

#[when("execution starts with a recording observer and runs to completion")]
fn when_start_and_complete(world: &mut RuntimeObserverWorld) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let observer = RuntimeObserverHandle::new(RecordingObserver::new(Arc::clone(&events)));

    let run = MontyRun::new(world.script.clone(), "test.py", vec!["x".to_owned()], vec![])
        .expect("runner creation should succeed");

    let progress = run
        .start_with_observer(
            vec![MontyObject::Int(1)],
            NoLimitTracker,
            &mut PrintWriter::Stdout,
            observer,
        )
        .expect("start should complete");

    assert!(matches!(progress, RunProgress::Complete(MontyObject::Int(3))));

    world
        .events
        .clone_from(&events.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
}

#[when("execution starts with a recording observer and resumes with raised exception")]
fn when_start_and_resume_with_exception(world: &mut RuntimeObserverWorld) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let observer = RuntimeObserverHandle::new(RecordingObserver::new(Arc::clone(&events)));

    let run = MontyRun::new(world.script.clone(), "test.py", vec![], vec!["ext_fn".to_owned()])
        .expect("runner creation should succeed");

    let progress = run
        .start_with_observer(vec![], NoLimitTracker, &mut PrintWriter::Stdout, observer)
        .expect("start should pause at external call");

    let RunProgress::FunctionCall { call_id, state, .. } = progress else {
        panic!("expected function call progress");
    };

    world.call_id = Some(call_id);

    let error = state
        .run(
            MontyException::new(ExcType::RuntimeError, Some("bdd failure".to_owned())),
            &mut PrintWriter::Stdout,
        )
        .expect_err("resume should return an error");
    assert!(error.to_string().contains("bdd failure"));

    world
        .events
        .clone_from(&events.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
}

#[then("observer events include an external function request")]
fn then_has_external_request(world: &RuntimeObserverWorld) {
    let Some(call_id) = world.call_id else {
        panic!("call id should be recorded");
    };

    assert!(world.events.iter().any(|event| {
        matches!(
            event,
            RecordedEvent::ExternalCallRequested {
                call_id: observed_call_id,
                kind: ExternalCallKind::Function,
            } if *observed_call_id == call_id
        )
    }));
}

#[then("observer events include an external function return")]
fn then_has_external_return(world: &RuntimeObserverWorld) {
    let Some(call_id) = world.call_id else {
        panic!("call id should be recorded");
    };

    assert!(world.events.iter().any(|event| {
        matches!(
            event,
            RecordedEvent::ExternalCallReturned {
                call_id: observed_call_id,
                kind: ExternalCallReturnKind::Return,
            } if *observed_call_id == call_id
        )
    }));
}

#[then("observer events include an external error return")]
fn then_has_external_error_return(world: &RuntimeObserverWorld) {
    let Some(call_id) = world.call_id else {
        panic!("call id should be recorded");
    };

    assert!(world.events.iter().any(|event| {
        matches!(
            event,
            RecordedEvent::ExternalCallReturned {
                call_id: observed_call_id,
                kind: ExternalCallReturnKind::Error,
            } if *observed_call_id == call_id
        )
    }));
}

#[then("observer events include a control condition event")]
fn then_has_control_condition(world: &RuntimeObserverWorld) {
    assert!(
        world
            .events
            .iter()
            .any(|event| matches!(event, RecordedEvent::ControlCondition))
    );
}

#[then("observer events include an operation-result event with inputs")]
fn then_has_op_result_with_inputs(world: &RuntimeObserverWorld) {
    assert!(world.events.iter().any(|event| {
        matches!(
            event,
            RecordedEvent::OpResult {
                input_count,
            } if *input_count >= 1
        )
    }));
}

#[scenario(
    path = "tests/features/runtime_observer_events.feature",
    name = "Function call emits request and return events"
)]
fn function_call_emits_request_and_return(world: RuntimeObserverWorld) {
    drop(world);
}

#[scenario(
    path = "tests/features/runtime_observer_events.feature",
    name = "Branching code emits control and operation-result events"
)]
fn branching_code_emits_control_and_operation_result(world: RuntimeObserverWorld) {
    drop(world);
}

#[scenario(
    path = "tests/features/runtime_observer_events.feature",
    name = "Failed external call emits error return event"
)]
fn failed_call_emits_error_return(world: RuntimeObserverWorld) {
    drop(world);
}
