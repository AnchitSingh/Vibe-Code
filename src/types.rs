//! Defines common types and error enumerations used throughout the CPU Circulatory System.
//!
//! This module includes definitions for `NodeError` (representing various errors
//! that can occur within an `OmegaNode`) and `LocalStats` (for tracking node-specific
//! performance metrics).

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

// --- Constants for Rolling Windows ---
/// The maximum number of recent task durations to keep in the rolling window for `LocalStats`.
const ROLLING_WINDOW_TASK_DURATIONS: usize = 100;
/// The maximum number of recent task outcomes (success/failure) to keep in the rolling window for `LocalStats`.
const ROLLING_WINDOW_FAILURES: usize = 50;

// --- NodeError ---
/// Represents various errors that can occur during `OmegaNode` operations or task submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeError {
    /// The task queue of an `OmegaNode` is full, and no more tasks can be submitted.
    QueueFull,
    /// The task queue of an `OmegaNode` has been closed, preventing further submissions.
    QueueClosed,
    /// The `OmegaNode` is in the process of shutting down and cannot accept new tasks.
    NodeShuttingDown,
    /// Failed to send a system signal (e.g., `SystemSignal`) from a node.
    SignalSendError,
    /// No `OmegaNode`s are available in the system to accept a task.
    NoNodesAvailable,
    /// The system is maxed out; all sampled nodes are at their full pressure capacity,
    /// and no suitable node could be found for task submission after multiple retries.
    SystemMaxedOut,
}

impl fmt::Display for NodeError {
    /// Implements the `Display` trait for `NodeError`, providing human-readable error messages.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeError::QueueFull => write!(f, "Node task queue is full"),
            NodeError::QueueClosed => write!(f, "Node task queue is closed"),
            NodeError::NodeShuttingDown => write!(f, "Node is shutting down"),
            NodeError::SignalSendError => write!(f, "Node failed to send a system signal"),
            NodeError::NoNodesAvailable => {
                write!(f, "No OmegaNodes available for task submission")
            }
            NodeError::SystemMaxedOut => write!(
                f,
                "System is maxed out; all sampled nodes are at full pressure"
            ),
        }
    }
}

impl std::error::Error for NodeError {}

// --- LocalStats ---
/// Tracks statistics for an `OmegaNode`, including both all-time and rolling window metrics.
///
/// This struct provides insights into the performance and workload of an individual node,
/// such as task submission rates, processing times, and success/failure rates.
#[derive(Debug)]
pub struct LocalStats {
    /// The total number of tasks submitted to this node since its creation.
    tasks_submitted: AtomicUsize,
    /// The total number of tasks that have begun processing on this node.
    tasks_processed: AtomicUsize,
    /// The total number of tasks that successfully completed on this node.
    tasks_succeeded: AtomicUsize,
    /// The total number of tasks that failed on this node.
    tasks_failed: AtomicUsize,
    /// The cumulative processing time for all tasks on this node, in microseconds.
    total_processing_time_micros: AtomicU64,

    /// A rolling window of recent task durations (in microseconds), used for short-term heuristics.
    recent_task_durations_micros: Mutex<VecDeque<u64>>,
    /// A rolling window of recent task outcomes (0 for success, 1 for failure), used for failure rate tracking.
    recent_task_outcomes: Mutex<VecDeque<u8>>,
}

impl Default for LocalStats {
    /// Creates a new `LocalStats` instance with all counters initialized to zero
    /// and empty rolling windows.
    fn default() -> Self {
        Self::new()
    }
}

impl LocalStats {
    /// Creates a new `LocalStats` instance with all counters initialized to zero
    /// and empty rolling windows, pre-allocated with specified capacities.
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

    /// Increments the count of tasks submitted to this node.
    pub fn task_submitted(&self) {
        self.tasks_submitted.fetch_add(1, Ordering::Relaxed);
    }

    /// Records the outcome of a processed task, updating total counts and rolling windows.
    ///
    /// # Arguments
    ///
    /// * `duration` - The processing time of the task in microseconds.
    /// * `success` - A boolean indicating whether the task completed successfully.
    pub fn record_task_outcome(&self, duration: u64, success: bool) {
        self.total_processing_time_micros
            .fetch_add(duration, Ordering::Relaxed);

        if success {
            self.tasks_succeeded.fetch_add(1, Ordering::Relaxed);
        } else {
            self.tasks_failed.fetch_add(1, Ordering::Relaxed);
        }

        // Update rolling window for task durations.
        {
            let mut durations_guard = self
                .recent_task_durations_micros
                .lock()
                .expect("Mutex poisoned: recent_task_durations_micros");
            if durations_guard.len() == ROLLING_WINDOW_TASK_DURATIONS && !durations_guard.is_empty()
            {
                durations_guard.pop_front(); // Remove oldest entry if capacity is reached
            }
            if ROLLING_WINDOW_TASK_DURATIONS > 0 {
                durations_guard.push_back(duration); // Add new duration
            }
        }

        // Update rolling window for task outcomes.
        {
            let mut outcomes_guard = self
                .recent_task_outcomes
                .lock()
                .expect("Mutex poisoned: recent_task_outcomes");
            if outcomes_guard.len() == ROLLING_WINDOW_FAILURES && !outcomes_guard.is_empty() {
                outcomes_guard.pop_front(); // Remove oldest entry if capacity is reached
            }
            if ROLLING_WINDOW_FAILURES > 0 {
                outcomes_guard.push_back(if success { 0 } else { 1 }); // 0 for success, 1 for failure
            }
        }
    }
}