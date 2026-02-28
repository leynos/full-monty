//! Integration tests for generic runtime observer events.

use std::sync::{Arc, Mutex};

use monty::{
    ExcType, ExternalCallKind, ExternalCallReturnKind, MontyException, MontyObject, MontyRun, NoLimitTracker,
    NoopRuntimeObserver, OpInputIds, PrintWriter, RunProgress, RuntimeObserver, RuntimeObserverEvent,
    RuntimeObserverHandle,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecordedEvent {
    ValueCreated {
        value_id: usize,
    },
    OpResult {
        output_id: usize,
        input_ids: Vec<usize>,
    },
    ExternalCallRequested {
        call_id: u32,
        kind: ExternalCallKind,
        arg_runtime_ids: Vec<usize>,
        kwarg_runtime_ids: Vec<(usize, usize)>,
    },
    ExternalCallReturned {
        call_id: u32,
        kind: ExternalCallReturnKind,
    },
    ControlCondition {
        condition_id: usize,
        branch_taken: bool,
    },
}

impl RecordedEvent {
    fn from_runtime_event(event: RuntimeObserverEvent<'_>) -> Self {
        match event {
            RuntimeObserverEvent::ValueCreated(value_event) => Self::ValueCreated {
                value_id: value_event.value_id.raw(),
            },
            RuntimeObserverEvent::OpResult(op_event) => Self::OpResult {
                output_id: op_event.output_id.raw(),
                input_ids: match op_event.inputs {
                    OpInputIds::None => vec![],
                    OpInputIds::One(id) => vec![id.raw()],
                    OpInputIds::Two(first, second) => vec![first.raw(), second.raw()],
                },
            },
            RuntimeObserverEvent::ExternalCallRequested(call_event) => Self::ExternalCallRequested {
                call_id: call_event.call_id,
                kind: call_event.kind,
                arg_runtime_ids: call_event.arg_runtime_ids.iter().map(|id| id.raw()).collect(),
                kwarg_runtime_ids: call_event
                    .kwarg_runtime_ids
                    .iter()
                    .map(|(key, value)| (key.raw(), value.raw()))
                    .collect(),
            },
            RuntimeObserverEvent::ExternalCallReturned(call_event) => Self::ExternalCallReturned {
                call_id: call_event.call_id,
                kind: call_event.kind,
            },
            RuntimeObserverEvent::ControlCondition(control_event) => Self::ControlCondition {
                condition_id: control_event.condition_id.raw(),
                branch_taken: control_event.branch_taken,
            },
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
        let mut events = self.events.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        events.push(RecordedEvent::from_runtime_event(event));
    }
}

fn build_recording_observer() -> (RuntimeObserverHandle, Arc<Mutex<Vec<RecordedEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let handle = RuntimeObserverHandle::new(RecordingObserver::new(Arc::clone(&events)));
    (handle, events)
}

fn read_events(events: &Arc<Mutex<Vec<RecordedEvent>>>) -> Vec<RecordedEvent> {
    events.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

#[test]
fn runtime_observer_tracks_external_function_request_and_return() {
    let (observer, events) = build_recording_observer();
    let run = MontyRun::new("ext_fn(1)".to_owned(), "test.py", vec![], vec!["ext_fn".to_owned()])
        .expect("runner creation should succeed");

    let progress = run
        .start_with_observer(vec![], NoLimitTracker, &mut PrintWriter::Stdout, observer)
        .expect("start should pause at external call");

    let RunProgress::FunctionCall { call_id, state, .. } = progress else {
        panic!("expected function-call progress");
    };

    let completion = state
        .run(MontyObject::Int(7), &mut PrintWriter::Stdout)
        .expect("resume should complete");
    assert!(matches!(completion, RunProgress::Complete(MontyObject::Int(7))));

    let events = read_events(&events);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            RecordedEvent::ExternalCallRequested {
                call_id: observed_call_id,
                kind: ExternalCallKind::Function,
                ..
            } if *observed_call_id == call_id
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            RecordedEvent::ExternalCallReturned {
                call_id: observed_call_id,
                kind: ExternalCallReturnKind::Return,
            } if *observed_call_id == call_id
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            RecordedEvent::OpResult {
                input_ids,
                ..
            } if input_ids.is_empty()
        )
    }));
}

#[test]
fn runtime_observer_tracks_os_call_requests() {
    let (observer, events) = build_recording_observer();
    let run = MontyRun::new(
        "from pathlib import Path\nPath('/tmp/observer-test').exists()".to_owned(),
        "test.py",
        vec![],
        vec![],
    )
    .expect("runner creation should succeed");

    let progress = run
        .start_with_observer(vec![], NoLimitTracker, &mut PrintWriter::Stdout, observer)
        .expect("start should pause at OS call");

    let RunProgress::OsCall { state, .. } = progress else {
        panic!("expected OS-call progress");
    };

    let completion = state
        .run(MontyObject::Bool(false), &mut PrintWriter::Stdout)
        .expect("OS call resume should complete");
    assert!(matches!(completion, RunProgress::Complete(MontyObject::Bool(false))));

    let events = read_events(&events);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            RecordedEvent::ExternalCallRequested {
                kind: ExternalCallKind::Os,
                ..
            }
        )
    }));
}

