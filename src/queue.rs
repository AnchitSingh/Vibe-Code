//! Implements `VibeQueue`, a thread-safe, bounded queue for tasks.
//!
//! This module provides the core queuing mechanism for a `VibeNode`. It manages
//! task backpressure and signals the node when it becomes overloaded or idle,
//! based on configurable high and low watermarks.

use crate::signals::{NodeId, QueueId, SystemSignal};
use crate::task::Task;
use std::collections::VecDeque;
use std::fmt;
use std::sync::mpsc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

/// Represents errors that can occur during queue operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    /// The queue has reached its maximum capacity.
    Full,
    /// The queue is empty.
    Empty,
    /// The queue has been closed and can no longer be used.
    Closed,
    /// Failed to send a system signal regarding queue state.
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

/// A bounded, thread-safe queue for holding tasks.
///
/// `VibeQueue` is used within each `VibeNode` to buffer incoming tasks. It sends
/// `NodeOverloaded` and `NodeIdle` signals to the system when its size crosses
/// defined percentage thresholds (watermarks).
pub struct VibeQueue<T> {
    /// A unique identifier for this queue.
    id: QueueId,
    /// The ID of the node that owns this queue.
    node_id: NodeId,
    /// The internal queue storage, protected by a `Mutex`.
    tasks: Arc<Mutex<VecDeque<T>>>,
    /// The maximum number of items the queue can hold.
    capacity: usize,
    /// The percentage of capacity at or below which the queue is considered "idle".
    low_watermark_percentage: f32,
    /// The percentage of capacity at or above which the queue is considered "overloaded".
    high_watermark_percentage: f32,
    /// A channel to send signals to the central system.
    signal_tx: mpsc::Sender<SystemSignal>,
    /// A flag to track if the queue is currently in an overloaded state.
    was_overloaded: Arc<AtomicBool>,
    /// A flag to indicate if the queue has been permanently closed.
    is_closed: Arc<AtomicBool>,
}

impl<T> fmt::Debug for VibeQueue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VibeQueue")
            .field("id", &self.id)
            .field("node_id", &self.node_id)
            .field("capacity", &self.capacity)
            .field("current_len", &self.len())
            .field("is_closed", &self.is_closed.load(Ordering::Relaxed))
            .finish()
    }
}

impl<T> VibeQueue<T> {
    /// Returns the current number of items in the queue.
    pub fn len(&self) -> usize {
        self.tasks
            .lock()
            .expect("Queue mutex poisoned for len")
            .len()
    }

    /// Returns the maximum capacity of the queue.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns `true` if the queue contains no items.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if the queue has reached its maximum capacity.
    pub fn is_full(&self) -> bool {
        self.len() >= self.capacity
    }

    /// Returns the unique ID of this queue.
    pub fn id(&self) -> QueueId {
        self.id
    }

    /// Closes the queue, preventing any new items from being added.
    ///
    /// Items already in the queue can still be removed.
    pub fn close(&self) {
        self.is_closed.store(true, Ordering::SeqCst);
    }

    /// Returns `true` if the queue has been closed.
    pub fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::Relaxed)
    }
}

impl VibeQueue<Task> {
    /// Creates a new `VibeQueue` with custom watermarks for signaling.
    ///
    /// # Panics
    /// Panics if `capacity` is 0 or if watermark percentages are invalid.
    pub fn with_watermarks_and_signal(
        node_id: NodeId,
        capacity: usize,
        low_watermark_percentage: f32,
        high_watermark_percentage: f32,
        signal_tx: mpsc::Sender<SystemSignal>,
    ) -> Self {
        if capacity == 0 {
            panic!("VibeQueue capacity cannot be 0");
        }
        if !(0.0 < low_watermark_percentage && low_watermark_percentage < high_watermark_percentage)
        {
            panic!("Invalid low_watermark_percentage");
        }
        if !(low_watermark_percentage < high_watermark_percentage
            && high_watermark_percentage < 1.0)
        {
            panic!("Invalid high_watermark_percentage");
        }

        VibeQueue {
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

    /// Creates a new `VibeQueue` with default watermarks (25% and 75%).
    pub fn new_with_signal(
        node_id: NodeId,
        capacity: usize,
        signal_tx: mpsc::Sender<SystemSignal>,
    ) -> Self {
        Self::with_watermarks_and_signal(node_id, capacity, 0.25, 0.75, signal_tx)
    }

    /// Adds a task to the back of the queue.
    ///
    /// If adding the task causes the queue to become full, it sends a
    /// `NodeOverloaded` signal.
    pub fn enqueue(&self, task: Task) -> Result<(), QueueError> {
        if self.is_closed.load(Ordering::Relaxed) {
            return Err(QueueError::Closed);
        }

        let mut tasks_guard = self.tasks.lock().expect("Queue mutex poisoned for enqueue");

        if tasks_guard.len() >= self.capacity {
            if !self.was_overloaded.swap(true, Ordering::SeqCst) {
                let signal = SystemSignal::NodeOverloaded {
                    node_id: self.node_id,
                    queue_id: Some(self.id),
                };
                if self.signal_tx.send(signal).is_err() {
                    self.was_overloaded.store(false, Ordering::SeqCst);
                    return Err(QueueError::SendError);
                }
            }
            return Err(QueueError::Full);
        }

        tasks_guard.push_back(task);
        Ok(())
    }

    /// Removes a task from the front of the queue.
    ///
    /// If removing the task causes a previously overloaded queue to drop below
    /// its low watermark, it sends a `NodeIdle` signal.
    pub fn dequeue(&self) -> Option<Task> {
        let mut tasks_guard = self.tasks.lock().expect("Queue mutex poisoned for dequeue");
        let task_option = tasks_guard.pop_front();

        if task_option.is_some() {
            let current_len = tasks_guard.len();
            let low_watermark_count =
                (self.capacity as f32 * self.low_watermark_percentage) as usize;

            if self.was_overloaded.load(Ordering::Relaxed)
                && current_len <= low_watermark_count
                && self
                    .was_overloaded
                    .compare_exchange(true, false, Ordering::SeqCst, Ordering::Relaxed)
                    .is_ok()
            {
                let signal = SystemSignal::NodeIdle {
                    node_id: self.node_id,
                    queue_id: Some(self.id),
                };
                if self.signal_tx.send(signal).is_err() {
                    self.was_overloaded.store(true, Ordering::SeqCst);
                }
            }
        } else if self.is_closed.load(Ordering::Relaxed) && tasks_guard.is_empty() {
            if self
                .was_overloaded
                .compare_exchange(true, false, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                let signal = SystemSignal::NodeIdle {
                    node_id: self.node_id,
                    queue_id: Some(self.id),
                };
                let _ = self.signal_tx.send(signal);
            }
            return None;
        }

        task_option
    }
}

impl<T> Clone for VibeQueue<T> {
    /// Clones the queue.
    ///
    /// This is a cheap operation, as it only clones the `Arc` pointers to the
    /// underlying queue data, not the data itself.
    fn clone(&self) -> Self {
        VibeQueue {
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
