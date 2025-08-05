//! Defines the `Task` abstraction, its associated `TaskId`, `Priority`, and `TaskError` types,
//! and the `TaskHandle` for retrieving task results.
//!
//! This module provides the fundamental building blocks for defining and managing
//! units of work within the Vibe System.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

// --- TaskId ---
/// A unique, monotonic identifier for a `Task` instance.
///
/// Each `TaskId` is globally unique within the system, generated using an
/// atomic counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(u64);

/// Atomic counter for generating the next unique `TaskId`.
static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

impl TaskId {
    /// Creates a new, unique `TaskId`.
    ///
    /// Each call to this function increments an internal atomic counter
    /// to ensure uniqueness.
    pub fn new() -> Self {
        TaskId(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the inner `u64` value of the `TaskId`.
    pub fn value(&self) -> u64 {
        self.0
    }
}

impl Default for TaskId {
    /// Provides a default `TaskId` by calling `TaskId::new()`.
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TaskId {
    /// Implements the `Display` trait for `TaskId`, formatting it as "Task(ID)".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Task({})", self.0)
    }
}

// --- Priority ---
/// Defines the priority level of a task.
///
/// Tasks with higher priority are generally processed before tasks with lower priority.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Low priority task.
    Low,
    /// Normal priority task (default).
    #[default]
    Normal,
    /// High priority task.
    High,
}

// --- TaskError ---
/// Represents various errors that can occur during a task's execution.
#[derive(Debug)]
pub enum TaskError {
    /// The task's underlying work function panicked during execution.
    Panicked(String),
    /// The task's logic failed gracefully and returned an error.
    ExecutionFailed(Box<dyn std::error::Error + Send + Sync + 'static>),
    /// The operation associated with the task timed out.
    TimedOut,
}

impl fmt::Display for TaskError {
    /// Implements the `Display` trait for `TaskError`, providing human-readable error messages.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskError::Panicked(msg) => write!(f, "Task panicked: {msg}"),
            TaskError::ExecutionFailed(err) => write!(f, "Task execution failed: {err}"),
            TaskError::TimedOut => write!(f, "Operation timed out"),
        }
    }
}

impl std::error::Error for TaskError {
    /// Returns the source of the error, if available.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TaskError::ExecutionFailed(err) => Some(err.as_ref()),
            _ => None,
        }
    }
}

/// Converts a panic payload into a `String` representation.
///
/// This helper function attempts to downcast the panic payload to common types
/// (`&str`, `String`) to provide a more informative error message.
fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "Unknown panic payload type".to_string()
    }
}

// --- TaskHandle ---
/// A handle that allows the client to retrieve the result of a submitted task.
///
/// This handle is returned immediately upon task submission. The client can use it
/// to block and wait for the result, or poll for the result without blocking.
///
/// If the `TaskHandle` is dropped, the task's work will still be computed by the worker,
/// but the attempt to send the result back will fail gracefully.
#[derive(Debug)]
pub struct TaskHandle<Output> {
    /// The unique ID of the task associated with this handle.
    pub task_id: TaskId,
    /// The receiver end of the one-time channel for the task's result.
    result_receiver: mpsc::Receiver<Result<Output, TaskError>>,
}

impl<Output> TaskHandle<Output> {
    /// Creates a new `TaskHandle`.
    ///
    /// This constructor is intended for internal use by the library when a task
    /// is submitted and a channel is set up for its result.
    pub(crate) fn new(
        task_id: TaskId,
        result_receiver: mpsc::Receiver<Result<Output, TaskError>>,
    ) -> Self {
        Self {
            task_id,
            result_receiver,
        }
    }

    /// Returns the unique ID of the task associated with this handle.
    pub fn get_task_id(&self) -> TaskId {
        self.task_id
    }

    /// Blocks the current thread until the task's result is available.
    ///
    /// # Returns
    ///
    /// - `Ok(Result<Output, TaskError>)`: The result of the task execution.
    /// - `Err(mpsc::RecvError)`: An error indicating the sending side of the channel
    ///   has hung up, which might happen if the worker thread itself panics
    ///   in an unrecoverable way before sending a result.
    pub fn recv_result(&self) -> Result<Result<Output, TaskError>, mpsc::RecvError> {
        self.result_receiver.recv()
    }

    /// Attempts to receive the task's result without blocking.
    ///
    /// # Returns
    ///
    /// - `Ok(Result<Output, TaskError>)`: The result if it was immediately available.
    /// - `Err(mpsc::TryRecvError::Empty)`: If the result is not yet available.
    /// - `Err(mpsc::TryRecvError::Disconnected)`: If the sending side has hung up.
    pub fn try_recv_result(&self) -> Result<Result<Output, TaskError>, mpsc::TryRecvError> {
        self.result_receiver.try_recv()
    }
}

