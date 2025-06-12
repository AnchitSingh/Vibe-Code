// src/queue.rs

use crate::signals::{NodeId, QueueId, SystemSignal};
use crate::task::Task; // TaskId is part of Task
use std::collections::VecDeque;
use std::fmt;
use std::sync::mpsc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

// --- QueueError ---
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    Full,
    Empty,
    Closed,
    SendError,
}

impl fmt::Display for QueueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueueError::Full => write!(f, "Queue is full"),
            QueueError::Empty => write!(f, "Queue is empty"),
            QueueError::Closed => write!(f, "Queue is closed"),
            QueueError::SendError => write!(f, "Failed to send system signal"),
        }
    }
}

impl std::error::Error for QueueError {}

// --- PressureLevel ---
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureLevel {
    Empty,
    Low,
    Normal,
    High,
    Full,
}

// --- OmegaQueue ---
pub struct OmegaQueue<Output> {
    id: QueueId,
    node_id: NodeId,
    tasks: Arc<Mutex<VecDeque<Task<Output>>>>,
    capacity: usize,
    low_watermark_percentage: f32,
    high_watermark_percentage: f32,
    signal_tx: mpsc::Sender<SystemSignal>,
    was_overloaded: Arc<AtomicBool>,
    is_closed: Arc<AtomicBool>,
}

impl<Output> fmt::Debug for OmegaQueue<Output> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OmegaQueue")
            .field("id", &self.id)
            .field("node_id", &self.node_id)
            .field("capacity", &self.capacity)
            .field("current_len", &self.len())
            .field("low_watermark", &self.low_watermark_percentage)
            .field("high_watermark", &self.high_watermark_percentage)
            .field(
                "was_overloaded",
                &self.was_overloaded.load(Ordering::Relaxed),
            )
            .field("is_closed", &self.is_closed.load(Ordering::Relaxed))
            .field("signal_tx", &"mpsc::Sender<SystemSignal>")
            .finish()
    }
}

impl<Output> OmegaQueue<Output> {
    pub fn with_watermarks_and_signal(
        node_id: NodeId,
        capacity: usize,
        low_watermark_percentage: f32,
        high_watermark_percentage: f32,
        signal_tx: mpsc::Sender<SystemSignal>,
    ) -> Self {
        if capacity == 0 {
            panic!("OmegaQueue capacity cannot be 0");
        }
        if !(0.0 < low_watermark_percentage && low_watermark_percentage < high_watermark_percentage)
        {
            panic!(
                "Invalid low_watermark_percentage: {} (must be > 0.0 and < high_watermark {})",
                low_watermark_percentage, high_watermark_percentage
            );
        }
        if !(low_watermark_percentage < high_watermark_percentage
            && high_watermark_percentage < 1.0)
        {
            panic!(
                "Invalid high_watermark_percentage: {} (must be > low_watermark {} and < 1.0)",
                high_watermark_percentage, low_watermark_percentage
            );
        }

        OmegaQueue {
            id: QueueId::new(),
            node_id,
            tasks: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
            low_watermark_percentage,
            high_watermark_percentage,
            signal_tx,
            was_overloaded: Arc::new(AtomicBool::new(false)),
            is_closed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn new_with_signal(
        node_id: NodeId,
        capacity: usize,
        signal_tx: mpsc::Sender<SystemSignal>,
    ) -> Self {
        Self::with_watermarks_and_signal(node_id, capacity, 0.25, 0.75, signal_tx)
    }

    pub fn close(&self) {
        self.is_closed.store(true, Ordering::SeqCst);
    }

    pub fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::Relaxed)
    }

    // src/queue.rs
    // Inside impl<Output> OmegaQueue<Output>

    pub fn enqueue(&self, task: Task<Output>) -> Result<(), QueueError> {
        if self.is_closed.load(Ordering::Relaxed) {
            return Err(QueueError::Closed);
        }

        let mut tasks_guard = self.tasks.lock().expect("Queue mutex poisoned for enqueue");
        let current_len = tasks_guard.len();

        if current_len >= self.capacity {
            // Queue is full
            if !self.was_overloaded.swap(true, Ordering::SeqCst) {
                // Only send NodeOverloaded if it wasn't already marked as overloaded.
                // This prevents signal storms.

                // Construct signal WITHOUT OpportunisticInfo
                let signal = SystemSignal::NodeOverloaded {
                    node_id: self.node_id,
                    queue_id: Some(self.id),
                    // No opportunistic field
                };

                if self.signal_tx.send(signal).is_err() {
                    // If sending fails (e.g., SystemPulse disconnected),
                    // revert was_overloaded state as the signal wasn't effectively sent.
                    self.was_overloaded.store(false, Ordering::SeqCst);
                    // Log this error, as it might indicate a problem with SystemPulse.
                    eprintln!(
                        "[OmegaQueue {}] Failed to send NodeOverloaded signal.",
                        self.id
                    );
                    return Err(QueueError::SendError); // Propagate as a send error
                }
            }
            return Err(QueueError::Full);
        }

        // Enqueue the task if not full
        tasks_guard.push_back(task);
        // No need to update pressure here; OmegaNode will do it after successful enqueue.
        Ok(())
    }

