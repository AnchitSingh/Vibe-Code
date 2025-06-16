// src/task.rs

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc; // Added for result channels

// --- TaskId ---
/// A unique identifier for a Task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(u64);

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

impl TaskId {
    pub fn new() -> Self {
        TaskId(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the inner u64 value of the TaskId.
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

// --- Priority ---
/// Defines the priority of a task.
#[derive(Debug,Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
}

// impl Default for Priority {
//     fn default() -> Self {
//         Priority::Normal
//     }
// }

// --- TaskError ---
/// Represents errors that can occur during a task's execution.
#[derive(Debug)]
pub enum TaskError {
    /// The task's code panicked during execution.
    Panicked(String),
    /// The task's logic failed gracefully and returned an error.
    ExecutionFailed(Box<dyn std::error::Error + Send + Sync + 'static>),
    TimedOut, // ADDED THIS VARIANT
}

impl fmt::Display for TaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskError::Panicked(msg) => write!(f, "Task panicked: {}", msg),
            TaskError::ExecutionFailed(err) => write!(f, "Task execution failed: {}", err),
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

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "Unknown panic payload type".to_string()
    }
}


// --- NEW: TaskHandle ---
/// A handle that allows the client to retrieve the result of a submitted task.
///
/// This handle is returned immediately upon task submission. The client can use it
/// to block and wait for the result, or poll for the result without blocking.
///
/// If the `TaskHandle` is dropped, the result will still be computed by the worker,
/// but the attempt to send it will fail gracefully.
#[derive(Debug)]
pub struct TaskHandle<Output> {
    pub task_id: TaskId,
    // The receiver end of the one-time channel for the task's result.
    result_receiver: mpsc::Receiver<Result<Output, TaskError>>,
}

impl<Output> TaskHandle<Output> {
    /// Creates a new `TaskHandle`. This is intended for internal use by the library.
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
    /// - `Ok(Result<Output, TaskError>)`: The result of the task execution.
    /// - `Err(RecvError)`: An error indicating the sending side of the channel
    ///   has hung up, which might happen if the worker thread itself panics
    ///   in an unrecoverable way before sending a result.
    pub fn recv_result(&self) -> Result<Result<Output, TaskError>, mpsc::RecvError> {
        self.result_receiver.recv()
    }

    /// Attempts to receive the task's result without blocking.
    ///
    /// # Returns
    /// - `Ok(Result<Output, TaskError>)`: The result if it was immediately available.
    /// - `Err(TryRecvError::Empty)`: If the result is not yet available.
    /// - `Err(TryRecvError::Disconnected)`: If the sending side has hung up.
    pub fn try_recv_result(&self) -> Result<Result<Output, TaskError>, mpsc::TryRecvError> {
        self.result_receiver.try_recv()
    }
}


// --- NEW: TaskExecutionOutcome ---
/// An internal enum used by an OmegaNode worker to know the outcome of a task's
/// execution, separate from the actual result data. This is used for internal
/// logging, metrics, and potential future features like circuit breakers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskExecutionOutcome {
    /// The task's logic completed successfully and the result was sent.
    Success,
    /// The task's logic returned a failure (a `TaskError`) and the error was sent.
    LogicError,
    /// The task's result (success or error) could not be sent because the
    /// client dropped the `TaskHandle`'s receiver.
    ResultSendFailed,
    /// The task's logic panicked during execution, and a `TaskError::Panicked` was sent.
    Panicked,
}


// --- REFACTORED: Task ---
/// Represents a unit of work to be processed by an OmegaNode.
///
/// This struct is now non-generic. It uses a boxed closure (`runnable`) to
/// erase the `Output` type, allowing `OmegaNode` to handle any kind of task
/// without needing to be generic itself.
pub struct Task {
    pub id: TaskId,
    pub priority: Priority,
    pub estimated_cost: u32,
    // The type-erased closure that the worker will execute.
    runnable: Box<dyn FnOnce() -> TaskExecutionOutcome + Send + 'static>,
}

impl Task {
    /// Creates a new CPU-bound task.
    ///
    // *   This is the primary constructor for tasks that will be executed by workers.
    // *   It wraps the user's provided work function in a closure that handles
    // *   result/error passing, panic catching, and returning an internal outcome.
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

            // CORRECTED: The match now correctly handles all variants of TaskError.
            let outcome_before_send = match &task_result {
                Ok(_) => TaskExecutionOutcome::Success,
                Err(TaskError::Panicked(_)) => TaskExecutionOutcome::Panicked,
                Err(TaskError::ExecutionFailed(_)) => TaskExecutionOutcome::LogicError,
                // Add the new case for completeness, even if it's rare for CPU tasks.
                Err(TaskError::TimedOut) => TaskExecutionOutcome::LogicError,
            };

            // 2. Send the result back to the client via the channel.
            match result_tx.send(task_result) {
                Ok(_) => outcome_before_send, // Return the original outcome
                Err(_) => {
                    // The client dropped the TaskHandle receiver. This is not an error
                    // for the worker, just a fact. The work was still done.
                    TaskExecutionOutcome::ResultSendFailed
                }
            }
        });

        Task {
            id: task_id,
            priority,
            estimated_cost: estimated_cost.clamp(1, 100),
            runnable,
        }
    }

    /// Executes the task's internal closure. Called by the OmegaNode worker.
    /// Returns an outcome for the worker to use for internal stats.
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