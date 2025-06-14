// src/types.rs

use crate::queue::QueueError;
use std::collections::VecDeque;
use std::fmt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

// --- Constants for Rolling Windows ---
const ROLLING_WINDOW_TASK_DURATIONS: usize = 100;
const ROLLING_WINDOW_FAILURES: usize = 50;

// --- NodeError ---
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeError {
    QueueFull,
    QueueClosed,
    NodeShuttingDown,
    SignalSendError,
    NoNodesAvailable,
    SystemMaxedOut,
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

// --- LocalStats ---
/// Tracks statistics for an OmegaNode, including both all-time and rolling window metrics.
#[derive(Debug)]
pub struct LocalStats {
    tasks_submitted: AtomicUsize,
    tasks_processed: AtomicUsize,
    tasks_succeeded: AtomicUsize,
    tasks_failed: AtomicUsize,
    total_processing_time_micros: AtomicU64,

    // Rolling window statistics (kept for potential internal node heuristics)
    recent_task_durations_micros: Mutex<VecDeque<u64>>,
    recent_task_outcomes: Mutex<VecDeque<u8>>,
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
        self.total_processing_time_micros
            .fetch_add(duration, Ordering::Relaxed);

        if success {
            self.tasks_succeeded.fetch_add(1, Ordering::Relaxed);
        } else {
            self.tasks_failed.fetch_add(1, Ordering::Relaxed);
        }

        {
            let mut durations_guard = self.recent_task_durations_micros.lock().expect(
                "Mutex poisoned: recent_task_durations_micros",
            );
            if durations_guard.len() == ROLLING_WINDOW_TASK_DURATIONS && !durations_guard.is_empty() {
                durations_guard.pop_front();
            }
            if ROLLING_WINDOW_TASK_DURATIONS > 0 {
                durations_guard.push_back(duration);
            }
        }

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