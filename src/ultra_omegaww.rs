// src/main.rs

use crate::node::OmegaNode;
use crate::task::TaskId;
// use system_pulse::OmegaSystemPulse;
// TaskHandle and TaskError are now needed by the client (main)
use crate::task::{Priority, Task, TaskError, TaskHandle};
use crate::types::NodeError;
use std::thread::{JoinHandle,Builder};
use omega::borrg::{BiasStrategy, OmegaRng};
use std::sync::mpsc;
use std::sync::{
    atomic::AtomicUsize,
    Arc
};
use crate::io::reactor::{GlobalReactor, ReactorCommand};
use crate::io::{IoOp, IoOutput};
use std::sync::mpsc::Sender;
use std::io as std_io;
pub struct SharedSubmitterStats {
    pub tasks_attempted_submission: AtomicUsize,
    pub tasks_successfully_submitted: AtomicUsize,
    pub tasks_failed_submission_max_retries: AtomicUsize,
    pub system_maxed_out_events_router: AtomicUsize,
}

impl SharedSubmitterStats {
    pub fn new() -> Self {
        SharedSubmitterStats {
            tasks_attempted_submission: AtomicUsize::new(0),
            tasks_successfully_submitted: AtomicUsize::new(0),
            tasks_failed_submission_max_retries: AtomicUsize::new(0),
            system_maxed_out_events_router: AtomicUsize::new(0),
        }
    }
}
pub struct UltraOmegaSystem {
    pub nodes: Arc<Vec<Arc<OmegaNode>>>,
    reactor_thread: Option<JoinHandle<()>>,
    reactor_cmd_tx: Sender<ReactorCommand>,
}


impl UltraOmegaSystem {
    /// Creates a new UltraOmegaSystem with the specified number of nodes.
    pub fn new(nodes: Vec<Arc<OmegaNode>>) -> Self {
        let (reactor_cmd_tx, reactor_cmd_rx) = mpsc::channel();
        
        let reactor = GlobalReactor::new(reactor_cmd_rx)
            .expect("Failed to initialize Global Reactor");
            
        let reactor_thread = Builder::new()
            .name("global-io-reactor".to_string())
            .spawn(move || reactor.run())
            .expect("Failed to spawn Global Reactor thread");

        Self {
            nodes: Arc::new(nodes),
            reactor_thread: Some(reactor_thread),
            reactor_cmd_tx,
        }
    }

    // CORRECTED: Method now accepts a mutable reference to an RNG.
    pub fn submit_cpu_task<F, Output>(
        &self,
        priority: Priority,
        estimated_cost: u32,
        work_fn: F,
        rng: &mut OmegaRng, // Pass in the RNG
    ) -> Result<TaskHandle<Output>, NodeError>
    where
        F: FnOnce() -> Result<Output, TaskError> + Send + 'static,
        Output: Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let task = Task::new_for_cpu(priority, estimated_cost, work_fn, tx);
        let task_id = task.id;

        // Pass the RNG down to the routing method.
        self.route_task(task, rng)?;

