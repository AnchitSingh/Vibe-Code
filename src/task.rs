//! Defines the core `Task` abstraction and related components.
//!
//! This module provides the fundamental building blocks for defining and managing
//! units of work within the system. It includes the `Task` struct itself, which
//! wraps a user's function, and the `TaskHandle`, which is returned to the user
//! to get the final result.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

/// A unique identifier for a `Task`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(u64);

/// An atomic counter to generate unique task IDs.
static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

impl TaskId {
    /// Creates a new, unique `TaskId`.
    pub fn new() -> Self {
        TaskId(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the inner `u64` value of the `TaskId`.
    pub fn value(&self) -> u64 {
        self.0
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Task({})", self.0)
    }
}

/// Defines the priority level of a task.
///
/// This can be used by the system to prioritize certain tasks over others,
/// although it is not heavily used by the simple `VibeSystem` API.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
}

/// Represents errors that can occur during a task's execution.
#[derive(Debug)]
pub enum TaskError {
    /// The task's function panicked.
    Panicked(String),
    /// The task's function returned an error.
    ExecutionFailed(Box<dyn std::error::Error + Send + Sync + 'static>),
    /// The task timed out.
    TimedOut,
}

impl fmt::Display for TaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskError::Panicked(msg) => write!(f, "Task panicked: {msg}"),
            TaskError::ExecutionFailed(err) => write!(f, "Task execution failed: {err}"),
            TaskError::TimedOut => write!(f, "Operation timed out"),
        }
    }
}

impl std::error::Error for TaskError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TaskError::ExecutionFailed(err) => Some(err.as_ref()),
            _ => None,
        }
    }
}

/// Converts a panic payload into a readable string.
fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "Unknown panic payload type".to_string()
    }
}

/// A handle to a task that has been submitted to the system.
///
/// This is the public-facing handle (wrapped by `Job`) that allows a user
/// to retrieve the result of their function once it's complete.
#[derive(Debug)]
pub struct TaskHandle<Output> {
    pub task_id: TaskId,
    result_receiver: mpsc::Receiver<Result<Output, TaskError>>,
}

impl<Output> TaskHandle<Output> {
    /// Creates a new `TaskHandle` linked to a result channel. (Internal use)
    pub(crate) fn new(
        task_id: TaskId,
        result_receiver: mpsc::Receiver<Result<Output, TaskError>>,
    ) -> Self {
        Self {
            task_id,
            result_receiver,
        }
    }

    /// Returns the unique ID of the associated task.
    pub fn get_task_id(&self) -> TaskId {
        self.task_id
    }

    /// Waits (blocks) until the task's result is available and returns it.
    pub fn recv_result(&self) -> Result<Result<Output, TaskError>, mpsc::RecvError> {
        self.result_receiver.recv()
    }

    /// Attempts to receive the result without blocking.
    pub fn try_recv_result(&self) -> Result<Result<Output, TaskError>, mpsc::TryRecvError> {
        self.result_receiver.try_recv()
    }
}

/// An internal enum representing the outcome of a task's execution.
///
/// This is used by `VibeNode` workers for metrics and logging, distinguishing
/// between a successful run, a logical failure, or a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskExecutionOutcome {
    /// The task completed successfully and the result was sent.
    Success,
    /// The task returned a `TaskError`.
    LogicError,
    /// The result could not be sent because the receiver was dropped.
    ResultSendFailed,
    /// The task's function panicked.
    Panicked,
}

/// An internal representation of a unit of work.
///
/// This struct wraps the user's closure in a type-erased `Box<dyn FnOnce...>`,
/// allowing different kinds of functions to be stored and executed by workers.
pub struct Task {
    pub id: TaskId,
    pub priority: Priority,
    pub estimated_cost: u32,
    runnable: Box<dyn FnOnce() -> TaskExecutionOutcome + Send + 'static>,
}

impl Task {
    /// Creates a new `Task` from a user-provided function.
    ///
    /// This wraps the function in a new closure that handles panic catching
    /// and sending the result back over a channel.
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
            // Execute the user's function, catching any panics.
            let task_result = match catch_unwind(AssertUnwindSafe(work_fn)) {
                Ok(result) => result,
                Err(panic_payload) => {
                    Err(TaskError::Panicked(panic_payload_to_string(panic_payload)))
                }
            };

            // Determine the internal outcome for metrics.
            let outcome_before_send = match &task_result {
                Ok(_) => TaskExecutionOutcome::Success,
                Err(TaskError::Panicked(_)) => TaskExecutionOutcome::Panicked,
                Err(TaskError::ExecutionFailed(_)) => TaskExecutionOutcome::LogicError,
                Err(TaskError::TimedOut) => TaskExecutionOutcome::LogicError,
            };

            // Send the result back to the user.
            match result_tx.send(task_result) {
                Ok(_) => outcome_before_send,
                Err(_) => TaskExecutionOutcome::ResultSendFailed,
            }
        });

        Task {
            id: task_id,
            priority,
            estimated_cost: estimated_cost.clamp(1, 100),
            runnable,
        }
    }

    /// Executes the wrapped user function.
    ///
    /// This is called by the `VibeNode` worker thread.
    pub fn run(self) -> TaskExecutionOutcome {
        (self.runnable)()
    }
}

impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Task")
            .field("id", &self.id)
            .field("priority", &self.priority)
            .field("estimated_cost", &self.estimated_cost)
            .field("runnable", &"<Box<dyn FnOnce() -> TaskExecutionOutcome>>")
            .finish()
    }
}
