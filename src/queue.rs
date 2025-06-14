// src/queue.rs

use crate::signals::{NodeId, QueueId, SystemSignal};
use crate::task::Task;
use std::collections::VecDeque;
use std::fmt;
use std::sync::mpsc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

// --- QueueError (unchanged) ---
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


// --- OmegaQueue ---
// This remains generic
pub struct OmegaQueue<T> {
    id: QueueId,
    node_id: NodeId,
    tasks: Arc<Mutex<VecDeque<T>>>,
    capacity: usize,
    low_watermark_percentage: f32,
    high_watermark_percentage: f32,
    signal_tx: mpsc::Sender<SystemSignal>,
    was_overloaded: Arc<AtomicBool>,
    is_closed: Arc<AtomicBool>,
}

impl<T> fmt::Debug for OmegaQueue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OmegaQueue")
            .field("id", &self.id)
            .field("node_id", &self.node_id)
            .field("capacity", &self.capacity)
            .field("current_len", &self.len())
            .field("is_closed", &self.is_closed.load(Ordering::Relaxed))
            .finish()
    }
}

// --- NEW/CORRECTED BLOCK: Generic methods for ANY OmegaQueue<T> ---
/// This block contains methods that are generic and do not depend on the inner type `T`.
impl<T> OmegaQueue<T> {
    pub fn len(&self) -> usize {
        self.tasks.lock().expect("Queue mutex poisoned for len").len()
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

    pub fn close(&self) {
        self.is_closed.store(true, Ordering::SeqCst);
    }

    pub fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::Relaxed)
    }
}

// --- Specific methods for OmegaQueue<Task> ---
/// This block contains methods that are specific to a queue holding `Task` structs
/// and interacting with the `SystemSignal` enum.
impl OmegaQueue<Task> {
    pub fn with_watermarks_and_signal(
        node_id: NodeId,
        capacity: usize,
        low_watermark_percentage: f32,
        high_watermark_percentage: f32,
        signal_tx: mpsc::Sender<SystemSignal>,
    ) -> Self {
        if capacity == 0 { panic!("OmegaQueue capacity cannot be 0"); }
        if !(0.0 < low_watermark_percentage && low_watermark_percentage < high_watermark_percentage) {
            panic!("Invalid low_watermark_percentage");
        }
        if !(low_watermark_percentage < high_watermark_percentage && high_watermark_percentage < 1.0) {
            panic!("Invalid high_watermark_percentage");
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

    pub fn dequeue(&self) -> Option<Task> {
        let mut tasks_guard = self.tasks.lock().expect("Queue mutex poisoned for dequeue");
        let task_option = tasks_guard.pop_front();

        if task_option.is_some() {
            let current_len = tasks_guard.len();
            let low_watermark_count = (self.capacity as f32 * self.low_watermark_percentage) as usize;
            if self.was_overloaded.load(Ordering::Relaxed) && current_len <= low_watermark_count {
                if self.was_overloaded.compare_exchange(true, false, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                    let signal = SystemSignal::NodeIdle {
                        node_id: self.node_id,
                        queue_id: Some(self.id),
                    };
                    if self.signal_tx.send(signal).is_err() {
                        self.was_overloaded.store(true, Ordering::SeqCst);
                    }
                }
            }
        } else if self.is_closed.load(Ordering::Relaxed) && tasks_guard.is_empty() {
            if self.was_overloaded.compare_exchange(true, false, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
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

// The generic Clone implementation remains correct.
impl<T> Clone for OmegaQueue<T> {
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