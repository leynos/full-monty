//! Stateful REPL execution support for Monty.
//!
//! This module implements incremental snippet execution where each new snippet
//! is compiled and executed against persistent heap/namespace state without
//! replaying previously executed snippets.

use std::mem;

use ahash::AHashMap;
use ruff_python_ast::token::TokenKind;
use ruff_python_parser::{InterpolatedStringErrorType, LexicalErrorType, ParseErrorType, parse_module};
use serde::de::DeserializeOwned;

use crate::{
    ExcType, MontyException,
    args::{ArgValues, KwargsValues},
    asyncio::CallId,
    bytecode::{VM, VMSnapshot},
    defer_drop,
    exception_private::RunError,
    heap::{DropWithHeap, Heap, HeapReader},
    heap_data::HeapData,
    intern::{InternerBuilder, Interns},
    io::PrintWriter,
    namespace::NamespaceId,
    object::MontyObject,
    observer::{
        ExternalCallKind, ExternalCallRequestedEvent, ExternalCallReturnKind, ExternalCallReturnedEvent,
        RuntimeObserverEvent, RuntimeObserverHandle,
    },
    os::OsFunction,
    resource::ResourceTracker,
    run::Executor,
    run_progress::{ConvertedExit, ExtFunctionResult, NameLookupResult, convert_frame_exit},
    runtime_id::RuntimeValueId,
    snapshot_extension::{SnapshotExtension, clone_snapshot_extension},
    value::Value,
};

type RuntimeIdSlices<'a> = (&'a [RuntimeValueId], &'a [(RuntimeValueId, RuntimeValueId)]);

/// Stateful REPL session that executes snippets incrementally without replay.
///
/// `MontyRepl` preserves heap and global variable state between snippets.
/// Each `feed()` compiles and executes only the new snippet against the current
/// state, avoiding the cost and semantic risks of replaying prior code.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "T: serde::Serialize", deserialize = "T: DeserializeOwned"))]
pub struct MontyRepl<T: ResourceTracker> {
    /// Script name used for runtime error messages and REPL identification.
    ///
    /// Incremental `feed()` / `start()` snippets intentionally use internal script names
    /// like `<python-input-0>` to match CPython's interactive traceback style.
    script_name: String,
    /// Counter for generated `<python-input-N>` snippet filenames.
    next_input_id: u64,
    /// Stable mapping of global variable names to namespace slot IDs.
    global_name_map: AHashMap<String, NamespaceId>,
    /// Persistent intern table across snippets so intern/function IDs remain valid.
    interns: Interns,
    /// Source text of every snippet that has been fed, keyed by its
    /// generated script name (`<python-input-N>`).
    ///
    /// Required because a traceback raised in snippet N can include frames
    /// from functions defined in snippet M < N. Those frames carry
    /// `CodeRange` byte offsets that index into snippet M's source, so the
    /// diagnostic pass must be able to look that source up by filename —
    /// the current snippet's `Executor.code` is not sufficient.
    #[serde(default)]
    sources: AHashMap<String, String>,
    /// Persistent heap across snippets.
    heap: Heap<T>,
    /// Persistent global variable values across snippets.
    ///
    /// Indexed by `NamespaceId` slots from `global_name_map`. Between snippet
    /// executions these are the only VM values that persist — stack and frames
    /// are transient.
    globals: Vec<Value>,
}

impl<T: ResourceTracker> MontyRepl<T> {
    /// Creates an empty REPL session with no code parsed or executed.
    ///
    /// All code execution is driven through `feed_run()` or `feed_start()`. This separates
    /// construction from execution, matching the pattern used by `MontyRun::new()`.
    #[must_use]
    pub fn new(script_name: &str, resource_tracker: T) -> Self {
        let heap = Heap::new(0, resource_tracker);

        Self {
            script_name: script_name.to_owned(),
            next_input_id: 0,
            global_name_map: AHashMap::new(),
            interns: Interns::new(InternerBuilder::default(), Vec::new()),
            sources: AHashMap::new(),
            heap,
            globals: Vec::new(),
        }
    }

    /// Returns the resource tracker that will be used for the next snippet.
    ///
    /// This is primarily intended for host integrations that need to attach
    /// per-execution state, such as cancellation markers, to an existing REPL.
    pub fn tracker(&self) -> &T {
        self.heap.tracker()
    }

    /// Returns mutable access to the resource tracker for the next snippet.
    ///
    /// REPL hosts use this to install ephemeral execution controls, such as
    /// async cancellation flags, before calling `feed_start()`.
    pub fn tracker_mut(&mut self) -> &mut T {
        self.heap.tracker_mut()
    }

    /// Starts executing a new snippet and returns suspendable REPL progress.
    ///
    /// This is the REPL equivalent of `MontyRun::start`: execution may complete,
    /// suspend at external calls / OS calls / unresolved futures, or raise a Python
    /// exception. Resume with the returned state object and eventually recover the
    /// updated REPL from `ReplProgress::into_complete`.
    ///
    /// Unlike `MontyRepl::feed`, this method consumes `self` so runtime state can be
    /// safely moved into snapshot objects for serialization and cross-process resume.
    ///
    /// On a Python-level runtime exception the REPL is **not** destroyed: it is
    /// returned inside `ReplStartError` so the caller can continue feeding
    /// subsequent snippets against the same heap and namespace state.
    ///
    /// # Errors
    /// Returns `Err(Box<ReplStartError>)` for syntax, compile-time, or runtime
    /// failures — the REPL session is always preserved inside the error.
    pub fn feed_start(
        self,
        code: &str,
        inputs: Vec<(String, MontyObject)>,
        print: PrintWriter<'_>,
    ) -> Result<ReplProgress<T>, Box<ReplStartError<T>>> {
        self.feed_start_with_observer(code, inputs, print, RuntimeObserverHandle::disabled())
    }

