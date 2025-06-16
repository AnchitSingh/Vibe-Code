//! Defines various identifiers and system-wide signals used for inter-component communication
//! within the CPU Circulatory System.
//!
//! This module provides unique ID types for nodes and queues, and an enumeration
//! of `SystemSignal`s that convey important operational events across the system.

use crate::task::TaskId;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

// --- NodeId ---
/// A unique, monotonic identifier for an `OmegaNode` instance.
///
/// Used to distinguish individual nodes within the system for logging,
/// monitoring, and routing purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub usize);

/// Atomic counter for generating the next unique `NodeId`.
static NEXT_NODE_ID: AtomicUsize = AtomicUsize::new(1);

impl NodeId {
    /// Creates a new, unique `NodeId`.
    ///
    /// Each call to this function increments an internal atomic counter
    /// to ensure uniqueness.
    pub fn new() -> Self {
        NodeId(NEXT_NODE_ID.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for NodeId {
    /// Provides a default `NodeId` by calling `NodeId::new()`.
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for NodeId {
    /// Implements the `Display` trait for `NodeId`, formatting it as "Node(ID)".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({})", self.0)
    }
}

// --- QueueId ---
/// A unique, monotonic identifier for a task queue within an `OmegaNode`.
///
/// Used to distinguish different queues (e.g., priority queues) within a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QueueId(usize);

/// Atomic counter for generating the next unique `QueueId`.
static NEXT_QUEUE_ID: AtomicUsize = AtomicUsize::new(1);

impl QueueId {
    /// Creates a new, unique `QueueId`.
    ///
    /// Each call to this function increments an internal atomic counter
    /// to ensure uniqueness.
    pub fn new() -> Self {
        QueueId(NEXT_QUEUE_ID.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for QueueId {
    /// Provides a default `QueueId` by calling `QueueId::new()`.
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for QueueId {
    /// Implements the `Display` trait for `QueueId`, formatting it as "Queue(ID)".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Queue({})", self.0)
    }
}

// --- SystemSignal ---
/// Signals exchanged within the Ultra-Ω system, focusing on operational lifecycle
/// and state changes of `OmegaNode`s and tasks.
///
/// These signals are typically sent from `OmegaNode`s to a central system
/// component (e.g., a supervisor or load balancer) to provide real-time
/// operational feedback.
#[derive(Debug, Clone)]
pub enum SystemSignal {
    /// Emitted when a node's primary task queue is considered overloaded,
    /// indicating high pressure or nearing capacity.
    NodeOverloaded {
        /// The ID of the overloaded node.
        node_id: NodeId,
        /// The ID of the specific queue that is overloaded, if applicable.
        queue_id: Option<QueueId>,
    },
    /// Emitted when a previously overloaded node's queue has drained to a safe level,
    /// indicating it is ready to accept more tasks.
    NodeIdle {
        /// The ID of the node that has become idle.
        node_id: NodeId,
        /// The ID of the specific queue that has become idle, if applicable.
        queue_id: Option<QueueId>,
    },
    /// Emitted by a worker thread just before it starts processing a task.
    TaskDequeuedByWorker {
        /// The ID of the node where the task was dequeued.
        node_id: NodeId,
        /// The ID of the task that was dequeued.
        task_id: TaskId,
    },
    /// Emitted by a worker thread after it has finished processing a task,
    /// regardless of the task's logical success or failure.
    TaskProcessed {
        /// The ID of the node where the task was processed.
        node_id: NodeId,
        /// The ID of the task that was processed.
        task_id: TaskId,
        /// The duration the task took to process, in microseconds.
        duration_micros: u64,
    },
}

impl SystemSignal {
    /// Returns the `NodeId` associated with the `SystemSignal`.
    ///
    /// This is a convenience method to extract the originating node's ID
    /// from any system signal.
    pub fn get_node_id(&self) -> NodeId {
        match self {
            SystemSignal::NodeOverloaded { node_id, .. } => *node_id,
            SystemSignal::NodeIdle { node_id, .. } => *node_id,
            SystemSignal::TaskDequeuedByWorker { node_id, .. } => *node_id,
            SystemSignal::TaskProcessed { node_id, .. } => *node_id,
        }
    }
}