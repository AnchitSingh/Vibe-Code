//! Defines unique identifiers and system-wide signals for communication.
//!
//! This module provides the core vocabulary for different parts of the system
//! to talk to each other. It includes unique IDs for nodes and queues, and an
//! enum of `SystemSignal`s that report important events like a node being
//! overloaded or a task finishing.

use crate::task::TaskId;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A unique identifier for a `VibeNode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub usize);

/// An atomic counter to generate unique node IDs.
static NEXT_NODE_ID: AtomicUsize = AtomicUsize::new(1);

impl NodeId {
    /// Creates a new, unique `NodeId`.
    pub fn new() -> Self {
        NodeId(NEXT_NODE_ID.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({})", self.0)
    }
}

/// A unique identifier for a `VibeQueue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QueueId(usize);

/// An atomic counter to generate unique queue IDs.
static NEXT_QUEUE_ID: AtomicUsize = AtomicUsize::new(1);

impl QueueId {
    /// Creates a new, unique `QueueId`.
    pub fn new() -> Self {
        QueueId(NEXT_QUEUE_ID.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for QueueId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for QueueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Queue({})", self.0)
    }
}

/// Represents operational events that are passed within the system.
///
/// These signals provide real-time feedback from nodes to the central system,
/// allowing it to monitor health and make load-balancing decisions.
#[derive(Debug, Clone)]
pub enum SystemSignal {
    /// Sent when a node's task queue is nearing its capacity.
    NodeOverloaded {
        node_id: NodeId,
        queue_id: Option<QueueId>,
    },
    /// Sent when a previously overloaded node has cleared enough space in its queue.
    NodeIdle {
        node_id: NodeId,
        queue_id: Option<QueueId>,
    },
    /// Sent by a worker just before it begins executing a task.
    TaskDequeuedByWorker { node_id: NodeId, task_id: TaskId },
    /// Sent by a worker after it has finished executing a task.
    TaskProcessed {
        node_id: NodeId,
        task_id: TaskId,
        /// The total time it took to execute the task, in microseconds.
        duration_micros: u64,
    },
}

impl SystemSignal {
    /// A convenience method to get the `NodeId` from any signal type.
    pub fn get_node_id(&self) -> NodeId {
        match self {
            SystemSignal::NodeOverloaded { node_id, .. } => *node_id,
            SystemSignal::NodeIdle { node_id, .. } => *node_id,
            SystemSignal::TaskDequeuedByWorker { node_id, .. } => *node_id,
            SystemSignal::TaskProcessed { node_id, .. } => *node_id,
        }
    }
}
