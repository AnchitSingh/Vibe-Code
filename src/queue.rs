// src/queue.rs

use crate::task::Task; // TaskId is part of Task
use crate::signals::{SystemSignal, NodeId, QueueId};
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::sync::mpsc;

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
            .field("was_overloaded", &self.was_overloaded.load(Ordering::Relaxed))
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
        if !(0.0 < low_watermark_percentage && low_watermark_percentage < high_watermark_percentage) {
            panic!("Invalid low_watermark_percentage: {} (must be > 0.0 and < high_watermark {})", low_watermark_percentage, high_watermark_percentage);
        }
        if !(low_watermark_percentage < high_watermark_percentage && high_watermark_percentage < 1.0) {
            panic!("Invalid high_watermark_percentage: {} (must be > low_watermark {} and < 1.0)", high_watermark_percentage, low_watermark_percentage);
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
        signal_tx: mpsc::Sender<SystemSignal>
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
                eprintln!("[OmegaQueue {}] Failed to send NodeOverloaded signal.", self.id);
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

pub fn dequeue(&self) -> Option<Task<Output>> {
    // Even if closed, allow dequeuing remaining tasks.
    // is_empty() check inside lock will handle final None.

    let mut tasks_guard = self.tasks.lock().expect("Queue mutex poisoned for dequeue");
    let task_option = tasks_guard.pop_front();

    if task_option.is_some() {
        let current_len = tasks_guard.len(); // Length after popping
        let current_pressure_level = self.calculate_pressure_level_internal(current_len);

        // Check if we were overloaded and now have recovered to a normal or lower pressure level
        if self.was_overloaded.load(Ordering::Relaxed) &&
           (current_pressure_level == PressureLevel::Normal ||
            current_pressure_level == PressureLevel::Low ||
            current_pressure_level == PressureLevel::Empty) {
            
            // Attempt to change was_overloaded from true to false.
            // If successful, it means this is the first dequeue operation that noticed recovery.
            if self.was_overloaded.compare_exchange(true, false, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                
                // Construct signal WITHOUT OpportunisticInfo
                let signal = SystemSignal::NodeIdle {
                    node_id: self.node_id,
                    queue_id: Some(self.id),
                    // No opportunistic field
                };

                if self.signal_tx.send(signal).is_err() {
                    // If sending fails, revert was_overloaded to true, as the "idle" state wasn't signaled.
                    self.was_overloaded.store(true, Ordering::SeqCst);
                    eprintln!("[OmegaQueue {}] Failed to send NodeIdle signal.", self.id);
                    // We don't return an error here as dequeue itself was successful.
                }
            }
        }
    } else if self.is_closed.load(Ordering::Relaxed) && tasks_guard.is_empty() {
        // If closed and truly empty after attempting pop, ensure was_overloaded is false
        // and potentially send a final NodeIdle if it was previously overloaded and hadn't recovered.
        if self.was_overloaded.compare_exchange(true, false, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
            let signal = SystemSignal::NodeIdle {
                node_id: self.node_id,
                queue_id: Some(self.id),
            };
            let _ = self.signal_tx.send(signal); // Best effort on shutdown
        }
        return None; // Explicitly return None if closed and empty
    }
    
    // Pressure update will be handled by OmegaNode after a successful dequeue.
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

    fn calculate_pressure_level_internal(&self, len: usize) -> PressureLevel {
        if len == 0 {
            return PressureLevel::Empty;
        }
        if len >= self.capacity {
            return PressureLevel::Full;
        }
        let ratio = len as f32 / self.capacity as f32;
        if ratio <= self.low_watermark_percentage {
            PressureLevel::Low
        } else if ratio <= self.high_watermark_percentage {
            PressureLevel::Normal
        } else {
            PressureLevel::High
        }
    }

    pub fn pressure_level(&self) -> PressureLevel {
        let len = self.len();
        self.calculate_pressure_level_internal(len)
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

