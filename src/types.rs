// src/types.rs

use crate::queue::QueueError; // For NodeError From impl
use std::collections::VecDeque;
use std::fmt;
use std::sync::Mutex; // For VecDeques in LocalStats
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

// --- Constants for Rolling Windows ---
// These define the size of the windows for LocalStats' internal tracking.
const ROLLING_WINDOW_TASK_DURATIONS: usize = 100; // Calculate average over last 100 tasks
const ROLLING_WINDOW_FAILURES: usize = 50;       // Count failures in last 50 tasks

// --- NodeError ---
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeError {
    QueueFull,
    QueueClosed,
    NodeShuttingDown,
    SignalSendError,
    NoNodesAvailable, // Added in previous step
    SystemMaxedOut,   // Added in previous step
}

impl fmt::Display for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeError::QueueFull => write!(f, "Node task queue is full"),
            NodeError::QueueClosed => write!(f, "Node task queue is closed"),
            NodeError::NodeShuttingDown => write!(f, "Node is shutting down"),
            NodeError::SignalSendError => write!(f, "Node failed to send a system signal"),
            NodeError::NoNodesAvailable => write!(f, "No OmegaNodes available for task submission"),
            NodeError::SystemMaxedOut => write!(f, "System is maxed out; all sampled nodes are at full pressure"),
        }
    }
}

impl std::error::Error for NodeError {}

// impl From<QueueError> for NodeError {
//     fn from(q_err: QueueError) -> Self {
//         match q_err {
//             QueueError::Full => NodeError::QueueFull,
//             QueueError::Closed => NodeError::QueueClosed,
//             QueueError::Empty => {
//                 eprintln!(
//                     "Warning: Converting QueueError::Empty to NodeError::QueueClosed. Review if this is the desired mapping."
//                 );
//                 NodeError::QueueClosed
//             }
//             QueueError::SendError => NodeError::SignalSendError,
//         }
//     }
// }

// --- LocalStats ---
/// Tracks statistics for an OmegaNode, including both all-time and rolling window metrics.
#[derive(Debug)]
pub struct LocalStats {
    tasks_submitted: AtomicUsize,
    tasks_processed: AtomicUsize,
    tasks_succeeded: AtomicUsize,
    tasks_failed: AtomicUsize,               // All-time failures
    total_processing_time_micros: AtomicU64, // All-time total duration

    // Rolling window statistics (kept for potential internal node heuristics)
    recent_task_durations_micros: Mutex<VecDeque<u64>>, // Stores durations in micros
    recent_task_outcomes: Mutex<VecDeque<u8>>,         // Stores 1 for failure, 0 for success
}

impl LocalStats {
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

    pub fn task_submitted(&self) {
        self.tasks_submitted.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_task_outcome(&self, duration: u64, success: bool) {
        // Update overall (all-time) stats
        self.total_processing_time_micros
            .fetch_add(duration, Ordering::Relaxed);

        if success {
            self.tasks_succeeded.fetch_add(1, Ordering::Relaxed);
        } else {
            self.tasks_failed.fetch_add(1, Ordering::Relaxed);
        }

        // Update rolling window for durations
        {
            let mut durations_guard = self.recent_task_durations_micros.lock().expect(
                "Mutex poisoned: recent_task_durations_micros",
            );
            if durations_guard.len() == ROLLING_WINDOW_TASK_DURATIONS && !durations_guard.is_empty() {
                durations_guard.pop_front(); // Remove oldest if window is full
            }
            if ROLLING_WINDOW_TASK_DURATIONS > 0 { // Only push if window size is > 0
                durations_guard.push_back(duration);
            }
        }

        // Update rolling window for failure tracking
        {
            let mut outcomes_guard = self
                .recent_task_outcomes
                .lock()
                .expect("Mutex poisoned: recent_task_outcomes");
            if outcomes_guard.len() == ROLLING_WINDOW_FAILURES && !outcomes_guard.is_empty() {
                outcomes_guard.pop_front(); // Remove oldest
            }
            if ROLLING_WINDOW_FAILURES > 0 { // Only push if window size is > 0
                 outcomes_guard.push_back(if success { 0 } else { 1 });
            }
        }
    }

}