        Ok(TaskHandle::new(task_id, rx))
    }

    /// Submits a non-blocking I/O operation to the system.
    ///
    /// This method immediately returns a `TaskHandle` that will receive the result
    /// of the I/O operation from the Global Reactor. It works by submitting a
    /// lightweight "launcher" task to an OmegaNode worker, whose only job is to
    /// delegate the real I/O work to the reactor.
    pub fn submit_io_task(
        &self,
        io_op: IoOp,
        rng: &mut OmegaRng,
    ) -> Result<TaskHandle<IoOutput>, NodeError> {
        // 1. Create the result channel and TaskHandle that will be returned to the client.
        // The `tx` (sender) part will be given to the Reactor, and the `rx` (receiver)
        // part will be stored in the handle for the client.
        let (tx, rx) = mpsc::channel();
        let task_id = TaskId::new(); // A unique ID for this I/O operation.

        // 2. Create the command that the launcher task will send to the Reactor.
        // This command bundles the I/O operation details and the result sender.
        let command = ReactorCommand::SubmitIoOp {
            op: io_op,
            task_id,
            result_tx: tx,
        };

        // 3. Create the simple, fire-and-forget "launcher" work function.
        // Its only job is to send the command to the reactor.
        let reactor_tx_clone = self.reactor_cmd_tx.clone();
        let launcher_work_fn = move || -> Result<(), TaskError> {
            reactor_tx_clone.send(command).map_err(|e| {
                // If this send fails, the Reactor is dead. This is a critical system error.
                TaskError::ExecutionFailed(Box::new(std_io::Error::new(
                    std_io::ErrorKind::BrokenPipe,
                    format!("Failed to send command to I/O reactor: {}", e),
                )))
            })?;
            // The launcher's "work" is done. Return a simple Ok.
            Ok(())
        };

        // 4. Submit the launcher function as a high-priority, low-cost CPU task.
        // We use a dummy channel for the launcher's own result, as we don't care about it.
        let (dummy_tx, _) = mpsc::channel();
        let launcher_task = Task::new_for_cpu(Priority::High, 1, launcher_work_fn, dummy_tx);

        // Route this lightweight launcher task to an OmegaNode.
        self.route_task(launcher_task, rng)?;

        // 5. Immediately return the handle to the client. The client will use this handle
        // to await the result from the Reactor, not from the launcher task.
        Ok(TaskHandle::new(task_id, rx))
    }


    /// Internal routing logic (Power of K Choices).
    fn route_task(&self, task: Task, rng: &mut OmegaRng) -> Result<(), NodeError> {
        let n_total_nodes = self.nodes.len();
        if n_total_nodes == 0 {
            return Err(NodeError::NoNodesAvailable);
        }
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
                let (mut prev, mut boundary) = (u64::MAX, (n_total_nodes - k) as u64);

                for _ in 0..k {
                    let mut next;
                    if _try_num > 2 {
                        next = rng.range_biased(
                            prev.wrapping_add(1),
                            boundary,
                            BiasStrategy::Power(0.05),
                        );
                    } else {
                        next = rng.range_biased(
                            prev.wrapping_add(1),
                            boundary,
                            BiasStrategy::Power(3.14),
                        );
                        if 4 * next >= boundary as u64 {
                            // Weaker bias against upper 1/4
                            next = rng.range_biased(
                                prev.wrapping_add(1),
                                boundary,
                                BiasStrategy::Power(3.14),
                            );
                        }
                        if 4 * next >= boundary as u64 {
                            // Weaker bias against upper 1/4
                            next = rng.range_biased(
                                prev.wrapping_add(1),
                                boundary,
                                BiasStrategy::Stepped,
                            );
                        }
                        if 4 * next >= boundary as u64 {
                            // Weaker bias against upper 1/4
                            next = rng.range_biased(
                                prev.wrapping_add(1),
                                boundary,
                                BiasStrategy::Weighted,
                            );
                        }
                        if 4 * next >= boundary as u64 {
                            // Weaker bias against upper 1/4
                            next = rng.range_biased(
                                prev.wrapping_add(1),
                                boundary,
                                BiasStrategy::Exponential,
                            );
                        }
                    }
                    chosen_indices.push(next as usize);
                    prev = next;
                    boundary += 1;
                }
            }

            let mut best_node_index: Option<usize> = None;
            let mut min_pressure_found = usize::MAX;

            for &node_idx in &chosen_indices {
                let node = &self.nodes[node_idx];
                let pressure = node.get_pressure();

                if pressure < node.max_pressure() {
                    if pressure < min_pressure_found {
                        min_pressure_found = pressure;
                        best_node_index = Some(node_idx);
                    }
                }
            }

            if let Some(idx) = best_node_index {
                return self.nodes[idx].submit_task(task);
            } else {
                for &node_idx in &chosen_indices {
                    let node = &self.nodes[n_total_nodes-node_idx-1];
                    let pressure = node.get_pressure();
                    let max_pressure = node.max_pressure();
        
                    if pressure < max_pressure {
                        if pressure < min_pressure_found {
                            min_pressure_found = pressure;
                            best_node_index = Some(node_idx);
                        }
                    }
                }
                if let Some(idx) = best_node_index {
                    return self.nodes[idx].submit_task(task);
                }else{
                    continue;
                }
            }
        }
        Err(NodeError::SystemMaxedOut)
    }

    /// Shuts down all nodes in the system.
    pub fn shutdown_all(&self) {
        for node in self.nodes.iter() {
            node.shutdown();
        }
    }
}
