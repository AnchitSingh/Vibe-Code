// src/main.rs

mod monitor;
mod node;
mod queue;
mod signals;
// mod system_pulse;
mod task;
mod types;

use node::OmegaNode;
use signals::NodeId;
// use system_pulse::OmegaSystemPulse;
// TaskHandle and TaskError are now needed by the client (main)
use task::{Priority, Task, TaskError, TaskHandle};
use types::NodeError;

use monitor::Monitor;
use omega::borrg::{BiasStrategy, OmegaRng};
use omega::omega_timer::{omega_time_ns, omega_timer_init};
use std::io::{stdout, Write};
use std::sync::mpsc;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

// --- Test Scenario Selection & Configuration (remains the same) ---
#[derive(Debug, Clone, Copy)]
enum TestScenario {
    Baseline,
    ExtremeDurations,
    FailingTasks,
    SustainedLoad,
    HeterogeneousNodes,
}
const CURRENT_SCENARIO: TestScenario = TestScenario::HeterogeneousNodes;

const NUM_NODES_CONF: usize = 16;
const NUM_SUPER_NODES_CONF: usize = 4;
const NUM_SUBMITTER_THREADS_CONF: usize = 8;
const TOTAL_TASKS_PER_SUBMITTER_CONF: usize = 100;
const AVG_TASK_PROCESSING_MS_CONF: u64 = 75;
const TASK_PROCESSING_VARIABILITY_MS_CONF: u64 = 50;
const SUBMISSION_DELAY_MS_CONF: u64 = 5;
const SUBMISSION_DELAY_VARIABILITY_MS_CONF: u64 = 5;
const SYSTEM_MAXED_OUT_BACKOFF_MS_CONF: u64 = 250;
const MAX_SUBMISSION_RETRIES_CONF: usize = 10;
const MONITORING_INTERVAL_MS_CONF: u64 = 1000;
const POST_SUBMISSION_STABILIZATION_S_CONF: u64 = 20;
const TASK_FAILURE_PROBABILITY: f64 = 0.05;

// --- SharedSubmitterStats (remains the same) ---
pub struct SharedSubmitterStats {
    tasks_attempted_submission: AtomicUsize,
    tasks_successfully_submitted: AtomicUsize,
    tasks_failed_submission_max_retries: AtomicUsize,
    system_maxed_out_events_router: AtomicUsize,
}

impl SharedSubmitterStats {
    fn new() -> Self {
        SharedSubmitterStats {
            tasks_attempted_submission: AtomicUsize::new(0),
            tasks_successfully_submitted: AtomicUsize::new(0),
            tasks_failed_submission_max_retries: AtomicUsize::new(0),
            system_maxed_out_events_router: AtomicUsize::new(0),
        }
    }
}


// --- CORRECTED: UltraOmegaSystem ---
// The system no longer owns a global, shared RNG.
pub struct UltraOmegaSystem {
    nodes: Arc<Vec<Arc<OmegaNode>>>,
}