    /// Starts executing a new snippet with a runtime observer for host instrumentation.
    pub fn feed_start_with_observer(
        self,
        code: &str,
        inputs: Vec<(String, MontyObject)>,
        print: PrintWriter<'_>,
        observer: RuntimeObserverHandle,
    ) -> Result<ReplProgress<T>, Box<ReplStartError<T>>> {
        let mut this = self;
        if code.is_empty() {
            return Ok(ReplProgress::Complete {
                repl: this,
                value: MontyObject::None,
            });
        }

        let (input_names, input_values): (Vec<_>, Vec<_>) = inputs.into_iter().unzip();

        let input_script_name = this.next_input_script_name();
        // Preserve this snippet's source (see `feed_run` for rationale).
        this.sources.insert(input_script_name.clone(), code.to_owned());
        let executor = match Executor::new_repl_snippet(
            code.to_owned(),
            &input_script_name,
            this.global_name_map.clone(),
            &this.interns,
            input_names,
        ) {
            Ok(exec) => exec,
            Err(error) => return Err(Box::new(ReplStartError { repl: this, error })),
        };

        this.ensure_globals_size(executor.namespace_size);

        match HeapReader::with(
            &mut this.heap,
            &mut (&executor, print, &observer),
            |reader, (executor, print, observer)| {
                let mut vm = VM::new_with_observer(
                    mem::take(&mut this.globals),
                    reader,
                    &executor.interns,
                    print.reborrow(),
                    (*observer).clone(),
                );

                // Inject inputs with VM alive
                if let Err(error) = inject_inputs_into_vm(executor, input_values, &mut vm) {
                    this.globals = vm.take_globals();
                    return Err(error);
                }

                let vm_result = vm.run_module(&executor.module_code);

                // Convert while VM alive, then snapshot or reclaim globals
                let converted = convert_frame_exit(vm_result, &mut vm);
                let vm_state = if converted.needs_snapshot() {
                    Some(vm.snapshot())
                } else {
                    this.globals = vm.take_globals();
                    None
                };
                Ok((converted, vm_state))
            },
        ) {
            Ok((converted, vm_state)) => build_repl_progress(converted, vm_state, executor, this, observer, None),
            Err(error) => Err(Box::new(ReplStartError { repl: this, error })),
        }
    }

    /// Convenience wrapper for starting a snippet without explicit inputs.
    pub fn start_with_observer(
        self,
        code: &str,
        print: PrintWriter<'_>,
        observer: RuntimeObserverHandle,
    ) -> Result<ReplProgress<T>, Box<ReplStartError<T>>> {
        self.feed_start_with_observer(code, vec![], print, observer)
    }

    /// Feeds and executes a new snippet against the current REPL state to completion.
    ///
    /// This compiles only `code` using the existing global slot map, extends the
    /// global namespace if new names are introduced, and executes the snippet once.
    /// Previously executed snippets are never replayed. If execution raises after
    /// partially mutating globals, those mutations remain visible in later feeds,
    /// matching Python REPL semantics.
    ///
    /// # Errors
    /// Returns `MontyException` for syntax/compile/runtime failures.
    pub fn feed_run(
        &mut self,
        code: &str,
        inputs: Vec<(String, MontyObject)>,
        print: PrintWriter<'_>,
    ) -> Result<MontyObject, MontyException> {
        if code.is_empty() {
            return Ok(MontyObject::None);
        }

        let (input_names, input_values): (Vec<_>, Vec<_>) = inputs.into_iter().unzip();

        let input_script_name = self.next_input_script_name();
        // Preserve this snippet's source before anything can fail, so later
        // tracebacks with frames from this snippet can still resolve line/
        // column/preview information — `Executor.code` only survives until
        // the next feed.
        self.sources.insert(input_script_name.clone(), code.to_owned());
        let executor = Executor::new_repl_snippet(
            code.to_owned(),
            &input_script_name,
            self.global_name_map.clone(),
            &self.interns,
            input_names,
        )?;

        self.ensure_globals_size(executor.namespace_size);

        let result = HeapReader::with(&mut self.heap, &mut (&executor, print), |reader, (executor, print)| {
            let mut vm = VM::new(
                mem::take(&mut self.globals),
                reader,
                &executor.interns,
                print.reborrow(),
            );

            if let Err(e) = inject_inputs_into_vm(executor, input_values, &mut vm) {
                self.globals = vm.take_globals();
                return Err(e);
            }

            let result = executor.run_to_completion(&mut vm);

            // Reclaim globals before cleanup.
            self.globals = vm.take_globals();
            Ok(result)
        })?;

        // Commit compiler metadata even on runtime errors.
        // Snippets can mutate globals before raising, and those values may contain
        // FunctionId/StringId values that must be interpreted with the updated tables.
        let Executor { name_map, interns, .. } = executor;
        self.global_name_map = name_map;
        self.interns = interns;

        // Resolve every traceback frame against the source of the snippet that
        // produced it — frames from earlier snippets live in `self.sources`.
        result.map_err(|e| e.into_python_exception(&self.interns, |fname| self.sources.get(fname).map(String::as_str)))
    }

    /// Calls a Python function defined in the session by name.
    ///
    /// Looks up the function in the global namespace, converts the arguments,
    /// executes the function, and converts the result back.
    ///
    /// # Errors
    /// Returns `MontyException` if the function is not found, not callable,
    /// raises an exception, or encounters an external function call.
    pub fn call_function(
        &mut self,
        name: &str,
        args: Vec<MontyObject>,
        print: PrintWriter<'_>,
    ) -> Result<MontyObject, MontyException> {
        let Some(slot_idx) = self.global_name_map.get(name) else {
            return Err(RunError::from(ExcType::name_error(name))
                .into_python_exception(&self.interns, |fname| self.sources.get(fname).map(String::as_str)));
        };

        HeapReader::with(
            &mut self.heap,
            &mut (&self.interns, print),
            |reader, (interns, print)| {
                let vm = &mut VM::new(mem::take(&mut self.globals), reader, interns, print.reborrow());

                let callable = vm.globals[slot_idx.index()].clone_with_heap(vm);
                defer_drop!(callable, vm);

                let arg_values = match convert_args(args, vm) {
                    Ok(av) => av,
                    Err(e) => {
                        self.globals = vm.take_globals();
                        return Err(e);
                    }
                };

                let result = match vm.evaluate_function("MontyRepl::call_function", callable, arg_values) {
                    Ok(value) => Ok(MontyObject::new(value, vm)),
                    Err(e) => {
                        Err(e.into_python_exception(&self.interns, |fname| self.sources.get(fname).map(String::as_str)))
                    }
                };

                self.globals = vm.take_globals();

                result
            },
        )
    }

