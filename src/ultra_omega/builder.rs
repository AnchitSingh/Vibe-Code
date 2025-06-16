//! Provides a builder pattern for constructing the `UltraOmegaSystem`.
//!
//! This module defines the `UltraOmegaBuilder` struct, which allows for
//! configurable creation of an `UltraOmegaSystem` with specified numbers
//! of normal and super nodes.

use crate::node::OmegaNode;
use crate::signals::NodeId;
use crate::signals::SystemSignal;
use crate::ultra_omega::UltraOmegaSystem;
use std::sync::{mpsc, Arc};

/// Default number of normal `OmegaNode`s if not specified.
const DEFAULT_NUM_NODES: usize = 8;
/// Default number of "super nodes" if not specified.
/// Super nodes are configured with higher capacity and more threads.
const DEFAULT_SUPER_NODES: usize = 2;

/// A builder for creating and configuring an `UltraOmegaSystem`.
///
/// This builder provides a flexible and extensible way to initialize
/// the `UltraOmegaSystem` with custom parameters such as the total
/// number of nodes and the count of "super nodes".
///
/// # Examples
///
/// ```
/// use cpu_circulatory_system::ultra_omega::UltraOmegaSystem;
///
/// let system = UltraOmegaSystem::builder()
///     .with_nodes(10)
///     .with_super_nodes(3)
///     .build();
/// // The system is now initialized with 10 nodes, 3 of which are super nodes.
/// ```
#[derive(Default)]
pub struct UltraOmegaBuilder {
    /// Optional total number of `OmegaNode`s to create.
    num_nodes: Option<usize>,
    /// Optional number of "super nodes" to create.
    num_super_nodes: Option<usize>,
    // Future configuration options can be added here, e.g.:
    // queue_capacity_normal: Option<usize>,
    // queue_capacity_super: Option<usize>,
    // min_threads_normal: Option<usize>,
    // max_threads_normal: Option<usize>,
    // min_threads_super: Option<usize>,
    // max_threads_super: Option<usize>,
}

impl UltraOmegaBuilder {
    /// Creates a new `UltraOmegaBuilder` instance with default settings.
    ///
    /// All configuration options are initially `None`, indicating that
    /// default values will be used during the `build` process if no
    /// explicit values are set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the total number of `OmegaNode`s for the system.
    ///
    /// # Arguments
    ///
    /// * `count` - The desired total number of nodes.
    ///
    /// # Returns
    ///
    /// Returns the `UltraOmegaBuilder` instance for method chaining.
    pub fn with_nodes(mut self, count: usize) -> Self {
        self.num_nodes = Some(count);
        self
    }

    /// Sets the number of "super nodes" within the total node count.
    ///
    /// Super nodes are `OmegaNode`s configured with higher task queue capacity
    /// and a larger thread pool compared to normal nodes.
    ///
    /// # Arguments
    ///
    /// * `count` - The desired number of super nodes.
    ///
    /// # Returns
    ///
    /// Returns the `UltraOmegaBuilder` instance for method chaining.
    pub fn with_super_nodes(mut self, count: usize) -> Self {
        self.num_super_nodes = Some(count);
        self
    }

    /// Consumes the builder and constructs the `UltraOmegaSystem`.
    ///
    /// This method applies the configured settings or falls back to
    /// predefined default values for `num_nodes` and `num_super_nodes`.
    /// It initializes the `OmegaNode`s with appropriate capacities and
    /// thread counts based on whether they are normal or super nodes.
    ///
    /// # Panics
    ///
    /// Panics if the specified number of super nodes is greater than
    /// the total number of nodes.
    ///
    /// # Returns
    ///
    /// Returns a fully initialized `UltraOmegaSystem`.
    pub fn build(self) -> UltraOmegaSystem {
        let num_nodes = self.num_nodes.unwrap_or(DEFAULT_NUM_NODES);
        let num_super_nodes = self.num_super_nodes.unwrap_or(DEFAULT_SUPER_NODES);

        if num_super_nodes > num_nodes {
            panic!("Cannot have more super nodes than total nodes.");
        }

        // Create a channel for system-wide signals. The receiver is currently unused.
        let (signal_tx, _signal_rx) = mpsc::channel::<SystemSignal>();
        
        // Initialize the vector of OmegaNodes based on configuration.
        let nodes_vec: Vec<Arc<OmegaNode>> = {
            let mut local_nodes_vec = Vec::with_capacity(num_nodes);
            for i in 0..num_nodes {
                // Determine node configuration (queue capacity, min/max threads)
                // based on whether it's a super node or a normal node.
                let (queue_cap, min_thr, max_thr) = if i < num_super_nodes {
                    // Super Node configuration: higher capacity and more threads.
                    (20, 2, 8)
                } else {
                    // Normal Node configuration: standard capacity and threads.
                    (10, 1, 4)
                };
                
                // Create and store the OmegaNode.
                let node = Arc::new(
                    OmegaNode::new(
                        NodeId::new(),
                        queue_cap,
                        min_thr,
                        max_thr,
                        signal_tx.clone(),
                        None, // No specific event handler for now.
                    )
                    .expect("Failed to create OmegaNode"),
                );
                local_nodes_vec.push(node);
            }
            local_nodes_vec
        };

        // Construct the UltraOmegaSystem using its internal constructor.
        UltraOmegaSystem::new_internal(nodes_vec)
    }
}