    // src/queue.rs
    // Inside impl<Output> OmegaQueue<Output>

    /// Dequeues a task from the front of the queue.
    ///
    /// This method will attempt to pop a task. If a task is successfully dequeued,
    /// it checks if the queue was previously in an "overloaded" state (i.e., it had
    /// hit its capacity). If it was, and the current queue length has now dropped
    /// below a low watermark threshold, it signals that the node is now "idle"
    /// (no longer under maximum pressure).
    ///
    /// This allows the system to continue processing tasks even if the queue is technically
    /// closed, ensuring no work is lost during shutdown.
    pub fn dequeue(&self) -> Option<Task<Output>> {
        // It's crucial to lock the mutex to ensure all operations on the inner VecDeque
        // are atomic and thread-safe.
        let mut tasks_guard = self.tasks.lock().expect("Queue mutex poisoned for dequeue");

        let task_option = tasks_guard.pop_front();

        if task_option.is_some() {
            // A task was successfully removed. Now check if we need to signal recovery.
            let current_len = tasks_guard.len(); // The length *after* popping the task.

            // Calculate the raw count for the low watermark based on the percentage.
            // This is done once to avoid repeated calculations.
            let low_watermark_count =
                (self.capacity as f32 * self.low_watermark_percentage) as usize;

            // Check for recovery:
            // 1. Was the queue previously marked as overloaded?
            // 2. Has the number of items now dropped to or below our recovery threshold?
            if self.was_overloaded.load(Ordering::Relaxed) && current_len <= low_watermark_count {
                // Attempt to atomically switch `was_overloaded` from `true` to `false`.
                // The `compare_exchange` ensures that only the *first thread* to notice
                // the recovery will send the signal, preventing a storm of `NodeIdle` signals.
                if self
                    .was_overloaded
                    .compare_exchange(true, false, Ordering::SeqCst, Ordering::Relaxed)
                    .is_ok()
                {
                    // We successfully claimed the right to send the signal.
                    let signal = SystemSignal::NodeIdle {
                        node_id: self.node_id,
                        queue_id: Some(self.id),
                    };

                    // Send the signal. If the receiver has disconnected, it's not a critical
                    // error for the dequeue operation itself, but we should handle it gracefully.
                    if self.signal_tx.send(signal).is_err() {
                        // The SystemPulse seems to be down. This is a problem, but not this
                        // function's primary responsibility to solve. We log it and revert
                        // the `was_overloaded` state so a future thread can try again.
                        eprintln!(
                            "[OmegaQueue {}] Failed to send NodeIdle signal; receiver disconnected.",
                            self.id
                        );
                        self.was_overloaded.store(true, Ordering::SeqCst);
                    }
                }
            }
        } else if self.is_closed.load(Ordering::Relaxed) && tasks_guard.is_empty() {
            // This handles a specific edge case on shutdown. If the queue is marked as
            // closed and is now truly empty, we must ensure a final `NodeIdle` signal
            // is sent if it was the last task being processed from an overloaded state.
            if self
                .was_overloaded
                .compare_exchange(true, false, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                let signal = SystemSignal::NodeIdle {
                    node_id: self.node_id,
                    queue_id: Some(self.id),
                };
                // On shutdown, we send this on a "best effort" basis.
                let _ = self.signal_tx.send(signal);
            }
            // Return None explicitly as there are no more tasks and the queue is closed.
            return None;
        }

        // Return the task that was dequeued.
        task_option
    }
    pub fn len(&self) -> usize {
        self.tasks.lock().expect("Queue mutex poisoned").len()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_full(&self) -> bool {
        self.len() >= self.capacity
    }

    pub fn id(&self) -> QueueId {
        self.id
    }

}

impl<Output> Clone for OmegaQueue<Output> {
    fn clone(&self) -> Self {
        OmegaQueue {
            id: self.id,
            node_id: self.node_id,
            tasks: Arc::clone(&self.tasks),
            capacity: self.capacity,
            low_watermark_percentage: self.low_watermark_percentage,
            high_watermark_percentage: self.high_watermark_percentage,
            signal_tx: self.signal_tx.clone(),
            was_overloaded: Arc::clone(&self.was_overloaded),
            is_closed: Arc::clone(&self.is_closed),
        }
    }
}
