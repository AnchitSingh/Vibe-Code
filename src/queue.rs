//! Implements `VibeQueue`, a bounded, thread-safe queue for managing tasks within an `VibeNode`.
//!
//! This module provides the core queueing mechanism, including error handling for
//! queue-specific operations and logic for signaling node overload/idle states
//! based on configurable watermarks.

use crate::signals::{NodeId, QueueId, SystemSignal};
use crate::task::Task;
use std::collections::VecDeque;
use std::fmt;
use std::sync::mpsc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

// --- QueueError ---
/// Represents errors that can occur during `VibeQueue` operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    /// The queue has reached its maximum capacity and cannot accept more items.
    Full,
    /// The queue is empty, and no items can be dequeued.
    Empty,
    /// The queue has been explicitly closed and no longer accepts or yields items.
    Closed,
    /// Failed to send a `SystemSignal` related to queue state (e.g., overload/idle).
    SendError,
}

impl fmt::Display for QueueError {
    /// Implements the `Display` trait for `QueueError`, providing human-readable error messages.
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

// --- VibeQueue ---
/// A bounded, thread-safe queue for holding items of type `T`.
///
/// `VibeQueue` is designed to manage task backpressure within an `VibeNode`.
/// It supports configurable high and low watermarks to signal `NodeOverloaded`
/// and `NodeIdle` events, respectively, to a central system component.
pub struct VibeQueue<T> {
    /// The unique identifier for this queue.
    id: QueueId,
    /// The ID of the `VibeNode` this queue belongs to.
    node_id: NodeId,
    /// The underlying `VecDeque` protected by a `Mutex` for thread-safe access.
    tasks: Arc<Mutex<VecDeque<T>>>,
    /// The maximum number of items this queue can hold.
    capacity: usize,
    /// The percentage of capacity at or below which the queue is considered "idle"
    /// after being overloaded.
    low_watermark_percentage: f32,
    /// The percentage of capacity at or above which the queue is considered "overloaded".
    high_watermark_percentage: f32,
    /// Sender channel for sending `SystemSignal`s to a central system component.
    signal_tx: mpsc::Sender<SystemSignal>,
    /// An atomic flag indicating if the queue was previously in an overloaded state.
    was_overloaded: Arc<AtomicBool>,
    /// An atomic flag indicating if the queue has been closed.
    is_closed: Arc<AtomicBool>,
}

impl<T> fmt::Debug for VibeQueue<T> {
    /// Implements the `Debug` trait for `VibeQueue`, providing a concise representation.
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

/// Generic methods for any `VibeQueue<T>` regardless of the inner type `T`.
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

    /// Returns the unique `QueueId` of this queue.
    pub fn id(&self) -> QueueId {
        self.id
    }

    /// Closes the queue, preventing further enqueue operations.
    ///
    /// Existing items can still be dequeued until the queue is empty.
    pub fn close(&self) {
        self.is_closed.store(true, Ordering::SeqCst);
    }

    /// Returns `true` if the queue has been closed.
    pub fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::Relaxed)
    }
}

/// Specific methods for `VibeQueue` instances holding `Task` structs.
///
/// This block includes constructors and methods for enqueueing and dequeuing
/// `Task`s, with integrated logic for sending `SystemSignal`s based on
/// queue watermarks.
impl VibeQueue<Task> {
    /// Creates a new `VibeQueue` for `Task`s with specified watermarks and a signal sender.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The `NodeId` of the owning `VibeNode`.
    /// * `capacity` - The maximum number of tasks the queue can hold.
    /// * `low_watermark_percentage` - The percentage (0.0 to 1.0) at which to signal `NodeIdle`.
    /// * `high_watermark_percentage` - The percentage (0.0 to 1.0) at which to signal `NodeOverloaded`.
    /// * `signal_tx` - A sender for `SystemSignal`s.
    ///
    /// # Panics
    ///
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

    /// Creates a new `VibeQueue` for `Task`s with default watermark percentages (0.25 and 0.75).
    ///
    /// # Arguments
    ///
    /// * `node_id` - The `NodeId` of the owning `VibeNode`.
    /// * `capacity` - The maximum number of tasks the queue can hold.
    /// * `signal_tx` - A sender for `SystemSignal`s.
    pub fn new_with_signal(
        node_id: NodeId,
        capacity: usize,
        signal_tx: mpsc::Sender<SystemSignal>,
    ) -> Self {
        Self::with_watermarks_and_signal(node_id, capacity, 0.25, 0.75, signal_tx)
    }

    /// Attempts to enqueue a `Task` into the queue.
    ///
    /// If the queue is full, it may send a `NodeOverloaded` signal.
    ///
    /// # Arguments
    ///
    /// * `task` - The `Task` to enqueue.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success (`Ok(())`) or a `QueueError` if the queue
    /// is full, closed, or a signal could not be sent.
    pub fn enqueue(&self, task: Task) -> Result<(), QueueError> {
        if self.is_closed.load(Ordering::Relaxed) {
            return Err(QueueError::Closed);
        }

        let mut tasks_guard = self.tasks.lock().expect("Queue mutex poisoned for enqueue");

        if tasks_guard.len() >= self.capacity {
            // If the queue is full and was not previously marked as overloaded, send a signal.
            if !self.was_overloaded.swap(true, Ordering::SeqCst) {
                let signal = SystemSignal::NodeOverloaded {
                    node_id: self.node_id,
                    queue_id: Some(self.id),
                };
                if self.signal_tx.send(signal).is_err() {
                    // If signal sending fails, revert the overloaded flag.
                    self.was_overloaded.store(false, Ordering::SeqCst);
                    return Err(QueueError::SendError);
                }
            }
            return Err(QueueError::Full);
        }

        tasks_guard.push_back(task);
        Ok(())
    }

    /// Attempts to dequeue a `Task` from the queue.
    ///
    /// If the queue's length drops below the low watermark after dequeuing,
    /// and it was previously overloaded, it may send a `NodeIdle` signal.
    ///
    /// # Returns
    ///
    /// An `Option<Task>` containing the dequeued task if available, or `None`
    /// if the queue is empty or closed and empty.
    pub fn dequeue(&self) -> Option<Task> {
        let mut tasks_guard = self.tasks.lock().expect("Queue mutex poisoned for dequeue");
        let task_option = tasks_guard.pop_front();

        if task_option.is_some() {
            let current_len = tasks_guard.len();
            let low_watermark_count =
                (self.capacity as f32 * self.low_watermark_percentage) as usize;
            // If the queue was overloaded and now drops below the low watermark, signal idle.
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
                    // If signal sending fails, revert the overloaded flag.
                    self.was_overloaded.store(true, Ordering::SeqCst);
                }
            }
        } else if self.is_closed.load(Ordering::Relaxed) && tasks_guard.is_empty() {
            // If the queue is closed and now empty, ensure NodeIdle signal is sent if it was overloaded.
            if self
                .was_overloaded
                .compare_exchange(true, false, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                let signal = SystemSignal::NodeIdle {
                    node_id: self.node_id,
                    queue_id: Some(self.id),
                };
                let _ = self.signal_tx.send(signal); // Ignore send error during shutdown
            }
            return None;
        }

        task_option
    }
}

/// Implements the `Clone` trait for `VibeQueue`, allowing it to be cheaply cloned.
///
/// Cloning an `VibeQueue` creates a new handle to the *same* underlying queue
/// data structure, as `tasks`, `was_overloaded`, and `is_closed` are `Arc`s.
impl<T> Clone for VibeQueue<T> {
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
