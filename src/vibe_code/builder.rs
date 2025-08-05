//! Provides a builder for constructing and configuring the `UltraVibeSystem`.

use crate::node::VibeNode;
use crate::signals::{NodeId, SystemSignal};
use crate::vibe_code::UltraVibeSystem;
use std::sync::{Arc, mpsc};

/// The default number of normal-powered worker nodes.
const DEFAULT_NUM_NODES: usize = 80;
/// The default number of high-powered "super" worker nodes.
const DEFAULT_SUPER_NODES: usize = 40;

/// A builder for creating an `UltraVibeSystem`.
///
/// This provides a flexible way to initialize the system with a custom
/// number of normal and "super" worker nodes.

#[derive(Default)]
pub struct UltraVibeBuilder {
    /// The total number of worker nodes to create.
    num_nodes: Option<usize>,
    /// The number of "super" nodes (with more threads and larger queues).
    num_super_nodes: Option<usize>,
}

impl UltraVibeBuilder {
    /// Creates a new `UltraVibeBuilder` with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the total number of worker nodes for the system.
    pub fn with_nodes(mut self, count: usize) -> Self {
        self.num_nodes = Some(count);
        self
    }

    /// Sets the number of "super" nodes within the total node count.
    ///
    /// Super nodes are configured with higher task queue capacity and more
    /// worker threads, making them suitable for more intensive tasks.
    pub fn with_super_nodes(mut self, count: usize) -> Self {
        self.num_super_nodes = Some(count);
        self
    }

    /// Builds and returns a fully initialized `UltraVibeSystem`.
    ///
    /// This method uses the configured settings or falls back to defaults.
    /// It creates all the `VibeNode` instances and wires them into the system.
    ///
    /// # Panics
    /// Panics if the number of super nodes is greater than the total number of nodes.
    pub fn build(self) -> UltraVibeSystem {
        let num_nodes = self.num_nodes.unwrap_or(DEFAULT_NUM_NODES);
        let num_super_nodes = self.num_super_nodes.unwrap_or(DEFAULT_SUPER_NODES);

        if num_super_nodes > num_nodes {
            panic!("Cannot have more super nodes than total nodes.");
        }

        // This channel is for internal signals, but the receiving end is currently unused.
        let (signal_tx, _signal_rx) = mpsc::channel::<SystemSignal>();

        let nodes_vec: Vec<Arc<VibeNode>> = {
            let mut local_nodes_vec = Vec::with_capacity(num_nodes);
            for i in 0..num_nodes {
                // Super nodes get more resources.
                let (queue_cap, min_thr, max_thr) = if i < num_super_nodes {
                    (20, 2, 8) // Super Node config
                } else {
                    (10, 1, 4) // Normal Node config
                };

                let node = Arc::new(
                    VibeNode::new(
                        NodeId::new(),
                        queue_cap,
                        min_thr,
                        max_thr,
                        signal_tx.clone(),
                        None,
                    )
                    .expect("Failed to create VibeNode"),
                );
                local_nodes_vec.push(node);
            }
            local_nodes_vec
        };

        UltraVibeSystem::new_internal(nodes_vec)
    }
}
