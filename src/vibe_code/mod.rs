//! The `vibe_code` module provides the core `UltraOmegaSystem` for managing
//! and executing tasks across a network of `VibeNode`s.
//!
//! This system integrates CPU-bound task processing with an asynchronous
//! I/O reactor, offering a robust and scalable solution for concurrent
//! operations. It includes a builder pattern for flexible system configuration
//! and implements a task routing strategy to distribute work efficiently.

// --- Submodules ---
pub mod builder; // Re-exported for public use.

// --- Public Re-exports ---
pub use builder::UltraOmegaBuilder;

// --- Internal Imports ---
use crate::node::VibeNode;
use crate::task::{Priority, Task, TaskError, TaskHandle, TaskId};
use crate::types::NodeError;
use crate::utils::{BiasStrategy, VibeRng};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, atomic::AtomicUsize};
use std::thread::{Builder, JoinHandle};
use std::time::Duration;

/// Statistics related to task submission within the `UltraOmegaSystem`.
///
/// This struct provides atomic counters to track various outcomes of task
/// submission attempts, useful for monitoring system performance and identifying
/// bottlenecks.
pub struct SharedSubmitterStats {
    /// The total number of tasks for which submission was attempted.
    pub tasks_attempted_submission: AtomicUsize,
    /// The number of tasks that were successfully submitted to an `VibeNode`.
    pub tasks_successfully_submitted: AtomicUsize,
    /// The number of tasks that failed submission after reaching the maximum
    /// number of retries, typically due to all chosen nodes being overloaded.
    pub tasks_failed_submission_max_retries: AtomicUsize,
    /// The number of times the system's internal event router (e.g., for I/O)
    /// was maxed out, indicating potential backpressure or congestion.
    pub system_maxed_out_events_router: AtomicUsize,
}

impl SharedSubmitterStats {
    /// Creates a new `SharedSubmitterStats` instance with all counters initialized to zero.
    pub fn new() -> Self {
        SharedSubmitterStats {
            tasks_attempted_submission: AtomicUsize::new(0),
            tasks_successfully_submitted: AtomicUsize::new(0),
            tasks_failed_submission_max_retries: AtomicUsize::new(0),
            system_maxed_out_events_router: AtomicUsize::new(0),
        }
    }
}

impl Default for SharedSubmitterStats {
    /// Provides a default `SharedSubmitterStats` instance using `SharedSubmitterStats::new()`.
    fn default() -> Self {
        Self::new()
    }
}

/// The main entry point for the Vibe System, managing `VibeNode`s
/// and an asynchronous I/O reactor.
///
/// `UltraOmegaSystem` is responsible for:
/// - Holding and managing a collection of `VibeNode`s.
/// - Spawning and managing a `GlobalReactor` thread for asynchronous I/O.
/// - Providing methods for submitting CPU-bound and I/O-bound tasks.
/// - Implementing a task routing strategy to distribute tasks among nodes.
/// - Ensuring proper shutdown of all managed components.
pub struct UltraOmegaSystem {
    /// A shared reference to the vector of `VibeNode`s managed by the system.
    pub nodes: Arc<Vec<Arc<VibeNode>>>,
}

impl UltraOmegaSystem {
    /// Returns a new `UltraOmegaBuilder` for configuring and creating an `UltraOmegaSystem`.
    ///
    /// This is the recommended public entry point for constructing an `UltraOmegaSystem`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cpu_circulatory_system::vibe_code::UltraOmegaSystem;
    ///
    /// let system = UltraOmegaSystem::builder()
    ///     .with_nodes(16)
    ///     .with_super_nodes(4)
    ///     .build();
    /// ```
    pub fn builder() -> UltraOmegaBuilder {
        UltraOmegaBuilder::new()
    }

    /// Creates a new `UltraOmegaSystem` instance with a pre-configured set of nodes.
    ///
    /// This is an internal constructor, primarily intended to be called by the
    /// `UltraOmegaBuilder` after node configuration. It initializes the
    /// `GlobalReactor` and spawns its dedicated thread.
    ///
    /// # Arguments
    ///
    /// * `nodes` - A `Vec` of `Arc<VibeNode>` representing the nodes to be managed by the system.
    ///
    /// # Panics
    ///
    /// Panics if the `GlobalReactor` fails to initialize or its thread fails to spawn.
    pub(crate) fn new_internal(nodes: Vec<Arc<VibeNode>>) -> Self {
        Self {
            nodes: Arc::new(nodes),
        }
    }

    /// Submits a CPU-bound task to the system for execution on an `VibeNode`.
    ///
    /// The task will be routed to an available node based on the system's
    /// internal routing strategy. A `TaskHandle` is returned immediately,
    /// which can be used to await the task's completion and retrieve its result.
    ///
    /// # Arguments
    ///
    /// * `priority` - The priority of the task (e.g., `Priority::High`, `Priority::Normal`).
    /// * `estimated_cost` - An estimated computational cost of the task, used for scheduling.
    /// * `work_fn` - The closure containing the CPU-bound work to be executed.
    /// * `rng` - A mutable reference to an `VibeRng` instance for task routing.
    ///
    /// # Type Parameters
    ///
    /// * `F` - The type of the work function, which must be `FnOnce`, `Send`, and `'static`.
    /// * `Output` - The return type of the work function, which must be `Send` and `'static`.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `TaskHandle<Output>` on success, or a `NodeError` if
    /// the task could not be submitted (e.g., no nodes available, system maxed out).
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

