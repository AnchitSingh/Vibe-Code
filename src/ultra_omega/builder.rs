// src/ultra_omega/builder.rs

use crate::node::OmegaNode;
use crate::signals::NodeId;
use crate::signals::SystemSignal;
use crate::ultra_omega::UltraOmegaSystem;
use std::sync::{mpsc, Arc};

// Default configuration values
const DEFAULT_NUM_NODES: usize = 8;
const DEFAULT_SUPER_NODES: usize = 2;

/// A builder for creating and configuring an `UltraOmegaSystem`.
///
/// This pattern provides a clean, readable, and extensible way to initialize
/// the system with various configurations.
#[derive(Default)]
pub struct UltraOmegaBuilder {
    num_nodes: Option<usize>,
    num_super_nodes: Option<usize>,
    // You can add more configuration options here later, e.g.:
    // queue_capacity_normal: Option<usize>,
    // queue_capacity_super: Option<usize>,
    // min_threads_normal: Option<usize>,
    // ...etc.
}

impl UltraOmegaBuilder {
    /// Creates a new builder with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the total number of `OmegaNode`s in the system.
    pub fn with_nodes(mut self, count: usize) -> Self {
        self.num_nodes = Some(count);
        self
    }

    /// Sets the number of "super nodes" within the total node count.
    /// Super nodes are configured with higher capacity and more threads.
    pub fn with_super_nodes(mut self, count: usize) -> Self {
        self.num_super_nodes = Some(count);
        self
    }

    /// Consumes the builder and constructs the `UltraOmegaSystem`.
    ///
    /// It uses provided configurations or falls back to sensible defaults.
    /// It will panic if the number of super nodes is greater than the total number of nodes.
    pub fn build(self) -> UltraOmegaSystem {
        let num_nodes = self.num_nodes.unwrap_or(DEFAULT_NUM_NODES);
        let num_super_nodes = self.num_super_nodes.unwrap_or(DEFAULT_SUPER_NODES);

        if num_super_nodes > num_nodes {
            panic!("Cannot have more super nodes than total nodes.");
        }

        // This is the same logic from main.rs, now encapsulated here.
        let (signal_tx, _) = mpsc::channel::<SystemSignal>(); // This receiver is unused for now
        let nodes_vec: Vec<Arc<OmegaNode>> = {
            let mut local_nodes_vec = Vec::new();
            for i in 0..num_nodes {
                // Determine configuration based on index
                let (queue_cap, min_thr, max_thr) = if i < num_super_nodes {
                    // Super Node configuration
                    (20, 2, 8)
                } else {
                    // Normal Node configuration
                    (10, 1, 4)
                };
                let node = Arc::new(
                    OmegaNode::new(
                        NodeId::new(),
                        queue_cap,
                        min_thr,
                        max_thr,
                        signal_tx.clone(),
                        None,
                    )
                    .unwrap(),
                );
                local_nodes_vec.push(node);
            }
            local_nodes_vec
        };

        // Construct the system using its internal `new` method.
        UltraOmegaSystem::new_internal(nodes_vec)
    }
}