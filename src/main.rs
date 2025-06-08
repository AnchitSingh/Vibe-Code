// src/main.rs

mod node;
mod queue;
mod signals;
mod system_pulse;
mod task;
mod types;

use node::OmegaNode;
use signals::NodeId;
use system_pulse::OmegaSystemPulse;
use task::{Priority, Task, TaskError};
use types::NodeError;

use omega::OmegaRng;
use std::sync::mpsc;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
// --- Test Scenario Selection ---
#[derive(Debug, Clone, Copy)]
enum TestScenario {
    Baseline,
    ExtremeDurations,
    FailingTasks,
    SustainedLoad,
    HeterogeneousNodes, // <<-- Current Scenario
}
const CURRENT_SCENARIO: TestScenario = TestScenario::HeterogeneousNodes;

// --- Stress Test Configuration ---
const NUM_NODES_CONF: usize = 8; // Total nodes
const NUM_SUPER_NODES_CONF: usize = 2; // How many of the total nodes will be "super"
const NUM_SUBMITTER_THREADS_CONF: usize = 16;
const TOTAL_TASKS_PER_SUBMITTER_CONF: usize = 50; // Increased tasks to give more work
const AVG_TASK_PROCESSING_MS_CONF: u64 = 75;
const TASK_PROCESSING_VARIABILITY_MS_CONF: u64 = 50;
const SUBMISSION_DELAY_MS_CONF: u64 = 5;
const SUBMISSION_DELAY_VARIABILITY_MS_CONF: u64 = 5;
const SYSTEM_MAXED_OUT_BACKOFF_MS_CONF: u64 = 250;
const MAX_SUBMISSION_RETRIES_CONF: usize = 5;
const MONITORING_INTERVAL_MS_CONF: u64 = 1000;
const POST_SUBMISSION_STABILIZATION_S_CONF: u64 = 20;

// Specific for FailingTasks scenario
const TASK_FAILURE_PROBABILITY: f64 = 0.05;

// Specific for SustainedLoad scenario
// const SUSTAINED_LOAD_DURATION_S_CONF: u64 = 30;

// --- Shared Stats ---
struct SharedSubmitterStats {
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

// --- Routing Function (Power of K Choices - remains the same) ---
fn route_task_to_least_loaded(
    nodes: &Vec<Arc<OmegaNode<String>>>,
    task: Task<String>,
    _task_id_for_log: String,
    rng: &mut OmegaRng,
) -> Result<(), NodeError> {
    let n_total_nodes = nodes.len();
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
                let mut next = rng.range(prev.wrapping_add(1), boundary);
                if 3*next >= boundary as u64 { // Weaker bias against upper 1/4
                    next = rng.range(prev.wrapping_add(1), boundary);
                }
                // if 3*idx > k && next <= 2*prev.wrapping_add(1) { // Same complex condition
                //     next = rng.range(prev.wrapping_add(1), boundary);
                // }
                chosen_indices.push(next as usize);
                prev = next;
                boundary += 1;
            }
        }

        let mut best_node_index: Option<usize> = None;
        let mut min_pressure_found = usize::MAX;

        for &node_idx in &chosen_indices {
            let node = &nodes[node_idx];
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
            return nodes[idx].submit_task(task);
        } else {
            continue;
        }
    }
    Err(NodeError::SystemMaxedOut)
}