impl UltraOmegaSystem {
    /// Creates a new UltraOmegaSystem with the specified number of nodes.
    pub fn new(nodes: Vec<Arc<OmegaNode>>) -> Self {
        Self {
            nodes: Arc::new(nodes),
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

fn main() {
    omega_timer_init();
    print!("\x1B[2J\x1B[H");
    stdout().flush().unwrap();
    
    const NANOS_PER_MSEC: u64 = 1_000_000;
    const NANOS_PER_SEC: u64 = 1_000_000_000;

    let overall_start_time_ns = omega_time_ns();
    let (signal_tx, signal_rx) = mpsc::channel();
    let shared_stats = Arc::new(SharedSubmitterStats::new());

    let nodes_vec: Vec<Arc<OmegaNode>> = {
        let mut local_nodes_vec = Vec::new();
        let mut node_rng = OmegaRng::new(0x9e79b97f469c15);
        for i in 0..NUM_NODES_CONF {
            let node_id_val = NodeId::new();
            let (queue_cap, min_thr, max_thr, cooldown_ms);
            if matches!(CURRENT_SCENARIO, TestScenario::HeterogeneousNodes) {
                if i < NUM_SUPER_NODES_CONF {
                    queue_cap = node_rng.range(20, 30); min_thr = 2; max_thr = node_rng.range(4, 6); cooldown_ms = node_rng.range(1000, 2000);
                } else {
                    queue_cap = node_rng.range(5, 10); min_thr = 1; max_thr = node_rng.range(1, 3); cooldown_ms = node_rng.range(2500, 4500);
                }
            } else {
                queue_cap = node_rng.range(5, 15); min_thr = 1; max_thr = node_rng.range(2, 4); cooldown_ms = node_rng.range(1500, 3500);
            }
            let cooldown_ns = cooldown_ms * NANOS_PER_MSEC;
            let node_instance = Arc::new(
                OmegaNode::new(node_id_val, queue_cap as usize, min_thr, max_thr as usize, signal_tx.clone(), Some(cooldown_ns))
                    .unwrap_or_else(|e| panic!("Failed to create Node {}: {}", i, e)),
            );
            local_nodes_vec.push(node_instance);
        }
        local_nodes_vec
    };
    
    let omega_system = Arc::new(UltraOmegaSystem::new(nodes_vec));
    let monitor = Monitor::start(Arc::clone(&omega_system.nodes), Arc::clone(&shared_stats), overall_start_time_ns);
    // let system_pulse_surge_threshold = (omega_system.nodes.len() / 2).max(1);
    // let system_pulse = OmegaSystemPulse::new(signal_rx, MONITORING_INTERVAL_MS_CONF, system_pulse_surge_threshold);
    // let system_pulse_thread = thread::spawn(move || system_pulse.run());

    let mut submitter_handles: Vec<JoinHandle<Vec<TaskHandle<String>>>> = Vec::new();
    for submitter_idx in 0..NUM_SUBMITTER_THREADS_CONF {
        let system_clone = Arc::clone(&omega_system);
        let stats_clone = Arc::clone(&shared_stats);
        let handle = thread::spawn(move || {
            // Each thread gets its own RNG, which is now used for routing too
            let mut task_rng = OmegaRng::new(submitter_idx as u64);
            let mut handles_for_this_thread = Vec::new();

            for i in 0..TOTAL_TASKS_PER_SUBMITTER_CONF {
                stats_clone.tasks_attempted_submission.fetch_add(1, Ordering::Relaxed);
                let task_work = move || -> Result<String, TaskError> {
                    let processing_ms = AVG_TASK_PROCESSING_MS_CONF.saturating_add(task_rng.range(0, TASK_PROCESSING_VARIABILITY_MS_CONF * 2)).saturating_sub(TASK_PROCESSING_VARIABILITY_MS_CONF).max(10);
                    thread::sleep(Duration::from_millis(processing_ms));
                    if task_rng.bool(TASK_FAILURE_PROBABILITY) {
                        Err(TaskError::ExecutionFailed(Box::new(std::io::Error::new(std::io::ErrorKind::Other, format!("Simulated logic error for S{}-T{}", submitter_idx, i)))))
                    } else {
                        Ok(format!("S{}-T{} OK", submitter_idx, i))
                    }
                };
                let mut retries = 0;
                loop {
                    match system_clone.submit_cpu_task(Priority::Normal, 10, task_work, &mut task_rng) {
                        Ok(handle) => {
                            stats_clone.tasks_successfully_submitted.fetch_add(1, Ordering::Relaxed);
                            handles_for_this_thread.push(handle);
                            break;
                        }
                        Err(NodeError::SystemMaxedOut | NodeError::QueueFull) => {
                            retries += 1;
                            if retries > MAX_SUBMISSION_RETRIES_CONF {
                                stats_clone.tasks_failed_submission_max_retries.fetch_add(1, Ordering::Relaxed);
                                break;
                            }
                            thread::sleep(Duration::from_millis(SYSTEM_MAXED_OUT_BACKOFF_MS_CONF + task_rng.range(0, 99)));
                        }
                        Err(_) => {
                            stats_clone.tasks_failed_submission_max_retries.fetch_add(1, Ordering::Relaxed);
                            break;
                        }
                    }
                }
                let submission_delay = SUBMISSION_DELAY_MS_CONF.saturating_add(task_rng.range(0, SUBMISSION_DELAY_VARIABILITY_MS_CONF*2)).saturating_sub(SUBMISSION_DELAY_VARIABILITY_MS_CONF);
                if submission_delay > 0 { thread::sleep(Duration::from_millis(submission_delay)); }
            }
            handles_for_this_thread
        });
        submitter_handles.push(handle);
    }

    let mut result_handles: Vec<TaskHandle<String>> = Vec::new();
    for handle in submitter_handles {
        match handle.join() {
            Ok(handles) => result_handles.extend(handles),
            Err(_) => eprintln!("Submitter thread panicked!"),
        }
    }

    let submission_phase_duration_ns = omega_time_ns() - overall_start_time_ns;
    println!("Submission Phase completed in {:?}.", Duration::from_nanos(submission_phase_duration_ns));
    println!("Now awaiting {} task results...", result_handles.len());
    
    let mut successes = 0;
    let mut logical_failures = 0;
    let mut panics = 0;
    let mut recv_errors = 0;
    for handle in result_handles {
        match handle.recv_result() {
            Ok(Ok(_)) => { successes += 1; }
            Ok(Err(TaskError::ExecutionFailed(_))) => { logical_failures += 1; }
            Ok(Err(TaskError::Panicked(_))) => { panics += 1; }
            Err(_) => { recv_errors += 1; }
        }
    }
    println!("Result Verification Complete:");
    println!("  - Successes: {}", successes);
    println!("  - Logical Failures: {}", logical_failures);
    println!("  - Panics: {}", panics);
    if recv_errors > 0 { println!("  - Channel Receive Errors: {}", recv_errors); }

    // Stabilize while monitoring
    // UPDATED: Use omega_time_ns and u64 arithmetic for stabilization wait
    let stabilization_start_ns = omega_time_ns();
    let stabilization_duration_ns = POST_SUBMISSION_STABILIZATION_S_CONF * NANOS_PER_SEC;
    while (omega_time_ns() - stabilization_start_ns) < stabilization_duration_ns {
        let mut all_idle = true;
        for node in omega_system.nodes.iter() {
            if node.task_queue.len() > 0 || node.active_threads() != node.min_threads {
                all_idle = false;
                break;
            }
        }
        if all_idle {
            break; // Exit stabilization early if system is idle
        }
        thread::sleep(Duration::from_millis(200));
    }
    monitor.stop();
    omega_system.shutdown_all();
    let num_submitters_for_scenario = NUM_SUBMITTER_THREADS_CONF;
    let tasks_per_submitter_for_scenario = TOTAL_TASKS_PER_SUBMITTER_CONF;
    // let _ = system_pulse_thread.join(); // Wait for system pulse to finish
    let total_tasks_target_for_scenario =
        (num_submitters_for_scenario * tasks_per_submitter_for_scenario).to_string();

    println!(
        "Submission Phase completed in {:?}. Target: {} tasks.",
        // UPDATED: Format the nanosecond duration for printing
        Duration::from_nanos(submission_phase_duration_ns),
        total_tasks_target_for_scenario
    );
    // --- CORRECTED FINAL ASSERTIONS ---
    // After shutdown_all(), all nodes should have 0 active threads.
    for (idx, node) in omega_system.nodes.iter().enumerate() {
        assert_eq!(
            node.task_queue.len(),
            0,
            "Node {} queue should be empty after shutdown.",
            idx
        );
        assert_eq!(
            node.active_threads(),
            0, // THIS IS THE FIX
            "Node {} should have 0 active threads after shutdown, but had {}. min_threads was {}",
            idx,
            node.active_threads(),
            node.min_threads
        );
    }
    
    let total_duration_ns = omega_time_ns() - overall_start_time_ns;
    println!("\nFinal Stats:");
    println!("Total Tasks Attempted for Submission: {}", shared_stats.tasks_attempted_submission.load(Ordering::Relaxed));
    println!("Total Tasks Successfully Submitted: {}", shared_stats.tasks_successfully_submitted.load(Ordering::Relaxed));
    println!("Total Tasks Failed Submission (Max Retries): {}", shared_stats.tasks_failed_submission_max_retries.load(Ordering::Relaxed));
    println!("Total SystemMaxedOut Events (Router gave up): {}", shared_stats.system_maxed_out_events_router.load(Ordering::Relaxed));
    println!("Total Test Duration: {:?}", Duration::from_nanos(total_duration_ns));
    println!("\n--- Ultra-Ω System: Phase D - Scenario: {:?} Finished ---", CURRENT_SCENARIO);
}