    /// Returns a list of all callable function names defined in the session.
    ///
    /// Includes functions, closures, and functions with default arguments.
    /// Does not include builtins or external functions.
    #[must_use]
    pub fn function_names(&self) -> Vec<&str> {
        self.global_name_map
            .iter()
            .filter_map(|(name, ns_id)| {
                let idx = ns_id.index();
                if idx < self.globals.len() && is_callable(&self.globals[idx], &self.heap) {
                    Some(name.as_str())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns whether a function with the given name exists in the session.
    #[must_use]
    pub fn has_function(&self, name: &str) -> bool {
        self.global_name_map.get(name).is_some_and(|ns_id| {
            let idx = ns_id.index();
            idx < self.globals.len() && is_callable(&self.globals[idx], &self.heap)
        })
    }

    /// Grows the globals vector to at least `size` slots.
    ///
    /// Newly introduced slots are initialized to `Undefined` to keep slot alignment
    /// with the compiler's global-name map.
    fn ensure_globals_size(&mut self, size: usize) {
        if self.globals.len() < size {
            self.globals.resize_with(size, || Value::Undefined);
        }
    }

    /// Returns the generated filename for the next interactive snippet.
    ///
    /// CPython labels interactive snippets as `<python-input-N>` and increments
    /// N for each feed attempt. Matching this improves traceback ergonomics and
    /// makes REPL errors easier to correlate with user input history.
    fn next_input_script_name(&mut self) -> String {
        let input_id = self.next_input_id;
        self.next_input_id += 1;
        format!("<python-input-{input_id}>")
    }
}

impl<T: ResourceTracker + serde::Serialize> MontyRepl<T> {
    /// Serializes the REPL session state to bytes.
    ///
    /// This includes heap + globals + global slot mapping, allowing snapshot/restore
    /// of interactive state between process runs.
    ///
    /// # Errors
    /// Returns an error if serialization fails.
    pub fn dump(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }
}

impl<T: ResourceTracker + DeserializeOwned> MontyRepl<T> {
    /// Restores a REPL session from bytes produced by `MontyRepl::dump`.
    ///
    /// # Errors
    /// Returns an error if deserialization fails.
    pub fn load(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}

impl<T: ResourceTracker> Drop for MontyRepl<T> {
    fn drop(&mut self) {
        self.globals.drain(..).drop_with_heap(&mut self.heap);
    }
}

// ---------------------------------------------------------------------------
// ReplProgress and per-variant structs
// ---------------------------------------------------------------------------

/// Result of a single suspendable REPL snippet execution.
///
/// This mirrors `RunProgress` but returns the updated `MontyRepl` on completion
/// so callers can continue feeding additional snippets without replaying prior code.
/// Each variant (except `Complete`) wraps a dedicated struct with only the relevant
/// resume methods.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "T: serde::Serialize", deserialize = "T: DeserializeOwned"))]
pub enum ReplProgress<T: ResourceTracker> {
    /// Execution paused at an external function call or dataclass method call.
    FunctionCall(ReplFunctionCall<T>),
    /// Execution paused for an OS-level operation.
    OsCall(ReplOsCall<T>),
    /// All async tasks are blocked waiting for external futures to resolve.
    ResolveFutures(ReplResolveFutures<T>),
    /// Execution paused for an unresolved name lookup.
    NameLookup(ReplNameLookup<T>),
    /// Snippet execution completed with the updated REPL and result value.
    Complete {
        /// Updated REPL session state to continue feeding snippets.
        repl: MontyRepl<T>,
        /// Final result produced by the snippet.
        value: MontyObject,
    },
}

/// Error returned when a REPL snippet raises a Python exception during `start()` or `resume()`.
///
/// Unlike syntax/compile errors which consume the REPL, runtime errors preserve
/// the full session state so the caller can inspect the error and continue feeding
/// subsequent snippets. Any global mutations that occurred before the exception
/// remain visible in the returned `repl`.
#[derive(Debug)]
pub struct ReplStartError<T: ResourceTracker> {
    /// REPL session state after the failed snippet — ready for further use.
    pub repl: MontyRepl<T>,
    /// The Python exception that was raised.
    pub error: MontyException,
}

impl<T: ResourceTracker> ReplProgress<T> {
    /// Consumes the progress and returns the `ReplFunctionCall` struct.
    #[must_use]
    pub fn into_function_call(self) -> Option<ReplFunctionCall<T>> {
        match self {
            Self::FunctionCall(call) => Some(call),
            _ => None,
        }
    }

    /// Consumes the progress and returns the `ReplResolveFutures` struct.
    #[must_use]
    pub fn into_resolve_futures(self) -> Option<ReplResolveFutures<T>> {
        match self {
            Self::ResolveFutures(state) => Some(state),
            _ => None,
        }
    }

    /// Consumes the progress and returns the `ReplNameLookup` struct.
    #[must_use]
    pub fn into_name_lookup(self) -> Option<ReplNameLookup<T>> {
        match self {
            Self::NameLookup(lookup) => Some(lookup),
            _ => None,
        }
    }

    /// Consumes the progress and returns the completed REPL and value.
    #[must_use]
    pub fn into_complete(self) -> Option<(MontyRepl<T>, MontyObject)> {
        match self {
            Self::Complete { repl, value } => Some((repl, value)),
            _ => None,
        }
    }

    /// Returns runtime-ID slices for suspendable call variants.
    #[must_use]
    pub fn runtime_ids(&self) -> Option<RuntimeIdSlices<'_>> {
        match self {
            Self::FunctionCall(call) => Some((&call.arg_runtime_ids, &call.kwarg_runtime_ids)),
            Self::OsCall(call) => Some((&call.arg_runtime_ids, &call.kwarg_runtime_ids)),
            _ => None,
        }
    }

    /// Extracts the REPL session from any progress variant, discarding
    /// the in-flight execution state.
    ///
    /// Use this to recover the REPL when you need to abandon the current
    /// snippet (e.g. because `feed_run` doesn't support async futures).
    /// The REPL state reflects any mutations that occurred before the
    /// snapshot was taken.
    #[must_use]
    pub fn into_repl(self) -> MontyRepl<T> {
        match self {
            Self::FunctionCall(call) => call.into_repl(),
            Self::OsCall(call) => call.into_repl(),
            Self::ResolveFutures(state) => state.into_repl(),
            Self::NameLookup(lookup) => lookup.into_repl(),
            Self::Complete { repl, .. } => repl,
        }
    }
}

impl<T: ResourceTracker + serde::Serialize> ReplProgress<T> {
    /// Serializes the REPL execution progress to a binary format.
    ///
    /// # Errors
    /// Returns an error if serialization fails.
    pub fn dump(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }
}

impl<T: ResourceTracker + DeserializeOwned> ReplProgress<T> {
    /// Deserializes REPL execution progress from a binary format.
    ///
    /// # Errors
    /// Returns an error if deserialization fails.
    pub fn load(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}

// ---------------------------------------------------------------------------
// ReplFunctionCall
// ---------------------------------------------------------------------------

/// REPL execution paused at an external function call or dataclass method call.
///
/// Resume with `resume(result, print)` to provide the return value and continue,
/// or `resume_pending(print)` to push an `ExternalFuture` for async resolution.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "T: serde::Serialize", deserialize = "T: DeserializeOwned"))]
pub struct ReplFunctionCall<T: ResourceTracker> {
    /// The name of the function or method being called.
    pub function_name: String,
    /// The positional arguments passed to the function.
    pub args: Vec<MontyObject>,
    /// The keyword arguments passed to the function (key, value pairs).
    pub kwargs: Vec<(MontyObject, MontyObject)>,
    /// Stable runtime IDs for positional arguments.
    pub arg_runtime_ids: Vec<RuntimeValueId>,
    /// Stable runtime IDs for keyword `(key, value)` pairs.
    pub kwarg_runtime_ids: Vec<(RuntimeValueId, RuntimeValueId)>,
    /// Unique identifier for this call (used for async correlation).
    pub call_id: u32,
    /// Whether this is a dataclass method call (first arg is `self`).
    pub method_call: bool,
    /// Internal REPL execution snapshot.
    snapshot: ReplSnapshot<T>,
}

impl<T: ResourceTracker> ReplFunctionCall<T> {
    /// Attaches embedder-owned snapshot extension bytes to this suspended state.
    #[must_use]
    pub fn with_snapshot_extension(mut self, snapshot_extension: impl Into<SnapshotExtension>) -> Self {
        self.snapshot = self.snapshot.with_snapshot_extension(snapshot_extension);
        self
    }

    /// Returns the embedder-owned snapshot extension bytes, if present.
    #[must_use]
    pub fn snapshot_extension(&self) -> Option<&SnapshotExtension> {
        self.snapshot.snapshot_extension()
    }

    /// Extracts the REPL session, discarding the in-flight execution state.
    ///
    /// Restores globals from the VM snapshot so the REPL remains usable.
    #[must_use]
    pub fn into_repl(self) -> MontyRepl<T> {
        self.snapshot.into_repl()
    }

    /// Resumes snippet execution with an external result.
    pub fn resume(
        self,
        result: impl Into<ExtFunctionResult>,
        print: PrintWriter<'_>,
    ) -> Result<ReplProgress<T>, Box<ReplStartError<T>>> {
        self.snapshot.run(result, print)
    }

    /// Resumes execution by pushing an `ExternalFuture` for async resolution.
    ///
    /// Uses `self.call_id` internally — no need to pass it again.
    pub fn resume_pending(self, print: PrintWriter<'_>) -> Result<ReplProgress<T>, Box<ReplStartError<T>>> {
        self.snapshot.run(ExtFunctionResult::Future(self.call_id), print)
    }
}

// ---------------------------------------------------------------------------
// ReplOsCall
// ---------------------------------------------------------------------------

/// REPL execution paused for an OS-level operation.
///
/// Resume with `resume(result, print)` to provide the OS call result and continue.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "T: serde::Serialize", deserialize = "T: DeserializeOwned"))]
pub struct ReplOsCall<T: ResourceTracker> {
    /// The OS function to execute.
    pub function: OsFunction,
    /// The positional arguments for the OS function.
    pub args: Vec<MontyObject>,
    /// The keyword arguments passed to the function (key, value pairs).
    pub kwargs: Vec<(MontyObject, MontyObject)>,
    /// Stable runtime IDs for positional arguments.
    pub arg_runtime_ids: Vec<RuntimeValueId>,
    /// Stable runtime IDs for keyword `(key, value)` pairs.
    pub kwarg_runtime_ids: Vec<(RuntimeValueId, RuntimeValueId)>,
    /// Unique identifier for this call (used for async correlation).
    pub call_id: u32,
    /// Internal REPL execution snapshot.
    snapshot: ReplSnapshot<T>,
}

impl<T: ResourceTracker> ReplOsCall<T> {
    /// Attaches embedder-owned snapshot extension bytes to this suspended state.
    #[must_use]
    pub fn with_snapshot_extension(mut self, snapshot_extension: impl Into<SnapshotExtension>) -> Self {
        self.snapshot = self.snapshot.with_snapshot_extension(snapshot_extension);
        self
    }

    /// Returns the embedder-owned snapshot extension bytes, if present.
    #[must_use]
    pub fn snapshot_extension(&self) -> Option<&SnapshotExtension> {
        self.snapshot.snapshot_extension()
    }

    /// Extracts the REPL session, discarding the in-flight execution state.
    ///
    /// Restores globals from the VM snapshot so the REPL remains usable.
    #[must_use]
    pub fn into_repl(self) -> MontyRepl<T> {
        self.snapshot.into_repl()
    }

    /// Resumes snippet execution with the OS call result.
    pub fn resume(
        self,
        result: impl Into<ExtFunctionResult>,
        print: PrintWriter<'_>,
    ) -> Result<ReplProgress<T>, Box<ReplStartError<T>>> {
        self.snapshot.run(result.into(), print)
    }
}

// ---------------------------------------------------------------------------
// ReplNameLookup
// ---------------------------------------------------------------------------

/// REPL execution paused for an unresolved name lookup.
///
/// The host should check if the name corresponds to a known external function or
/// value. Call `resume(result, print)` with the appropriate `NameLookupResult`.
/// The namespace slot and scope are managed internally.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "T: serde::Serialize", deserialize = "T: DeserializeOwned"))]
pub struct ReplNameLookup<T: ResourceTracker> {
    /// The name being looked up.
    pub name: String,
    /// The namespace slot where the resolved value should be cached.
    namespace_slot: u16,
    /// Whether this is a global slot or a local/function slot.
    is_global: bool,
    /// Internal REPL execution snapshot.
    snapshot: ReplSnapshot<T>,
}

impl<T: ResourceTracker> ReplNameLookup<T> {
    /// Extracts the REPL session, discarding the in-flight execution state.
    ///
    /// Restores globals from the VM snapshot so the REPL remains usable.
    #[must_use]
    pub fn into_repl(self) -> MontyRepl<T> {
        self.snapshot.into_repl()
    }

    /// Resumes execution after name resolution.
    ///
    /// Caches the resolved value in the namespace slot before restoring the VM,
    /// then either pushes the value onto the stack or raises `NameError`.
    pub fn resume(
        self,
        result: NameLookupResult,
        print: PrintWriter<'_>,
    ) -> Result<ReplProgress<T>, Box<ReplStartError<T>>> {
        let Self {
            name,
            namespace_slot,
            is_global,
            snapshot,
        } = self;

        let ReplSnapshot {
            mut repl,
            executor,
            vm_state,
            observer,
            extension_bytes,
            ..
        } = snapshot;

        match HeapReader::with(
            &mut repl.heap,
            &mut (&executor, print, &observer),
            |reader, (executor, print, observer)| {
                // Restore the VM first, then convert inside its lifetime
                let mut vm = VM::restore_with_observer(
                    vm_state,
                    &executor.module_code,
                    reader,
                    &executor.interns,
                    print.reborrow(),
                    (*observer).clone(),
                );

                // Resolve the name lookup result with the VM alive
                let vm_result = match result {
                    NameLookupResult::Value(obj) => {
                        let value = match obj.to_value(&mut vm) {
                            Ok(v) => v,
                            Err(e) => {
                                repl.globals = vm.take_globals();
                                return Err(MontyException::runtime_error(format!(
                                    "invalid name lookup result: {e}"
                                )));
                            }
                        };

                        // Cache the resolved value in the appropriate slot
                        let slot = namespace_slot as usize;
                        if is_global {
                            let cloned = value.clone_with_heap(&vm);
                            let old = mem::replace(&mut vm.globals[slot], cloned);
                            old.drop_with_heap(&mut vm);
                        } else {
                            let stack_base = vm.current_stack_base();
                            let cloned = value.clone_with_heap(&vm);
                            let old = mem::replace(&mut vm.stack[stack_base + slot], cloned);
                            old.drop_with_heap(&mut vm);
                        }

                        vm.push_created(value);
                        vm.run()
                    }
                    NameLookupResult::Undefined => {
                        let err: RunError = ExcType::name_error(&name).into();
                        vm.resume_with_exception(err)
                    }
                };

                // Convert while VM alive, then snapshot or reclaim globals
                let converted = convert_frame_exit(vm_result, &mut vm);
                let vm_state = if converted.needs_snapshot() {
                    Some(vm.snapshot())
                } else {
                    repl.globals = vm.take_globals();
                    None
                };
                Ok((converted, vm_state))
            },
        ) {
            Ok((converted, vm_state)) => {
                build_repl_progress(converted, vm_state, executor, repl, observer, extension_bytes.as_ref())
            }
            Err(error) => Err(Box::new(ReplStartError { repl, error })),
        }
    }

    /// Attaches embedder-owned snapshot extension bytes to this suspended state.
    #[must_use]
    pub fn with_snapshot_extension(mut self, snapshot_extension: impl Into<SnapshotExtension>) -> Self {
        self.snapshot = self.snapshot.with_snapshot_extension(snapshot_extension);
        self
    }

    /// Returns the embedder-owned snapshot extension bytes, if present.
    #[must_use]
    pub fn snapshot_extension(&self) -> Option<&SnapshotExtension> {
        self.snapshot.snapshot_extension()
    }
}

// ---------------------------------------------------------------------------
// ReplResolveFutures
// ---------------------------------------------------------------------------

/// REPL execution state blocked on unresolved external futures.
///
/// This is the REPL-aware counterpart to `ResolveFutures`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "T: serde::Serialize", deserialize = "T: DeserializeOwned"))]
pub struct ReplResolveFutures<T: ResourceTracker> {
    /// Persistent REPL session state while this snippet is suspended.
    repl: MontyRepl<T>,
    /// Compiled snippet and intern/function tables for this execution.
    executor: Executor,
    /// VM stack/frame state at suspension.
    vm_state: VMSnapshot,
    /// Runtime observer to reattach across restore/resume boundaries.
    #[serde(skip, default = "RuntimeObserverHandle::disabled")]
    observer: RuntimeObserverHandle,
    /// Pending call IDs expected by this snapshot.
    pending_call_ids: Vec<u32>,
    /// Optional embedder-owned bytes persisted with this snapshot.
    #[serde(default, rename = "snapshot_extension")]
    extension_bytes: Option<SnapshotExtension>,
}

impl<T: ResourceTracker> ReplResolveFutures<T> {
    /// Extracts the REPL session, restoring globals from the suspended VM state.
    ///
    /// As with the other REPL snapshot types, globals live inside the VM
    /// snapshot while execution is suspended. Recovering the REPL for a
    /// cancelled or abandoned async snippet must put those globals back so
    /// previously defined REPL bindings remain available.
    #[must_use]
    pub fn into_repl(self) -> MontyRepl<T> {
        let Self { mut repl, vm_state, .. } = self;
        repl.globals = vm_state.globals;
        repl
    }

    /// Returns unresolved call IDs for this suspended state.
    #[must_use]
    pub fn pending_call_ids(&self) -> &[u32] {
        &self.pending_call_ids
    }

    /// Resumes snippet execution with zero or more resolved futures.
    ///
    /// Supports incremental resolution: callers can provide only a subset of
    /// pending call IDs and continue resolving over multiple resumes.
    ///
    /// All errors — including API misuse (unknown `call_id`) and Python-level
    /// runtime failures — are returned as `Err(Box<ReplStartError>)` so the REPL
    /// session is always preserved.
    pub fn resume(
        self,
        results: Vec<(u32, ExtFunctionResult)>,
        print: PrintWriter<'_>,
    ) -> Result<ReplProgress<T>, Box<ReplStartError<T>>> {
        let Self {
            mut repl,
            executor,
            vm_state,
            observer,
            pending_call_ids,
            extension_bytes,
        } = self;

        for (call_id, result) in &results {
            emit_external_call_returned(*call_id, result, &observer);
        }

        let invalid_call_id = results
            .iter()
            .find(|(call_id, _)| !pending_call_ids.contains(call_id))
            .map(|(call_id, _)| *call_id);

        match HeapReader::with(
            &mut repl.heap,
            &mut (&executor, print, &observer),
            |reader, (executor, print, observer)| {
                let mut vm = VM::restore_with_observer(
                    vm_state,
                    &executor.module_code,
                    reader,
                    &executor.interns,
                    print.reborrow(),
                    (*observer).clone(),
                );

                if let Some(call_id) = invalid_call_id {
                    repl.globals = vm.take_globals();
                    return Err(MontyException::runtime_error(format!(
                        "unknown call_id {call_id}, expected one of: {pending_call_ids:?}"
                    )));
                }

                let vm_result = vm.resume_with_resolved_futures(results);

                // Convert while VM alive, then snapshot or reclaim globals
                let converted = convert_frame_exit(vm_result, &mut vm);
                let vm_state = if converted.needs_snapshot() {
                    Some(vm.snapshot())
                } else {
                    repl.globals = vm.take_globals();
                    None
                };
                Ok((converted, vm_state))
            },
        ) {
            Ok((converted, vm_state)) => {
                build_repl_progress(converted, vm_state, executor, repl, observer, extension_bytes.as_ref())
            }
            Err(error) => Err(Box::new(ReplStartError { repl, error })),
        }
    }

    /// Creates a new `ReplResolveFutures` from resumed VM state while preserving
    /// any embedder-owned snapshot extension bytes from the prior suspension.
    fn from_vm_snapshot(
        repl: MontyRepl<T>,
        executor: Executor,
        vm_state: VMSnapshot,
        pending_call_ids: Vec<u32>,
        observer: RuntimeObserverHandle,
        extension_bytes: Option<&SnapshotExtension>,
    ) -> Self {
        Self {
            repl,
            executor,
            vm_state,
            observer,
            pending_call_ids,
            extension_bytes: clone_snapshot_extension(extension_bytes),
        }
    }

    /// Attaches embedder-owned snapshot extension bytes to this suspended state.
    #[must_use]
    pub fn with_snapshot_extension(mut self, snapshot_extension: impl Into<SnapshotExtension>) -> Self {
        self.extension_bytes = Some(snapshot_extension.into());
        self
    }

    /// Returns the embedder-owned snapshot extension bytes, if present.
    #[must_use]
    pub fn snapshot_extension(&self) -> Option<&SnapshotExtension> {
        self.extension_bytes.as_ref()
    }
}

// ---------------------------------------------------------------------------
// ReplContinuationMode — public utility for interactive input collection
// ---------------------------------------------------------------------------

/// Parse-derived continuation state for interactive REPL input collection.
///
/// `monty-cli` uses this to decide whether to execute the buffered snippet
/// immediately, keep collecting continuation lines, or require a terminating
/// blank line for block statements (`if:`, `def:`, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplContinuationMode {
    /// The current snippet is syntactically complete and can run now.
    Complete,
    /// The snippet is incomplete and needs more continuation lines.
    IncompleteImplicit,
    /// The snippet opened an indented block and should wait for a trailing blank
    /// line before execution, matching CPython interactive behavior.
    IncompleteBlock,
}

/// Detects whether REPL source is complete or needs more input.
///
/// This mirrors CPython's broad interactive behavior:
/// - Incomplete bracketed / parenthesized / triple-quoted constructs continue.
/// - Clause headers (`if:`, `def:`, etc.) require an indented body and then a
///   terminating blank line before execution.
/// - All other parse outcomes are treated as complete (either valid code or a
///   syntax error that should be shown immediately).
#[must_use]
pub fn detect_repl_continuation_mode(source: &str) -> ReplContinuationMode {
    let Err(error) = parse_module(source) else {
        return ReplContinuationMode::Complete;
    };

    match error.error {
        ParseErrorType::OtherError(msg) => {
            if msg.starts_with("Expected an indented block after ") {
                ReplContinuationMode::IncompleteBlock
            } else {
                ReplContinuationMode::Complete
            }
        }
        ParseErrorType::Lexical(LexicalErrorType::Eof)
        | ParseErrorType::ExpectedToken {
            found: TokenKind::EndOfFile,
            ..
        }
        | ParseErrorType::FStringError(InterpolatedStringErrorType::UnterminatedTripleQuotedString)
        | ParseErrorType::TStringError(InterpolatedStringErrorType::UnterminatedTripleQuotedString) => {
            ReplContinuationMode::IncompleteImplicit
        }
        _ => ReplContinuationMode::Complete,
    }
}

// ---------------------------------------------------------------------------
// ReplSnapshot — internal execution state for suspend/resume
// ---------------------------------------------------------------------------

/// REPL execution state that can be resumed after an external call.
///
/// This is the REPL-aware counterpart to `Snapshot`. It is `pub(crate)` —
/// callers interact with the per-variant structs (`ReplFunctionCall`, etc.).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "T: serde::Serialize", deserialize = "T: DeserializeOwned"))]
pub(crate) struct ReplSnapshot<T: ResourceTracker> {
    /// Persistent REPL session state while this snippet is suspended.
    repl: MontyRepl<T>,
    /// Compiled snippet and intern/function tables for this execution.
    executor: Executor,
    /// VM stack/frame state at suspension.
    vm_state: VMSnapshot,
    /// Optional embedder-owned bytes persisted with this snapshot.
    #[serde(default, rename = "snapshot_extension")]
    extension_bytes: Option<SnapshotExtension>,
    /// Runtime observer to reattach across restore/resume boundaries.
    #[serde(skip, default = "RuntimeObserverHandle::disabled")]
    observer: RuntimeObserverHandle,
    /// Host-visible call identifier for return-event emission on resume.
    pending_call_id: u32,
}

impl<T: ResourceTracker> ReplSnapshot<T> {
    /// Extracts the REPL session, restoring globals from the VM snapshot.
    ///
    /// When a snapshot is taken, globals live inside the `VMSnapshot`.
    /// This method creates an empty snapshot from just the globals so the REPL
    /// can be used for further snippets.
    fn into_repl(self) -> MontyRepl<T> {
        let Self { mut repl, vm_state, .. } = self;
        repl.globals = vm_state.globals;
        repl
    }

    /// Continues snippet execution with an external result.
    fn run(
        self,
        result: impl Into<ExtFunctionResult>,
        print: PrintWriter<'_>,
    ) -> Result<ReplProgress<T>, Box<ReplStartError<T>>> {
        let Self {
            mut repl,
            executor,
            vm_state,
            observer,
            pending_call_id,
            extension_bytes,
        } = self;

        let ext_result = result.into();
        emit_external_call_returned(pending_call_id, &ext_result, &observer);

        let (converted, vm_state) = HeapReader::with(
            &mut repl.heap,
            &mut (&executor, print, &observer),
            |reader, (executor, print, observer)| {
                let mut vm = VM::restore_with_observer(
                    vm_state,
                    &executor.module_code,
                    reader,
                    &executor.interns,
                    print.reborrow(),
                    (*observer).clone(),
                );

                let vm_result = match ext_result {
                    ExtFunctionResult::Return(obj) => vm.resume(obj),
                    ExtFunctionResult::Error(exc) => vm.resume_with_exception(exc.into()),
                    ExtFunctionResult::Future(raw_call_id) => {
                        let call_id = CallId::new(raw_call_id);
                        vm.add_pending_call(call_id);
                        vm.push_created(Value::ExternalFuture(call_id));
                        vm.run()
                    }
                    ExtFunctionResult::NotFound(function_name) => {
                        vm.resume_with_exception(ExtFunctionResult::not_found_exc(&function_name))
                    }
                };

                // Convert while VM alive, then snapshot or reclaim globals
                let converted = convert_frame_exit(vm_result, &mut vm);
                let vm_state = if converted.needs_snapshot() {
                    Some(vm.snapshot())
                } else {
                    repl.globals = vm.take_globals();
                    None
                };
                (converted, vm_state)
            },
        );
        build_repl_progress(converted, vm_state, executor, repl, observer, extension_bytes.as_ref())
    }

    /// Creates a resumable REPL snapshot from VM-owned execution state.
    fn from_vm_snapshot(
        repl: MontyRepl<T>,
        executor: Executor,
        vm_state: VMSnapshot,
        observer: RuntimeObserverHandle,
        pending_call_id: u32,
        extension_bytes: Option<&SnapshotExtension>,
    ) -> Self {
        Self {
            repl,
            executor,
            vm_state,
            extension_bytes: clone_snapshot_extension(extension_bytes),
            observer,
            pending_call_id,
        }
    }

    /// Attaches embedder-owned snapshot extension bytes to this suspended state.
    #[must_use]
    fn with_snapshot_extension(mut self, snapshot_extension: impl Into<SnapshotExtension>) -> Self {
        self.extension_bytes = Some(snapshot_extension.into());
        self
    }

    /// Returns the embedder-owned snapshot extension bytes, if present.
    #[must_use]
    fn snapshot_extension(&self) -> Option<&SnapshotExtension> {
        self.extension_bytes.as_ref()
    }
}

// ---------------------------------------------------------------------------
// Private helper functions
// ---------------------------------------------------------------------------

/// Injects input values into the VM's global namespace slots.
///
/// Converts each `MontyObject` to a `Value` while the VM is alive, then stores
/// it in the global slot that the compiler assigned for the corresponding input name.
fn inject_inputs_into_vm(
    executor: &Executor,
    input_values: Vec<MontyObject>,
    vm: &mut VM<'_, impl ResourceTracker>,
) -> Result<(), MontyException> {
    for (name, obj) in executor.input_names.iter().zip(input_values) {
        let slot = executor
            .name_map
            .get(name)
            .expect("input name should have a namespace slot")
            .index();
        let value = obj
            .to_value(vm)
            .map_err(|e| MontyException::runtime_error(format!("invalid input type: {e}")))?;
        let old = mem::replace(&mut vm.globals[slot], value);
        old.drop_with_heap(vm);
    }
    Ok(())
}

/// Assembles a `ReplProgress` from already-converted data.
///
/// This is the REPL equivalent of `build_run_progress`. On completion/error,
/// compiler metadata is committed to the REPL so subsequent snippets see
/// updated intern tables and name maps.
fn build_repl_progress<T: ResourceTracker>(
    converted: ConvertedExit,
    vm_state: Option<VMSnapshot>,
    executor: Executor,
    mut repl: MontyRepl<T>,
    observer: RuntimeObserverHandle,
    extension_bytes: Option<&SnapshotExtension>,
) -> Result<ReplProgress<T>, Box<ReplStartError<T>>> {
    macro_rules! new_repl_snapshot {
        ($call_id:expr) => {
            ReplSnapshot::from_vm_snapshot(
                repl,
                executor,
                vm_state.expect("snapshot should exist"),
                observer.clone(),
                $call_id,
                extension_bytes,
            )
        };
    }

    match converted {
        ConvertedExit::Complete(obj) => {
            let Executor { name_map, interns, .. } = executor;
            repl.global_name_map = name_map;
            repl.interns = interns;
            Ok(ReplProgress::Complete { repl, value: obj })
        }
        ConvertedExit::FunctionCall {
            function_name,
            args,
            kwargs,
            arg_runtime_ids,
            kwarg_runtime_ids,
            call_id,
            method_call,
        } => {
            let kind = if method_call {
                ExternalCallKind::Method
            } else {
                ExternalCallKind::Function
            };
            emit_external_call_requested(&observer, call_id, kind, &arg_runtime_ids, &kwarg_runtime_ids);
            Ok(ReplProgress::FunctionCall(ReplFunctionCall {
                function_name,
                args,
                kwargs,
                arg_runtime_ids,
                kwarg_runtime_ids,
                call_id,
                method_call,
                snapshot: new_repl_snapshot!(call_id),
            }))
        }
        ConvertedExit::OsCall {
            function,
            args,
            kwargs,
            arg_runtime_ids,
            kwarg_runtime_ids,
            call_id,
        } => {
            emit_external_call_requested(
                &observer,
                call_id,
                ExternalCallKind::Os,
                &arg_runtime_ids,
                &kwarg_runtime_ids,
            );
            Ok(ReplProgress::OsCall(ReplOsCall {
                function,
                args,
                kwargs,
                arg_runtime_ids,
                kwarg_runtime_ids,
                call_id,
                snapshot: new_repl_snapshot!(call_id),
            }))
        }
        ConvertedExit::ResolveFutures(pending_call_ids) => {
            Ok(ReplProgress::ResolveFutures(ReplResolveFutures::from_vm_snapshot(
                repl,
                executor,
                vm_state.expect("snapshot should exist for ResolveFutures"),
                pending_call_ids,
                observer,
                extension_bytes,
            )))
        }
        ConvertedExit::NameLookup {
            name,
            namespace_slot,
            is_global,
        } => Ok(ReplProgress::NameLookup(ReplNameLookup {
            name,
            namespace_slot,
            is_global,
            snapshot: new_repl_snapshot!(0),
        })),
        ConvertedExit::Error(err) => {
            // Resolve traceback frames against every snippet the REPL has
            // seen, not just the currently-executing one. `executor.interns`
            // is still required because it holds the StringIds referenced by
            // the in-flight frames; `repl.sources` holds every snippet's
            // source text and is what owns any older snippets' sources.
            let error =
                err.into_python_exception(&executor.interns, |fname| repl.sources.get(fname).map(String::as_str));
            // Commit compiler metadata even on runtime errors, matching feed() behavior.
            // Snippets can create new variables or functions before raising, and those
            // values may reference FunctionId/StringId values from the new tables.
            let Executor { name_map, interns, .. } = executor;
            repl.global_name_map = name_map;
            repl.interns = interns;
            Err(Box::new(ReplStartError { repl, error }))
        }
    }
}

/// Converts `Vec<MontyObject>` to internal `ArgValues` for function calls.
fn convert_args(args: Vec<MontyObject>, vm: &mut VM<'_, impl ResourceTracker>) -> Result<ArgValues, MontyException> {
    match args.len() {
        0 => Ok(ArgValues::Empty),
        1 => {
            let value = args
                .into_iter()
                .next()
                .expect("checked len")
                .to_value(vm)
                .map_err(|e| MontyException::runtime_error(format!("invalid argument type: {e}")))?;
            Ok(ArgValues::One(value))
        }
        2 => {
            let mut iter = args.into_iter();
            let a = iter
                .next()
                .expect("checked len")
                .to_value(vm)
                .map_err(|e| MontyException::runtime_error(format!("invalid argument type: {e}")))?;
            match iter.next().expect("checked len").to_value(vm) {
                Ok(b) => Ok(ArgValues::Two(a, b)),
                Err(e) => {
                    a.drop_with_heap(&mut *vm);
                    Err(MontyException::runtime_error(format!("invalid argument type: {e}")))
                }
            }
        }
        _ => {
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                match arg.to_value(vm) {
                    Ok(value) => values.push(value),
                    Err(e) => {
                        values.drain(..).drop_with_heap(&mut *vm);
                        return Err(MontyException::runtime_error(format!("invalid argument type: {e}")));
                    }
                }
            }
            Ok(ArgValues::ArgsKargs {
                args: values,
                kwargs: KwargsValues::Empty,
            })
        }
    }
}

/// Returns `true` if the value is a callable type.
///
/// For heap-allocated values (`Ref`), checks the actual `HeapData` variant
/// rather than accepting all refs — only closures, functions with defaults,
/// and heap-allocated external functions are callable.
fn is_callable(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    match value {
        Value::DefFunction(_) | Value::Builtin(_) | Value::ExtFunction(_) | Value::ModuleFunction(_) => true,
        Value::Ref(id) => matches!(
            heap.get(*id),
            HeapData::Closure(_) | HeapData::FunctionDefaults(_) | HeapData::ExtFunction(_)
        ),
        _ => false,
    }
}

/// Emits an external-call request event when observation is enabled.
fn emit_external_call_requested(
    observer: &RuntimeObserverHandle,
    call_id: u32,
    kind: ExternalCallKind,
    arg_runtime_ids: &[RuntimeValueId],
    kwarg_runtime_ids: &[(RuntimeValueId, RuntimeValueId)],
) {
    if !observer.is_enabled() {
        return;
    }
    observer.emit(RuntimeObserverEvent::ExternalCallRequested(
        ExternalCallRequestedEvent {
            call_id,
            kind,
            arg_runtime_ids,
            kwarg_runtime_ids,
        },
    ));
}

/// Emits an external-call return event when observation is enabled.
fn emit_external_call_returned(call_id: u32, result: &ExtFunctionResult, observer: &RuntimeObserverHandle) {
    if !observer.is_enabled() {
        return;
    }
    let kind = match result {
        ExtFunctionResult::Return(_) => ExternalCallReturnKind::Return,
        ExtFunctionResult::Error(_) | ExtFunctionResult::NotFound(_) => ExternalCallReturnKind::Error,
        ExtFunctionResult::Future(_) => ExternalCallReturnKind::Future,
    };
    observer.emit(RuntimeObserverEvent::ExternalCallReturned(ExternalCallReturnedEvent {
        call_id,
        kind,
    }));
}
