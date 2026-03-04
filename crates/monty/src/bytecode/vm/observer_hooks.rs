//! Runtime-observer plumbing for the globals-based VM.
//!
//! This module keeps observer-specific helpers separate from the main VM file
//! so upstream VM changes remain easier to reconcile.

use super::VM;
use crate::{
    observer::{ControlConditionEvent, OpInputIds, OpResultEvent, RuntimeObserverEvent, ValueCreatedEvent},
    resource::ResourceTracker,
    runtime_id::RuntimeValueId,
    value::Value,
};

impl<T: ResourceTracker> VM<'_, T> {
    /// Pushes a value while emitting a `ValueCreated` event when observation is enabled.
    #[inline]
    pub(crate) fn push_created(&mut self, value: Value) {
        self.emit_value_created(&value);
        self.push(value);
    }

    /// Emits a value-creation observer event for a stack value.
    #[inline]
    pub(super) fn emit_value_created(&self, value: &Value) {
        if !self.observer.is_enabled() {
            return;
        }
        self.observer
            .emit(RuntimeObserverEvent::ValueCreated(ValueCreatedEvent {
                value_id: RuntimeValueId::new(value.id()),
            }));
    }

    /// Emits an operation-result observer event.
    #[inline]
    pub(super) fn emit_op_result(&self, output: &Value, inputs: OpInputIds) {
        if !self.observer.is_enabled() {
            return;
        }
        self.observer.emit(RuntimeObserverEvent::OpResult(OpResultEvent {
            output_id: RuntimeValueId::new(output.id()),
            inputs,
        }));
    }

    /// Emits a binary operation-result event.
    #[inline]
    pub(super) fn emit_binary_op_result(&self, lhs: &Value, rhs: &Value, output: &Value) {
        if !self.observer.is_enabled() {
            return;
        }
        self.emit_op_result(
            output,
            OpInputIds::Two(RuntimeValueId::new(lhs.id()), RuntimeValueId::new(rhs.id())),
        );
    }

    /// Emits a control-condition observer event.
    #[inline]
    pub(super) fn emit_control_condition(&self, condition: &Value, branch_taken: bool) {
        if !self.observer.is_enabled() {
            return;
        }
        self.observer
            .emit(RuntimeObserverEvent::ControlCondition(ControlConditionEvent {
                condition_id: RuntimeValueId::new(condition.id()),
                branch_taken,
            }));
    }
}