// --- TaskExecutionOutcome ---
/// An internal enum used by an `VibeNode` worker to know the outcome of a task's
/// execution, separate from the actual result data.
///
/// This is used for internal logging, metrics, and potential future features
/// like circuit breakers or adaptive load balancing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskExecutionOutcome {
    /// The task's logic completed successfully and its result was sent to the client.
    Success,
    /// The task's logic returned a `TaskError` (e.g., `ExecutionFailed`, `TimedOut`),
    /// and this error was sent to the client.
    LogicError,
    /// The task's result (whether success or error) could not be sent because the
    /// client dropped the `TaskHandle`'s receiver before the result was ready.
    ResultSendFailed,
    /// The task's underlying work function panicked during execution, and a
    /// `TaskError::Panicked` was sent to the client.
    Panicked,
}

// --- Task ---
/// Represents a unit of work to be processed by an `VibeNode`.
///
/// This struct is non-generic and encapsulates the task's metadata (ID, priority, cost)
/// and a type-erased runnable closure. The closure handles the execution of the
/// user's work function, panic catching, and sending the result back to the client.
pub struct Task {
    /// The unique identifier for this task.
    pub id: TaskId,
    /// The priority level of this task.
    pub priority: Priority,
    /// An estimated computational cost of the task, used for scheduling or load balancing.
    /// Clamped between 1 and 100.
    pub estimated_cost: u32,
    /// The type-erased closure that the worker will execute.
    /// It returns a `TaskExecutionOutcome` for internal node statistics.
    runnable: Box<dyn FnOnce() -> TaskExecutionOutcome + Send + 'static>,
}

impl Task {
    /// Creates a new CPU-bound task.
    ///
    /// This is the primary constructor for tasks that will be executed by `VibeNode` workers.
    /// It wraps the user's provided work function (`work_fn`) in an internal closure that
    /// handles result/error passing, panic catching, and returning an internal outcome.
    ///
    /// # Arguments
    ///
    /// * `priority` - The priority of the task.
    /// * `estimated_cost` - An estimated computational cost of the task.
    /// * `work_fn` - The closure containing the CPU-bound work to be executed.
    /// * `result_tx` - The sender channel for returning the task's `Result<Output, TaskError>` to the client.
    ///
    /// # Type Parameters
    ///
    /// * `F` - The type of the work function, which must be `FnOnce`, `Send`, and `'static`.
    /// * `Output` - The return type of the work function, which must be `Send` and `'static`.
    pub fn new_for_cpu<F, Output>(
        priority: Priority,
        estimated_cost: u32,
        work_fn: F,
        result_tx: mpsc::Sender<Result<Output, TaskError>>,
    ) -> Self
    where
        F: FnOnce() -> Result<Output, TaskError> + Send + 'static,
        Output: Send + 'static,
    {
        let task_id = TaskId::new();

        let runnable = Box::new(move || {
            // This is the closure the worker thread will execute.
            // 1. Execute the user's work function, catching any panics.
            let task_result = match catch_unwind(AssertUnwindSafe(work_fn)) {
                Ok(result) => result, // The user's function returned Result<Output, TaskError>
                Err(panic_payload) => {
                    // The user's function panicked.
                    Err(TaskError::Panicked(panic_payload_to_string(panic_payload)))
                }
            };

            // Determine the internal `TaskExecutionOutcome` before attempting to send the result.
            let outcome_before_send = match &task_result {
                Ok(_) => TaskExecutionOutcome::Success,
                Err(TaskError::Panicked(_)) => TaskExecutionOutcome::Panicked,
                Err(TaskError::ExecutionFailed(_)) => TaskExecutionOutcome::LogicError,
                Err(TaskError::TimedOut) => TaskExecutionOutcome::LogicError, // TimedOut is a logical error
            };

            // 2. Send the result back to the client via the channel.
            match result_tx.send(task_result) {
                Ok(_) => outcome_before_send, // Return the original outcome if send was successful
                Err(_) => {
                    // The client dropped the TaskHandle receiver. This is not an error
                    // for the worker, just a fact that the result won't be consumed.
                    // The work was still done.
                    TaskExecutionOutcome::ResultSendFailed
                }
            }
        });

        Task {
            id: task_id,
            priority,
            estimated_cost: estimated_cost.clamp(1, 100), // Ensure cost is within a reasonable range
            runnable,
        }
    }

    /// Executes the task's internal runnable closure.
    ///
    /// This method is called by the `VibeNode` worker thread.
    ///
    /// # Returns
    ///
    /// A `TaskExecutionOutcome` indicating how the task's execution concluded
    /// from the worker's perspective.
    pub fn run(self) -> TaskExecutionOutcome {
        (self.runnable)()
    }
}

impl fmt::Debug for Task {
    /// Implements the `Debug` trait for `Task`, providing a concise representation
    /// without exposing the internal `runnable` closure details.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Task")
            .field("id", &self.id)
            .field("priority", &self.priority)
            .field("estimated_cost", &self.estimated_cost)
            .field("runnable", &"<Box<dyn FnOnce() -> TaskExecutionOutcome>>")
            .finish()
    }
}