#[test]
fn runtime_observer_emits_control_and_operation_events_for_branching_code() {
    let (observer, events) = build_recording_observer();
    let run = MontyRun::new(
        "if x > 0:\n    y = x + 2\nelse:\n    y = x - 2\ny".to_owned(),
        "test.py",
        vec!["x".to_owned()],
        vec![],
    )
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

    let events = read_events(&events);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RecordedEvent::ControlCondition { .. }))
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            RecordedEvent::OpResult {
                input_ids,
                ..
            } if input_ids.len() == 2
        )
    }));
}

#[test]
fn runtime_observer_emits_error_return_for_failed_external_call() {
    let (observer, events) = build_recording_observer();
    let run = MontyRun::new("ext_fn(1)".to_owned(), "test.py", vec![], vec!["ext_fn".to_owned()])
        .expect("runner creation should succeed");

    let progress = run
        .start_with_observer(vec![], NoLimitTracker, &mut PrintWriter::Stdout, observer)
        .expect("start should pause at external call");

    let RunProgress::FunctionCall { call_id, state, .. } = progress else {
        panic!("expected function-call progress");
    };

    let error = state
        .run(
            MontyException::new(ExcType::RuntimeError, Some("observer failure".to_owned())),
            &mut PrintWriter::Stdout,
        )
        .expect_err("resume should return an error");
    assert!(error.to_string().contains("observer failure"));

    let events = read_events(&events);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            RecordedEvent::ExternalCallReturned {
                call_id: observed_call_id,
                kind: ExternalCallReturnKind::Error,
            } if *observed_call_id == call_id
        )
    }));
}

#[test]
fn noop_observer_preserves_suspend_resume_semantics() {
    let script = "ext_fn(1); ext_fn(2); 3";

    let run_without_observer = MontyRun::new(script.to_owned(), "test.py", vec![], vec!["ext_fn".to_owned()])
        .expect("runner creation should succeed");
    let first_without = run_without_observer
        .start(vec![], NoLimitTracker, &mut PrintWriter::Stdout)
        .expect("start should pause at first call");

    let run_with_noop = MontyRun::new(script.to_owned(), "test.py", vec![], vec!["ext_fn".to_owned()])
        .expect("runner creation should succeed");
    let first_with_noop = run_with_noop
        .start_with_observer(
            vec![],
            NoLimitTracker,
            &mut PrintWriter::Stdout,
            RuntimeObserverHandle::new(NoopRuntimeObserver),
        )
        .expect("start should pause at first call");

    let RunProgress::FunctionCall {
        function_name: first_name_without,
        args: first_args_without,
        kwargs: first_kwargs_without,
        state: first_state_without,
        ..
    } = first_without
    else {
        panic!("expected function-call progress without observer");
    };

    let RunProgress::FunctionCall {
        function_name: first_name_with,
        args: first_args_with,
        kwargs: first_kwargs_with,
        state: first_state_with,
        ..
    } = first_with_noop
    else {
        panic!("expected function-call progress with no-op observer");
    };

    assert_eq!(first_name_without, first_name_with);
    assert_eq!(first_args_without, first_args_with);
    assert_eq!(first_kwargs_without, first_kwargs_with);

    let second_without = first_state_without
        .run(MontyObject::None, &mut PrintWriter::Stdout)
        .expect("resume should pause at second call");
    let second_with_noop = first_state_with
        .run(MontyObject::None, &mut PrintWriter::Stdout)
        .expect("resume should pause at second call");

    let RunProgress::FunctionCall {
        function_name: second_name_without,
        args: second_args_without,
        kwargs: second_kwargs_without,
        state: second_state_without,
        ..
    } = second_without
    else {
        panic!("expected second function-call progress without observer");
    };

    let RunProgress::FunctionCall {
        function_name: second_name_with,
        args: second_args_with,
        kwargs: second_kwargs_with,
        state: second_state_with,
        ..
    } = second_with_noop
    else {
        panic!("expected second function-call progress with no-op observer");
    };

    assert_eq!(second_name_without, second_name_with);
    assert_eq!(second_args_without, second_args_with);
    assert_eq!(second_kwargs_without, second_kwargs_with);

    let completion_without = second_state_without
        .run(MontyObject::None, &mut PrintWriter::Stdout)
        .expect("final resume should complete without observer");
    let completion_with_noop = second_state_with
        .run(MontyObject::None, &mut PrintWriter::Stdout)
        .expect("final resume should complete with no-op observer");

    assert_eq!(completion_without.into_complete(), completion_with_noop.into_complete());
}