fn main() {
    println!(
        "--- Ultra-Ω System: Phase D - Scenario: {:?} ---",
        CURRENT_SCENARIO
    );
    let overall_start_time = Instant::now();

    let (signal_tx, signal_rx) = mpsc::channel();
    let shared_stats = Arc::new(SharedSubmitterStats::new());

    let num_nodes_for_scenario = NUM_NODES_CONF;

    let nodes_for_submission: Arc<Vec<Arc<OmegaNode<String>>>> = {
        let mut local_nodes_vec: Vec<Arc<OmegaNode<String>>> = Vec::new();
        let mut node_rng = OmegaRng::new(0x9e79b97f469c15);

        println!(
            "Creating {} OmegaNodes ({} super, {} normal)...",
            num_nodes_for_scenario,
            NUM_SUPER_NODES_CONF,
            num_nodes_for_scenario - NUM_SUPER_NODES_CONF
        );

        for i in 0..num_nodes_for_scenario {
            let node_id_val = NodeId::new();
            let (queue_cap, min_thr, max_thr, cooldown_ms, node_type_str);

            if matches!(CURRENT_SCENARIO, TestScenario::HeterogeneousNodes) {
                if i < NUM_SUPER_NODES_CONF {
                    // First few are "super" nodes
                    node_type_str = "SUPER";
                    queue_cap = node_rng.range(20, 30);
                    min_thr = 2;
                    max_thr = node_rng.range(4, 6);
                    cooldown_ms = node_rng.range(1000, 2000);
                } else {
                    // The rest are "normal" nodes
                    node_type_str = "NORMAL";
                    queue_cap = node_rng.range(5, 10);
                    min_thr = 1;
                    max_thr = node_rng.range(1, 3);
                    cooldown_ms = node_rng.range(2500, 4500);
                }
            } else {
                // For other scenarios, use randomized default
                node_type_str = "DEFAULT";
                queue_cap = node_rng.range(5, 15);
                min_thr = 1;
                max_thr = node_rng.range(2, 4);
                cooldown_ms = node_rng.range(1500, 3500);
            }

            let node_instance = Arc::new(
                OmegaNode::<String>::new(
                    node_id_val,
                    queue_cap.try_into().unwrap(),
                    min_thr.try_into().unwrap(),
                    max_thr.try_into().unwrap(),
                    signal_tx.clone(),
                    Some(Duration::from_millis(cooldown_ms)),
                )
                .unwrap_or_else(|e| panic!("Failed to create {} Node {}: {}", node_type_str, i, e)),
            );

            println!(
                "  {} Node {} (ID {}): Min/Max: {}/{}. QCap: {}. MaxPressure: {}. Cooldown: {}ms",
                node_type_str,
                i,
                node_instance.id().0,
                node_instance.min_threads,
                node_instance.max_threads,
                node_instance.task_queue.capacity(),
                node_instance.max_pressure(),
                cooldown_ms
            );

            local_nodes_vec.push(node_instance);
        }
        Arc::new(local_nodes_vec)
    };
    drop(signal_tx);

    let n_nodes_for_k_calc = nodes_for_submission.len();
    let k_for_routing_log = if n_nodes_for_k_calc <= 1 {
        1
    } else {
        (2.0f64)
            .max((n_nodes_for_k_calc as f64).log2().floor())
            .floor() as usize
    }
    .min(n_nodes_for_k_calc);
    println!(
        "Routing strategy: Power of K Choices, K = {}",
        k_for_routing_log
    );

    let system_pulse_surge_threshold = (n_nodes_for_k_calc / 2).max(1);
    let system_pulse = OmegaSystemPulse::new(
        signal_rx,
        MONITORING_INTERVAL_MS_CONF,
        system_pulse_surge_threshold,
    );
    let system_pulse_thread: JoinHandle<()> = thread::Builder::new()
        .name("omega-system-pulse".to_string())
        .spawn(move || {
            system_pulse.run();
        })
        .expect("Failed to spawn SystemPulse thread");

    let num_submitters_for_scenario = NUM_SUBMITTER_THREADS_CONF;
    let tasks_per_submitter_for_scenario = TOTAL_TASKS_PER_SUBMITTER_CONF;

    println!(
        "\n--- Launching {} Submitter Threads (tasks determined by scenario) ---",
        num_submitters_for_scenario
    );
    let mut submitter_handles: Vec<JoinHandle<()>> = Vec::new();

    for submitter_idx in 0..num_submitters_for_scenario {
        let nodes_clone_for_submitter = Arc::clone(&nodes_for_submission);
        let stats_clone_for_submitter = Arc::clone(&shared_stats);

        let handle = thread::spawn(move || {
            let mut task_rng = OmegaRng::new(submitter_idx as u64);

            let mut submitter_loop_logic = || {
                stats_clone_for_submitter
                    .tasks_attempted_submission
                    .fetch_add(1, Ordering::Relaxed);
                let current_attempt_count = stats_clone_for_submitter
                    .tasks_attempted_submission
                    .load(Ordering::Relaxed);
                let task_log_id = format!("S{}-T{}", submitter_idx, current_attempt_count);

                let processing_ms = match CURRENT_SCENARIO {
                    TestScenario::ExtremeDurations => {
                        if task_rng.bool(0.2) {
                            task_rng.range(400, 800)
                        } else {
                            task_rng.range(5, 50)
                        }
                    }
                    _ => {
                        // Baseline, FailingTasks, SustainedLoad, HeterogeneousNodes use similar timing
                        let base_proc_time = AVG_TASK_PROCESSING_MS_CONF;
                        let variability = if TASK_PROCESSING_VARIABILITY_MS_CONF > 0 {
                            task_rng.range(0, TASK_PROCESSING_VARIABILITY_MS_CONF * 2)
                        } else {
                            0
                        };
                        base_proc_time
                            .saturating_add(variability)
                            .saturating_sub(TASK_PROCESSING_VARIABILITY_MS_CONF)
                            .max(10)
                    }
                };
                
                    let should_fail_this_task =
                        if matches!(CURRENT_SCENARIO, TestScenario::FailingTasks) {
                            task_rng.bool(TASK_FAILURE_PROBABILITY)
                        } else {
                            false
                        };
            let create_task_fn = || {
                let task_log_id_ok = task_log_id.clone();
                let task_log_id_fail = task_log_id.clone();
        
                let task_fn_payload = move || {
                        thread::sleep(Duration::from_millis(processing_ms));
                        if should_fail_this_task {
                            Err(TaskError::ExecutionFailed(Box::new(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("Simulated task execution error for {}", task_log_id_fail),
                            ))))
                        } else {
                            Ok(format!("Task {} Complete", task_log_id_ok))
                        }
                    };
                    Task::new(
                        Priority::Normal,
                        (processing_ms / 10).max(1) as u32,
                        task_fn_payload,
                    )
                };

                let mut submission_retries = 0;
                loop {
                    let task_to_submit = create_task_fn();
                    match route_task_to_least_loaded(
                        &nodes_clone_for_submitter,
                        task_to_submit,
                        task_log_id.clone(),
                        &mut task_rng,
                    ) {
                        Ok(_) => {
                            stats_clone_for_submitter
                                .tasks_successfully_submitted
                                .fetch_add(1, Ordering::Relaxed);
                            break;
                        }
                        Err(actual_error_value @ NodeError::SystemMaxedOut)
                        | Err(actual_error_value @ NodeError::QueueFull) => {
                            if matches!(actual_error_value, NodeError::SystemMaxedOut) {
                                stats_clone_for_submitter
                                    .system_maxed_out_events_router
                                    .fetch_add(1, Ordering::Relaxed);
                            }
                            submission_retries += 1;
                            if submission_retries > MAX_SUBMISSION_RETRIES_CONF {
                                // eprintln!("[Submitter {}] Task {} failed submission after max retries (due to {:?}).", submitter_idx, task_log_id, actual_error_value);
                                stats_clone_for_submitter
                                    .tasks_failed_submission_max_retries
                                    .fetch_add(1, Ordering::Relaxed);
                                break;
                            }

                            thread::sleep(Duration::from_millis(
                                SYSTEM_MAXED_OUT_BACKOFF_MS_CONF + task_rng.range(0, 99),
                            ));
                        }
                        Err(e) => {
                            eprintln!(
                                "[Submitter {}] Task {} FAILED submission with truly unexpected error: {:?}",
                                submitter_idx, task_log_id, e
                            );
                            stats_clone_for_submitter
                                .tasks_failed_submission_max_retries
                                .fetch_add(1, Ordering::Relaxed);
                            break;
                        }
                    }
                }

                let base_sub_delay = SUBMISSION_DELAY_MS_CONF;
                let sub_variability = if SUBMISSION_DELAY_VARIABILITY_MS_CONF > 0 {
                    task_rng.range(0, SUBMISSION_DELAY_VARIABILITY_MS_CONF * 2)
                } else {
                    0
                };
                let submission_delay = base_sub_delay
                    .saturating_add(sub_variability)
                    .saturating_sub(SUBMISSION_DELAY_VARIABILITY_MS_CONF);
                if submission_delay > 0 {
                    thread::sleep(Duration::from_millis(submission_delay));
                }
            }; // End of submitter_loop_logic

            match CURRENT_SCENARIO {
                // TestScenario::SustainedLoad => { ... } // This logic would be different
                _ => {
                    // For Baseline, ExtremeDurations, FailingTasks, HeterogeneousNodes (fixed task count)
                    for _task_idx_in_submitter in 0..tasks_per_submitter_for_scenario {
                        submitter_loop_logic();
                    }
                }
            }
        });
        submitter_handles.push(handle);
    }

    for handle in submitter_handles {
        handle.join().expect("Submitter thread panicked");
    }
    let submission_phase_duration = overall_start_time.elapsed();
    let total_tasks_target_for_scenario = match CURRENT_SCENARIO {
        // TestScenario::SustainedLoad => "N/A (duration based)".to_string(),
        _ => (num_submitters_for_scenario * tasks_per_submitter_for_scenario).to_string(),
    };

    println!(
        "\n--- All task submissions attempted (took {:?}). Total Tasks Target for Scenario: {} ---",
        submission_phase_duration, total_tasks_target_for_scenario
    );
    println!(
        "--- Waiting for system to process remaining tasks and stabilize (approx {}s)... ---",
        POST_SUBMISSION_STABILIZATION_S_CONF
    );

    let monitoring_start_time = Instant::now();
    let mut last_monitor_output_time = Instant::now();

    loop {
        let mut all_queues_empty_locally = true;
        let mut all_at_min_threads_locally = true;
        let mut total_q_len = 0;
        let mut total_active_threads = 0;

        for node in nodes_for_submission.iter() {
            let q_len = node.task_queue.len();
            let active = node.active_threads();
            total_q_len += q_len;
            total_active_threads += active;
            if q_len > 0 {
                all_queues_empty_locally = false;
            }
            if active != node.min_threads {
                all_at_min_threads_locally = false;
            }
        }

        if last_monitor_output_time.elapsed() >= Duration::from_millis(MONITORING_INTERVAL_MS_CONF)
        {
            println!(
                "[Main-Monitor @ {:?}] Total QLen: {}, Total ActiveThreads: {}",
                overall_start_time.elapsed(),
                total_q_len,
                total_active_threads
            );
            for (idx, node) in nodes_for_submission.iter().enumerate() {
                println!(
                    "  Node {} (ID {}): P: {}/{}, A: {}, D: {}, Q: {}",
                    idx,
                    node.id().0,
                    node.get_pressure(),
                    node.max_pressure(),
                    node.active_threads(),
                    node.desired_threads(),
                    node.task_queue.len()
                );
            }
            last_monitor_output_time = Instant::now();
        }

        if all_queues_empty_locally && all_at_min_threads_locally {
            let expected_total_min_threads: usize =
                nodes_for_submission.iter().map(|n| n.min_threads).sum();
            if total_active_threads == expected_total_min_threads {
                println!(
                    "[Main-Monitor @ {:?}] All nodes appear idle and at min_threads. Exiting monitor loop.",
                    overall_start_time.elapsed()
                );
                break;
            }
        }
        if monitoring_start_time.elapsed()
            >= Duration::from_secs(POST_SUBMISSION_STABILIZATION_S_CONF)
        {
            println!(
                "[Main-Monitor @ {:?}] Stabilization period ended. Proceeding to final check.",
                overall_start_time.elapsed()
            );
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    println!("\n--- Final System State Check & Stats ---");
    for (idx, node) in nodes_for_submission.iter().enumerate() {
        if node.task_queue.len() != 0 || node.active_threads() != node.min_threads {
            let wait_time = node
                .scale_down_cooldown
                .as_millis()
                .max(MONITORING_INTERVAL_MS_CONF as u128);
            println!(
                "[Main] Node {} (ID {}) not fully idle, waiting extra {}ms...",
                idx,
                node.id().0,
                wait_time
            );
            thread::sleep(Duration::from_millis(wait_time as u64 + 200));
        }
        println!(
            "Node {} (ID {}): P: {}/{}, Active: {}, Desired: {}, Q_Len: {}",
            idx,
            node.id().0,
            node.get_pressure(),
            node.max_pressure(),
            node.active_threads(),
            node.desired_threads(),
            node.task_queue.len()
        );
        assert_eq!(
            node.task_queue.len(),
            0,
            "Node {} ({}) queue should be empty. Q_Len: {}",
            idx,
            node.id().0,
            node.task_queue.len()
        );
        assert_eq!(
            node.active_threads(),
            node.min_threads,
            "Node {} ({}) active threads ({}) should be at min ({}).",
            idx,
            node.id().0,
            node.active_threads(),
            node.min_threads
        );
        assert_eq!(
            node.desired_threads(),
            node.min_threads,
            "Node {} ({}) desired threads ({}) should be at min ({}).",
            idx,
            node.id().0,
            node.desired_threads(),
            node.min_threads
        );
    }

    println!("\nShutting down all OmegaNodes...");
    for node_arc_ref in nodes_for_submission.iter() {
        node_arc_ref.shutdown();
    }
    drop(nodes_for_submission);

    println!("\nWaiting for SystemPulse to finish...");
    if let Err(e) = system_pulse_thread.join() {
        eprintln!("SystemPulse thread panicked: {:?}", e);
    } else {
        println!("SystemPulse thread finished gracefully.");
    }

    println!(
        "\n--- Stress Test Summary (Scenario: {:?}) ---",
        CURRENT_SCENARIO
    );
    println!("Configuration: K for Routing = {}", k_for_routing_log);
    println!(
        "Total Tasks Attempted for Submission: {}",
        shared_stats
            .tasks_attempted_submission
            .load(Ordering::Relaxed)
    );
    println!(
        "Total Tasks Successfully Submitted: {}",
        shared_stats
            .tasks_successfully_submitted
            .load(Ordering::Relaxed)
    );
    println!(
        "Total Tasks Failed Submission (Max Retries): {}",
        shared_stats
            .tasks_failed_submission_max_retries
            .load(Ordering::Relaxed)
    );
    println!(
        "Total SystemMaxedOut Events (Router gave up): {}",
        shared_stats
            .system_maxed_out_events_router
            .load(Ordering::Relaxed)
    );
    println!("Total Test Duration: {:?}", overall_start_time.elapsed());

    println!(
        "\n--- Ultra-Ω System: Phase D - Scenario: {:?} Finished ---",
        CURRENT_SCENARIO
    );
}
