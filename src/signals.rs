// src/signals.rs

use crate::task::TaskId;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

// --- NodeId ---
/// A unique identifier for an OmegaNode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub usize); // Made field public for direct access if needed, e.g. in system_pulse

static NEXT_NODE_ID: AtomicUsize = AtomicUsize::new(1);

impl NodeId {
    pub fn new() -> Self {
        NodeId(NEXT_NODE_ID.fetch_add(1, Ordering::Relaxed))
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({})", self.0)
    }
}

// --- QueueId ---
/// A unique identifier for an OmegaQueue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QueueId(usize); // Made field public for direct access if needed

static NEXT_QUEUE_ID: AtomicUsize = AtomicUsize::new(1);

impl QueueId {
    pub fn new() -> Self {
        QueueId(NEXT_QUEUE_ID.fetch_add(1, Ordering::Relaxed))
    }
}

impl fmt::Display for QueueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Queue({})", self.0)
    }
}
// --- SystemSignal ---
/// Signals exchanged within the Ultra-Ω system, carrying operational status.
#[derive(Debug, Clone)]
pub enum SystemSignal {
    /// Emitted when a node's primary queue is considered overloaded.
    NodeOverloaded {
        node_id: NodeId,
        queue_id: Option<QueueId>, // Optional: identifies which queue if a node has multiple
    },
    /// Emitted when a previously overloaded node's queue has drained to a safe level.
    NodeIdle {
        node_id: NodeId,
        queue_id: Option<QueueId>, // Optional
    },
    /// Emitted when a task execution fails (panic or returned error).
    TaskFailed {
        node_id: NodeId,
        task_id: TaskId,
        // No opportunistic info
    },
    /// Emitted when a task completes successfully.
    TaskCompleted {
        node_id: NodeId,
        task_id: TaskId,
        duration_micros: u64, // Actual duration of this specific task
        // No opportunistic info
    },
    // Consider adding a NodeShutdown signal if SystemPulse needs to know explicitly
    // when a node is gone to refine its "all_nodes_seen" count to "live_nodes".
    // For now, SystemPulse relies on channel disconnect or lack of signals.
}

// Helper methods to get common fields if needed, adapted for no opportunistic info
// These might be less necessary now but can be kept if they simplify SystemPulse.
impl SystemSignal {
    pub fn get_node_id(&self) -> NodeId {
        match self {
            SystemSignal::NodeOverloaded { node_id, .. } => *node_id,
            SystemSignal::NodeIdle { node_id, .. } => *node_id,
            SystemSignal::TaskFailed { node_id, .. } => *node_id,
            SystemSignal::TaskCompleted { node_id, .. } => *node_id,
        }
    }
}
