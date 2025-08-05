//! Provides `UltraVibeSystem`, the core task scheduling and execution engine.
//!
//! This system manages a collection of `VibeNode`s and is responsible for
//! intelligently routing incoming tasks to the least busy nodes. It uses a
//! "Power of K Choices" algorithm for efficient, decentralized load balancing.

pub mod builder;
pub use builder::UltraVibeBuilder;

use crate::node::VibeNode;
use crate::task::{Priority, Task, TaskError, TaskHandle};
use crate::types::NodeError;
use crate::utils::{BiasStrategy, VibeRng};
use std::sync::{Arc, mpsc};

/// Tracks statistics related to task submission across the entire system.
pub struct SharedSubmitterStats {
    /// Total number of tasks for which submission was attempted.
    pub tasks_attempted_submission: std::sync::atomic::AtomicUsize,
    /// Number of tasks successfully submitted to a node.
    pub tasks_successfully_submitted: std::sync::atomic::AtomicUsize,
    /// Number of tasks that failed submission because all nodes were busy.
    pub tasks_failed_submission_max_retries: std::sync::atomic::AtomicUsize,
    /// Number of times the system's internal router was maxed out.
    pub system_maxed_out_events_router: std::sync::atomic::AtomicUsize,
}

impl SharedSubmitterStats {
    /// Creates a new `SharedSubmitterStats` instance with all counters at zero.
    pub fn new() -> Self {
        use std::sync::atomic::AtomicUsize;
        SharedSubmitterStats {
            tasks_attempted_submission: AtomicUsize::new(0),
            tasks_successfully_submitted: AtomicUsize::new(0),
            tasks_failed_submission_max_retries: AtomicUsize::new(0),
            system_maxed_out_events_router: AtomicUsize::new(0),
        }
    }
}

impl Default for SharedSubmitterStats {
    fn default() -> Self {
        Self::new()
    }
}

/// The central system for managing nodes and executing tasks.
///
/// `UltraVibeSystem` holds the pool of `VibeNode`s and provides the main
/// entry point for submitting and routing CPU-bound tasks.
pub struct UltraVibeSystem {
    /// The collection of worker nodes managed by the system.
    pub nodes: Arc<Vec<Arc<VibeNode>>>,
}

impl UltraVibeSystem {
    /// Returns a new `UltraVibeBuilder` for creating a system.
    ///
    /// This is the standard way to construct an `UltraVibeSystem`.
    pub fn builder() -> UltraVibeBuilder {
        UltraVibeBuilder::new()
    }

    /// Internal constructor called by the `UltraVibeBuilder`.
    pub(crate) fn new_internal(nodes: Vec<Arc<VibeNode>>) -> Self {
        Self {
            nodes: Arc::new(nodes),
        }
    }

    /// Submits a CPU-bound task to the system for execution.
    ///
    /// The task is routed to the most suitable node, and a `TaskHandle` is
    /// returned immediately for later result retrieval.
    pub fn submit_cpu_task<F, Output>(
        &self,
        priority: Priority,
        estimated_cost: u32,
        work_fn: F,
        rng: &mut VibeRng,
    ) -> Result<TaskHandle<Output>, NodeError>
    where
        F: FnOnce() -> Result<Output, TaskError> + Send + 'static,
        Output: Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let task = Task::new_for_cpu(priority, estimated_cost, work_fn, tx);
        let task_id = task.id;

        self.route_task(task, rng)?;

        Ok(TaskHandle::new(task_id, rx))
    }

    /// Routes a task to the best available node using "Power of K Choices".
    ///
    /// This strategy involves picking `k` random nodes and sending the task to
    /// the one with the lowest "pressure" (i.e., the shortest queue). This
    /// provides excellent load balancing without a central bottleneck.
    fn route_task(&self, task: Task, rng: &mut VibeRng) -> Result<(), NodeError> {
        let n_total_nodes = self.nodes.len();
        if n_total_nodes == 0 {
            return Err(NodeError::NoNodesAvailable);
        }

        // `k` is the number of nodes to sample. A larger `k` gives better load
        // balancing at the cost of more checking. log2(n) is a good compromise.
        let k = match n_total_nodes {
            1 => 1,
            _ => (2.0f64).max((n_total_nodes as f64).log2().floor()).floor() as usize,
        };

        const MAX_ROUTING_TRIES: usize = 5;

        for _try_num in 0..MAX_ROUTING_TRIES {
            let mut chosen_indices = Vec::with_capacity(k);
            if n_total_nodes <= k {
                chosen_indices.extend(0..n_total_nodes);
            } else {
                // Select `k` random, distinct node indices.
                let (mut prev, mut boundary) = (u64::MAX, (n_total_nodes - k) as u64);

                for _ in 0..k {
                    let next = if _try_num > 2 {
                        rng.range_biased(prev.wrapping_add(1), boundary, BiasStrategy::Power(0.05))
                    } else {
                        let mut temp_next = rng.range_biased(
                            prev.wrapping_add(1),
                            boundary,
                            BiasStrategy::Power(std::f64::consts::PI),
                        );

                        if 4 * temp_next >= boundary {
                            temp_next = rng.range_biased(
                                prev.wrapping_add(1),
                                boundary,
                                BiasStrategy::Power(std::f64::consts::PI),
                            );
                        }
                        if 4 * temp_next >= boundary {
                            temp_next = rng.range_biased(
                                prev.wrapping_add(1),
                                boundary,
                                BiasStrategy::Stepped,
                            );
                        }
                        if 4 * temp_next >= boundary {
                            temp_next = rng.range_biased(
                                prev.wrapping_add(1),
                                boundary,
                                BiasStrategy::Weighted,
                            );
                        }
                        if 4 * temp_next >= boundary {
                            temp_next = rng.range_biased(
                                prev.wrapping_add(1),
                                boundary,
                                BiasStrategy::Exponential,
                            );
                        }
                        temp_next
                    };
                    chosen_indices.push(next as usize);
                    prev = next;
                    boundary += 1;
                }
            }

            let mut best_node_index: Option<usize> = None;
            let mut min_pressure_found = usize::MAX;

            // Find the node with the lowest pressure among the chosen candidates.
            for &node_idx in &chosen_indices {
                let node = &self.nodes[node_idx];
                let pressure = node.get_pressure();

                if (pressure < node.max_pressure()) && pressure < min_pressure_found {
                    min_pressure_found = pressure;
                    best_node_index = Some(node_idx);
                }
            }

            if let Some(idx) = best_node_index {
                return self.nodes[idx].submit_task(task);
            } else {
                // If no suitable node was found, try a reverse search as a fallback.
                for &node_idx in &chosen_indices {
                    let node = &self.nodes[n_total_nodes - node_idx - 1];
                    let pressure = node.get_pressure();
                    let max_pressure = node.max_pressure();

                    if (pressure < max_pressure) && pressure < min_pressure_found {
                        min_pressure_found = pressure;
                        best_node_index = Some(node_idx);
                    }
                }
                if let Some(idx) = best_node_index {
                    return self.nodes[idx].submit_task(task);
                } else {
                    continue;
                }
            }
        }
        Err(NodeError::SystemMaxedOut)
    }

    /// Shuts down all `VibeNode`s in the system gracefully.
    pub fn shutdown_all(&self) {
        for node in self.nodes.iter() {
            node.shutdown();
        }
    }
}

impl Drop for UltraVibeSystem {
    /// Ensures the system is properly shut down when it goes out of scope.
    fn drop(&mut self) {
        for node in self.nodes.iter() {
            node.shutdown();
        }
    }
}
