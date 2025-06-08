// src/task.rs

use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};

// --- TaskId ---
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(u64);

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

impl TaskId {
    pub fn new() -> Self {
        TaskId(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the inner u64 value of the TaskId.
    pub fn value(&self) -> u64 { // ADDED THIS
        self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Task({})", self.0)
    }
}

// --- Priority ---
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    Normal,
    High,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

// --- TaskError ---
#[derive(Debug)]
pub enum TaskError {
    Panicked(String),
    ExecutionFailed(Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl fmt::Display for TaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskError::Panicked(msg) => write!(f, "Task panicked: {}", msg),
            TaskError::ExecutionFailed(err) => write!(f, "Task execution failed: {}", err),
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

// --- Task ---
pub struct Task<Output> {
    pub id: TaskId,
    pub priority: Priority,
    pub estimated_cost: u32,
    task_fn: Box<dyn FnOnce() -> Result<Output, TaskError> + Send + 'static>,
}

impl<Output> Task<Output> {
    pub fn new(
        priority: Priority,
        estimated_cost: u32,
        task_fn: impl FnOnce() -> Result<Output, TaskError> + Send + 'static,
    ) -> Self {
        Task {
            id: TaskId::new(),
            priority,
            estimated_cost: estimated_cost.clamp(1, 100),
            task_fn: Box::new(task_fn),
        }
    }

    pub fn execute(self) -> Result<Output, TaskError> {
        match std::panic::catch_unwind(AssertUnwindSafe(self.task_fn)) {
            Ok(result) => result,
            Err(panic_payload) => Err(TaskError::Panicked(panic_payload_to_string(panic_payload))),
        }
    }
}

impl<Output> fmt::Debug for Task<Output> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Task")
            .field("id", &self.id)
            .field("priority", &self.priority)
            .field("estimated_cost", &self.estimated_cost)
            .field("task_fn", &"<function>")
            .finish()
    }
}