    /// Internal routing logic for submitting a task to an `VibeNode` using a
    /// "Power of K Choices" strategy.
    ///
    /// This method attempts to find the least pressured node among a randomly
    /// chosen subset of `k` nodes. If no suitable node is found within a few
    /// tries, it may fall back to a reverse search or ultimately return an error.
    ///
    /// # Arguments
    ///
    /// * `task` - The `Task` to be routed.
    /// * `rng` - A mutable reference to an `VibeRng` instance for random node selection.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success (`Ok(())`) if the task was submitted, or a
    /// `NodeError` if no node could accept the task (e.g., `NodeError::NoNodesAvailable`,
    /// `NodeError::SystemMaxedOut`).
    fn route_task(&self, task: Task, rng: &mut VibeRng) -> Result<(), NodeError> {
        let n_total_nodes = self.nodes.len();
        if n_total_nodes == 0 {
            return Err(NodeError::NoNodesAvailable);
        }

        // Determine 'k' for "Power of K Choices" strategy.
        // k is at least 1, and at most log2(n_total_nodes) floored.
        let k = match n_total_nodes {
            1 => 1,
            _ => (2.0f64).max((n_total_nodes as f64).log2().floor()).floor() as usize,
        };

        const MAX_ROUTING_TRIES: usize = 5;

        for _try_num in 0..MAX_ROUTING_TRIES {
            let mut chosen_indices = Vec::with_capacity(k);
            if n_total_nodes <= k {
                // If total nodes are less than or equal to k, consider all nodes.
                chosen_indices.extend(0..n_total_nodes);
            } else {
                // Select k random, distinct indices using a biased random number generator.
                let (mut prev, mut boundary) = (u64::MAX, (n_total_nodes - k) as u64);

                for _ in 0..k {
                    let next = if _try_num > 2 {
                        rng.range_biased(prev.wrapping_add(1), boundary, BiasStrategy::Power(0.05))
                    } else {
                        // This block will determine the 'next' value using a sequence of biased attempts.
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
                        temp_next // Return the final value from this block
                    };
                    chosen_indices.push(next as usize);
                    prev = next;
                    boundary += 1;
                }
            }

            let mut best_node_index: Option<usize> = None;
            let mut min_pressure_found = usize::MAX;

            // Iterate through chosen nodes to find the one with the minimum pressure
            // that can accept the task.
            for &node_idx in &chosen_indices {
                let node = &self.nodes[node_idx];
                let pressure = node.get_pressure();

                if (pressure < node.max_pressure()) && pressure < min_pressure_found {
                    min_pressure_found = pressure;
                    best_node_index = Some(node_idx);
                }
            }

            // If a suitable node is found, submit the task to it.
            if let Some(idx) = best_node_index {
                return self.nodes[idx].submit_task(task);
            } else {
                // If no suitable node found in the initial selection,
                // try a reverse search on the chosen indices. This is a heuristic
                // to potentially find a less pressured node if the initial forward
                // search didn't yield one.
                for &node_idx in &chosen_indices {
                    let node = &self.nodes[n_total_nodes - node_idx - 1]; // Reverse index
                    let pressure = node.get_pressure();
                    let max_pressure = node.max_pressure();

                    if (pressure < max_pressure) && pressure < min_pressure_found {
                        min_pressure_found = pressure;
                        best_node_index = Some(node_idx); // Store original index
                    }
                }
                if let Some(idx) = best_node_index {
                    return self.nodes[idx].submit_task(task);
                } else {
                    // If still no node found, continue to the next routing try.
                    continue;
                }
            }
        }
        // If all routing attempts fail, the system is considered maxed out.
        Err(NodeError::SystemMaxedOut)
    }

    /// Shuts down all `VibeNode`s and the `GlobalReactor` in the system.
    ///
    /// This method sends shutdown signals to all components, ensuring a graceful
    /// termination of all worker threads and the I/O reactor.
    pub fn shutdown_all(&self) {
        // Shutdown VibeNodes.
        for node in self.nodes.iter() {
            node.shutdown();
        }
    }
}

/// Implements the `Drop` trait for `UltraOmegaSystem` to ensure proper shutdown
/// when the system goes out of scope.
///
/// This ensures that the `GlobalReactor` thread is joined and all `VibeNode`s
/// are shut down, preventing resource leaks and ensuring a clean exit.
impl Drop for UltraOmegaSystem {
    fn drop(&mut self) {
        // Shutdown VibeNodes.
        for node in self.nodes.iter() {
            node.shutdown();
        }
    }
}
