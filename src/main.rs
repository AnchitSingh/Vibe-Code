// src/main.rs

// Add the new monitor module
mod monitor;
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

// Import the monitor
use monitor::Monitor;

use omega::borrg::BiasStrategy;
use omega::borrg::OmegaRng;
// UPDATED: Imported omega_time_ns and removed Instant
use omega::omega_timer::{omega_time_ns, omega_timer_init};
use std::io::{stdout, Write}; // Needed for flushing
use std::sync::mpsc;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
// UPDATED: Kept Duration only for thread::sleep, removed Instant
use std::time::Duration;

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
// Adjust these as needed
const NUM_NODES_CONF: usize = 24; // Set to 24 to match monitor display
const NUM_SUPER_NODES_CONF: usize = 6;
const NUM_SUBMITTER_THREADS_CONF: usize = 8;
const TOTAL_TASKS_PER_SUBMITTER_CONF: usize = 1_000;

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

// --- Shared Stats ---
// Made public so monitor module can see it
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

// --- Routing Function (Power of K Choices - remains the same) ---
// ... This function is unchanged ...
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
                let mut next;
                if _try_num > 2 {
                    // println!("try num greater than {}", _try_num);
                    next = rng.range_biased(prev.wrapping_add(1), boundary, BiasStrategy::Power(0.05));
                } else {
                    next =
                        rng.range_biased(prev.wrapping_add(1), boundary, BiasStrategy::Power(3.14));
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
                        next =
                            rng.range_biased(prev.wrapping_add(1), boundary, BiasStrategy::Stepped);
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
    omega_timer_init();
    // Clear screen at the very beginning
    print!("\x1B[2J\x1B[H");
    stdout().flush().unwrap();

    // UPDATED: Added time constants for clarity
    const NANOS_PER_MSEC: u64 = 1_000_000;
    const NANOS_PER_SEC: u64 = 1_000_000_000;

    // UPDATED: Use omega_time_ns() for start time
    let overall_start_time_ns = omega_time_ns();

    let (signal_tx, signal_rx) = mpsc::channel();
    let shared_stats = Arc::new(SharedSubmitterStats::new());

    let num_nodes_for_scenario = NUM_NODES_CONF;

    let nodes_for_submission: Arc<Vec<Arc<OmegaNode<String>>>> = {
        let mut local_nodes_vec: Vec<Arc<OmegaNode<String>>> = Vec::new();
        let mut node_rng = OmegaRng::new(0x9e79b97f469c15);

        for i in 0..num_nodes_for_scenario {
            let node_id_val = NodeId::new();
            let (queue_cap, min_thr, max_thr, cooldown_ms, _node_type_str);

            if matches!(CURRENT_SCENARIO, TestScenario::HeterogeneousNodes) {
                if i < NUM_SUPER_NODES_CONF {
                    _node_type_str = "SUPER";
                    queue_cap = node_rng.range(20, 30);
                    min_thr = 2;
                    max_thr = node_rng.range(4, 6);
                    cooldown_ms = node_rng.range(1000, 2000);
                } else {
                    _node_type_str = "NORMAL";
                    queue_cap = node_rng.range(5, 10);
                    min_thr = 1;
                    max_thr = node_rng.range(1, 3);
                    cooldown_ms = node_rng.range(2500, 4500);
                }
            } else {
                _node_type_str = "DEFAULT";
                queue_cap = node_rng.range(5, 15);
                min_thr = 1;
                max_thr = node_rng.range(2, 4);
                cooldown_ms = node_rng.range(1500, 3500);
            }

            // UPDATED: Convert cooldown from ms to nanoseconds for the node
            let cooldown_ns = cooldown_ms * NANOS_PER_MSEC;

            let node_instance = Arc::new(
                OmegaNode::<String>::new(
                    node_id_val,
                    queue_cap.try_into().unwrap(),
                    min_thr.try_into().unwrap(),
                    max_thr.try_into().unwrap(),
                    signal_tx.clone(),
                    Some(cooldown_ns), // Pass cooldown in nanoseconds
                )
                .unwrap_or_else(|e| panic!("Failed to create Node {}: {}", i, e)),
            );
            local_nodes_vec.push(node_instance);
        }
        Arc::new(local_nodes_vec)
    };
    drop(signal_tx);

    // --- START THE LIVE MONITOR ---
    // UPDATED: Pass the u64 nanosecond start time to the monitor.
    // NOTE: The Monitor module itself must also be updated to accept a u64.
    let monitor = Monitor::start(
        Arc::clone(&nodes_for_submission),
        Arc::clone(&shared_stats),
        overall_start_time_ns,
    );

    let system_pulse_surge_threshold = (nodes_for_submission.len() / 2).max(1);
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
            };

            for _ in 0..tasks_per_submitter_for_scenario {
                submitter_loop_logic();
            }
        });
        submitter_handles.push(handle);
    }

    for handle in submitter_handles {
        handle.join().expect("Submitter thread panicked");
    }

    // UPDATED: Measure submission phase duration using omega_time_ns
    let submission_phase_duration_ns = omega_time_ns() - overall_start_time_ns;

    // Stabilize while monitoring
    // UPDATED: Use omega_time_ns and u64 arithmetic for stabilization wait
    let stabilization_start_ns = omega_time_ns();
    let stabilization_duration_ns = POST_SUBMISSION_STABILIZATION_S_CONF * NANOS_PER_SEC;
    while (omega_time_ns() - stabilization_start_ns) < stabilization_duration_ns {
        let mut all_idle = true;
        for node in nodes_for_submission.iter() {
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

    // --- STOP THE LIVE MONITOR ---
    monitor.stop();
    
    let total_tasks_target_for_scenario =
        (num_submitters_for_scenario * tasks_per_submitter_for_scenario).to_string();

    println!(
        "Submission Phase completed in {:?}. Target: {} tasks.",
        // UPDATED: Format the nanosecond duration for printing
        Duration::from_nanos(submission_phase_duration_ns),
        total_tasks_target_for_scenario
    );

    for (idx, node) in nodes_for_submission.iter().enumerate() {
        if node.task_queue.len() != 0 || node.active_threads() != node.min_threads {
            // UPDATED: Calculate wait time from node's nanosecond cooldown value
            let cooldown_ms_from_node = node.scale_down_cooldown / NANOS_PER_MSEC;
            let wait_time_ms = cooldown_ms_from_node.max(MONITORING_INTERVAL_MS_CONF);
            thread::sleep(Duration::from_millis(wait_time_ms + 200));
        }
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

    for node_arc_ref in nodes_for_submission.iter() {
        node_arc_ref.shutdown();
    }
    drop(nodes_for_submission);

    let _ = system_pulse_thread;

    let n_nodes_for_k_calc = NUM_NODES_CONF;
     let k_for_routing_log = if n_nodes_for_k_calc <= 1 {
        1
    } else {
        (2.0f64)
            .max((n_nodes_for_k_calc as f64).log2().floor())
            .floor() as usize
    }
    .min(n_nodes_for_k_calc);
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
    // UPDATED: Calculate and format total duration for printing
    let total_duration_ns = omega_time_ns() - overall_start_time_ns;
    println!("Total Test Duration: {:?}", Duration::from_nanos(total_duration_ns));

    println!(
        "\n--- Ultra-Ω System: Phase D - Scenario: {:?} Finished ---",
        CURRENT_SCENARIO
    );
}