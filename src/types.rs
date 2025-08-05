//! Defines common error types and statistics-tracking structures.

use std::collections::VecDeque;
use std::fmt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// The maximum number of recent task durations to store for statistical analysis.
const ROLLING_WINDOW_TASK_DURATIONS: usize = 100;
/// The maximum number of recent task outcomes (success/failure) to store.
const ROLLING_WINDOW_FAILURES: usize = 50;

/// Represents errors that can occur at the node or system level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeError {
    /// A node's task queue is full.
    QueueFull,
    /// A node's task queue has been closed.
    QueueClosed,
    /// A node is shutting down and cannot accept new tasks.
    NodeShuttingDown,
    /// A node failed to send a signal to the central system.
    SignalSendError,
    /// No nodes are available in the system to process a task.
    NoNodesAvailable,
    /// All nodes are at maximum capacity and cannot accept new tasks.
    SystemMaxedOut,
}

impl fmt::Display for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeError::QueueFull => write!(f, "Node task queue is full"),
            NodeError::QueueClosed => write!(f, "Node task queue is closed"),
            NodeError::NodeShuttingDown => write!(f, "Node is shutting down"),
            NodeError::SignalSendError => write!(f, "Node failed to send a system signal"),
            NodeError::NoNodesAvailable => write!(f, "No VibeNodes available for task submission"),
            NodeError::SystemMaxedOut => {
                write!(
                    f,
                    "System is maxed out; all sampled nodes are at full pressure"
                )
            }
        }
    }
}

impl std::error::Error for NodeError {}

/// Tracks performance statistics for a single `VibeNode`.
///
/// This struct uses atomic counters and rolling windows to provide insights into
/// the performance and workload of an individual node.
#[derive(Debug)]
pub struct LocalStats {
    /// Total number of tasks submitted to this node.
    tasks_submitted: AtomicUsize,
    /// Total number of tasks processed by this node.
    tasks_processed: AtomicUsize,
    /// Total number of tasks that completed successfully.
    tasks_succeeded: AtomicUsize,
    /// Total number of tasks that failed.
    tasks_failed: AtomicUsize,
    /// Cumulative processing time for all tasks on this node, in microseconds.
    total_processing_time_micros: AtomicU64,
    /// A rolling window of recent task durations (in microseconds).
    recent_task_durations_micros: Mutex<VecDeque<u64>>,
    /// A rolling window of recent task outcomes (0 for success, 1 for failure).
    recent_task_outcomes: Mutex<VecDeque<u8>>,
}

impl Default for LocalStats {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalStats {
    /// Creates a new `LocalStats` instance with all counters initialized to zero.
    pub fn new() -> Self {
        LocalStats {
            tasks_submitted: AtomicUsize::new(0),
            tasks_processed: AtomicUsize::new(0),
            tasks_succeeded: AtomicUsize::new(0),
            tasks_failed: AtomicUsize::new(0),
            total_processing_time_micros: AtomicU64::new(0),
            recent_task_durations_micros: Mutex::new(VecDeque::with_capacity(
                ROLLING_WINDOW_TASK_DURATIONS,
            )),
            recent_task_outcomes: Mutex::new(VecDeque::with_capacity(ROLLING_WINDOW_FAILURES)),
        }
    }

    /// Increments the count of submitted tasks.
    pub fn task_submitted(&self) {
        self.tasks_submitted.fetch_add(1, Ordering::Relaxed);
    }

    /// Records the outcome of a finished task.
    pub fn record_task_outcome(&self, duration: u64, success: bool) {
        self.total_processing_time_micros
            .fetch_add(duration, Ordering::Relaxed);

        if success {
            self.tasks_succeeded.fetch_add(1, Ordering::Relaxed);
        } else {
            self.tasks_failed.fetch_add(1, Ordering::Relaxed);
        }

        // Add duration to the rolling window.
        {
            let mut durations_guard = self
                .recent_task_durations_micros
                .lock()
                .expect("Mutex poisoned: recent_task_durations_micros");
            if durations_guard.len() == ROLLING_WINDOW_TASK_DURATIONS && !durations_guard.is_empty()
            {
                durations_guard.pop_front();
            }
            if ROLLING_WINDOW_TASK_DURATIONS > 0 {
                durations_guard.push_back(duration);
            }
        }

        // Add outcome to the rolling window.
        {
            let mut outcomes_guard = self
                .recent_task_outcomes
                .lock()
                .expect("Mutex poisoned: recent_task_outcomes");
            if outcomes_guard.len() == ROLLING_WINDOW_FAILURES && !outcomes_guard.is_empty() {
                outcomes_guard.pop_front();
            }
            if ROLLING_WINDOW_FAILURES > 0 {
                outcomes_guard.push_back(if success { 0 } else { 1 });
            }
        }
    }
}
