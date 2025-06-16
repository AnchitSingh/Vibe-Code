// src/signals.rs

use crate::task::TaskId;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

// --- NodeId, QueueId (unchanged) ---
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub usize);
static NEXT_NODE_ID: AtomicUsize = AtomicUsize::new(1);
impl NodeId { pub fn new() -> Self { NodeId(NEXT_NODE_ID.fetch_add(1, Ordering::Relaxed)) } }
impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}
impl fmt::Display for NodeId { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Node({})", self.0) } }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QueueId(usize);
static NEXT_QUEUE_ID: AtomicUsize = AtomicUsize::new(1);
impl QueueId { 
    pub fn new() -> Self {
         QueueId(NEXT_QUEUE_ID.fetch_add(1, Ordering::Relaxed)) 
    }
}
impl Default for QueueId {
    fn default() -> Self {
        Self::new()
    }
}
impl fmt::Display for QueueId { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Queue({})", self.0) } }


// --- REFACTORED: SystemSignal ---
/// Signals exchanged within the Ultra-Ω system, focusing on operational lifecycle.
#[derive(Debug, Clone)]
pub enum SystemSignal {
    /// Emitted when a node's primary queue is considered overloaded.
    NodeOverloaded {
        node_id: NodeId,
        queue_id: Option<QueueId>,
    },
    /// Emitted when a previously overloaded node's queue has drained to a safe level.
    NodeIdle {
        node_id: NodeId,
        queue_id: Option<QueueId>,
    },
    /// Emitted by a worker just before it starts processing a task.
    TaskDequeuedByWorker {
        node_id: NodeId,
        task_id: TaskId,
    },
    /// Emitted by a worker after it has finished processing a task,
    /// regardless of the task's logical success or failure.
    TaskProcessed {
        node_id: NodeId,
        task_id: TaskId,
        duration_micros: u64,
    },
}

impl SystemSignal {
    pub fn get_node_id(&self) -> NodeId {
        match self {
            SystemSignal::NodeOverloaded { node_id, .. } => *node_id,
            SystemSignal::NodeIdle { node_id, .. } => *node_id,
            SystemSignal::TaskDequeuedByWorker { node_id, .. } => *node_id,
            SystemSignal::TaskProcessed { node_id, .. } => *node_id,
        }
    }
}