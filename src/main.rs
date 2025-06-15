// src/main.rs

mod monitor;
mod node;
mod queue;
mod signals;
// mod system_pulse;
mod task;
mod types;
// ADD the io module declaration
mod io;
mod ultra_omega;
// ... rest of the modules ...

// USE the new public I/O types
use io::{IoOp, IoOutput};
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
use crate::ultra_omega::{UltraOmegaSystem,SharedSubmitterStats};
// --- Test Scenario Selection & Configuration (remains the same) ---
#[derive(Debug, Clone, Copy)]
enum TestScenario {
    Baseline,
    ExtremeDurations,
    FailingTasks,
    SustainedLoad,
    HeterogeneousNodes,
}
// const CURRENT_SCENARIO: TestScenario = TestScenario::HeterogeneousNodes;

// const NUM_NODES_CONF: usize = 8;
// const NUM_SUPER_NODES_CONF: usize = 2;
// const NUM_SUBMITTER_THREADS_CONF: usize = 16;
// const TOTAL_TASKS_PER_SUBMITTER_CONF: usize = 50;
// // const AVG_TASK_PROCESSING_MS_CONF: u64 = 75;
// // const TASK_PROCESSING_VARIABILITY_MS_CONF: u64 = 50;
// const SUBMISSION_DELAY_MS_CONF: u64 = 5;
// const SUBMISSION_DELAY_VARIABILITY_MS_CONF: u64 = 5;
// const SYSTEM_MAXED_OUT_BACKOFF_MS_CONF: u64 = 15;
// const MAX_SUBMISSION_RETRIES_CONF: usize = 10;
// const MONITORING_INTERVAL_MS_CONF: u64 = 1000;
// const POST_SUBMISSION_STABILIZATION_S_CONF: u64 = 20;
// const TASK_FAILURE_PROBABILITY: f64 = 0.05;
use sha2::{Digest, Sha256}; // NEW: Import for hashing

use std::net::SocketAddr;

const USE_HETEROGENEOUS_NODES: bool = true; // Use Super/Normal node configuration
const NUM_NODES_CONF: usize = 8;
const NUM_SUPER_NODES_CONF: usize = 2; // The first 2 nodes will be "super"
const NUM_SUBMITTER_THREADS_CONF: usize = 16;
const TOTAL_TASKS_PER_SUBMITTER_CONF: usize = 500; // Increased task count for a good stress test

// Configuration for the CPU-bound task
const HASHING_ITERATIONS_SUPER_NODE: u32 = 150_000; // More work for expected super nodes
const HASHING_ITERATIONS_NORMAL_NODE: u32 = 50_000; // Less work for normal nodes

fn main() {
    omega_timer_init();
    println!("--- Starting Ultra-Ω CPU Stress & Correctness Test ---");
    println!("--- NOTE: This will utilize 100% of available CPU cores. ---");
    
    // --- System Setup ---
    let (signal_tx, _) = mpsc::channel(); // Signal channel for nodes
    let shared_stats = Arc::new(SharedSubmitterStats::new());

    // Create a heterogeneous set of nodes
    let nodes_vec: Vec<Arc<OmegaNode>> = {
        let mut local_nodes_vec = Vec::new();
        for i in 0..NUM_NODES_CONF {
            let (queue_cap, min_thr, max_thr) = if USE_HETEROGENEOUS_NODES && i < NUM_SUPER_NODES_CONF {
                // Super Node configuration
                (20, 2, 8) 
            } else {
                // Normal Node configuration
                (10, 1, 4)
            };
            let node = Arc::new(OmegaNode::new(NodeId::new(), queue_cap, min_thr, max_thr, signal_tx.clone(), None).unwrap());
            local_nodes_vec.push(node);
        }
        local_nodes_vec
    };
    
    let omega_system = Arc::new(UltraOmegaSystem::new(nodes_vec));
    let monitor = Monitor::start(Arc::clone(&omega_system.nodes), Arc::clone(&shared_stats), omega_time_ns());

    // --- Task Submission ---
    let submission_start_time = omega_time_ns();
    let mut submitter_handles = Vec::new();
    
    // This Vec will store the handles AND the expected correct result for later verification.
    type VerificationTuple = (TaskHandle<Vec<u8>>, Vec<u8>);

    for submitter_idx in 0..NUM_SUBMITTER_THREADS_CONF {
        let system_clone = Arc::clone(&omega_system);
        let stats_clone = Arc::clone(&shared_stats);

        let handle: JoinHandle<Vec<VerificationTuple>> = thread::spawn(move || {
            let mut task_rng = OmegaRng::new(submitter_idx as u64);
            let mut verification_data_for_thread = Vec::new();

            for task_idx in 0..TOTAL_TASKS_PER_SUBMITTER_CONF {
                stats_clone.tasks_attempted_submission.fetch_add(1, Ordering::Relaxed);

                // Determine workload and pre-calculate the correct answer
                let iterations = if task_rng.bool(0.5) { 
                    HASHING_ITERATIONS_SUPER_NODE 
                } else { 
                    HASHING_ITERATIONS_NORMAL_NODE 
                };
                let seed_data = format!("S{}-T{}", submitter_idx, task_idx).into_bytes();

                // Pre-calculate the correct final hash in the submitter thread
                let expected_result = {
                    let mut hasher = Sha256::new();
                    let mut current_data = seed_data.clone();
                    for _ in 0..iterations {
                        hasher.update(&current_data);
                        current_data = hasher.finalize_reset().to_vec();
                    }
                    current_data
                };
                
                // This is the actual work the OmegaNode worker will perform
                let task_work = move || -> Result<Vec<u8>, TaskError> {
                    let mut hasher = Sha256::new();
                    let mut current_data = seed_data;
                    for _ in 0..iterations {
                        hasher.update(&current_data);
                        current_data = hasher.finalize_reset().to_vec();
                    }
                    Ok(current_data)
                };

                // Submit the task (simplified retry logic for this test)
                match system_clone.submit_cpu_task(Priority::Normal, iterations as u32, task_work, &mut task_rng) {
                    Ok(handle) => {
                        stats_clone.tasks_successfully_submitted.fetch_add(1, Ordering::Relaxed);
                        verification_data_for_thread.push((handle, expected_result));
                    }
                    Err(_) => {
                        stats_clone.tasks_failed_submission_max_retries.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            verification_data_for_thread
        });
        submitter_handles.push(handle);
    }

    // --- Result Collection and Verification ---
    let mut all_verification_data: Vec<VerificationTuple> = Vec::new();
    for handle in submitter_handles {
        all_verification_data.extend(handle.join().unwrap());
    }
    
    let submission_phase_duration = Duration::from_nanos(omega_time_ns() - submission_start_time);
    println!("Submission Phase completed in {:?}.", submission_phase_duration);
    println!("Now awaiting and verifying {} task results...", all_verification_data.len());
    
    let mut successful_verifications = 0;
    let mut failed_verifications = 0;
    
    for (handle, expected_result) in all_verification_data {
        match handle.recv_result() {
            Ok(Ok(worker_result)) => {
                if worker_result == expected_result {
                    successful_verifications += 1;
                } else {
                    failed_verifications += 1;
                    eprintln!("!! VERIFICATION FAILED for Task ID {} !!", handle.get_task_id());
                }
            }
            Ok(Err(e)) => {
                failed_verifications += 1;
                eprintln!("!! Task ID {} failed with logic error: {} !!", handle.get_task_id(), e);
            }
            Err(_) => {
                failed_verifications += 1;
                eprintln!("!! Failed to receive result for Task ID {} (channel error) !!", handle.get_task_id());
            }
        }
    }
    
    println!("\n--- Result Verification Complete ---");
    println!("  - Successful Verifications: {}", successful_verifications);
    println!("  - Failed Verifications:     {}", failed_verifications);
    if failed_verifications > 0 {
        println!("\n!!!! CRITICAL: ONE OR MORE TASKS FAILED VERIFICATION. !!!!");
    } else {
        println!("\n>>>> ALL TASKS VERIFIED SUCCESSFULLY. OMEGA-GRADE CORRECTNESS CONFIRMED. <<<<");
    }

    // --- Shutdown ---
    monitor.stop();
    omega_system.shutdown_all();
    
    let total_duration = Duration::from_nanos(omega_time_ns() - submission_start_time);
    println!("\nFinal Stats:");
    println!("Total Tasks Submitted: {}", shared_stats.tasks_successfully_submitted.load(Ordering::Relaxed));
    println!("Total Test Duration: {:?}", total_duration);
    println!("\n--- Ultra-Ω CPU Stress Test Finished ---");

    // Final assertion to ensure correctness
    assert_eq!(failed_verifications, 0, "There should be no verification failures.");
}


// Test code for I/O tasks
// fn main() {
//     omega_timer_init();
//     println!("--- Starting Ultra-Ω I/O Echo Test ---");

//     // 1. Setup the system
//     let nodes_vec: Vec<Arc<OmegaNode>> = (0..2).map(|i| {
//         Arc::new(OmegaNode::new(NodeId::new(), 10, 1, 2, mpsc::channel().0, None).unwrap())
//     }).collect();
    
//     let omega_system = Arc::new(UltraOmegaSystem::new(nodes_vec));
//     let mut rng = OmegaRng::new(42);
//     let listen_addr: SocketAddr = "127.0.0.1:9999".parse().unwrap();

//     // 1. Start the TCP Listener
//     println!("Step 1: Starting TCP listener on {}", listen_addr);
//     let listener_handle = omega_system.submit_io_task(
//         IoOp::TcpListen { addr: listen_addr },
//         &mut rng
//     ).expect("Failed to submit listener task");

//     // 2. Start the Client and Connect
//     println!("Step 2: Submitting client connect task...");
//     let client_connect_handle = omega_system.submit_io_task(
//         IoOp::TcpConnect { peer_addr: listen_addr },
//         &mut rng
//     ).expect("Failed to submit connect task");
    
//     // In a real app, you might do other things here. For the test, we'll wait.
//     thread::sleep(Duration::from_millis(100)); // Give reactor time to process

//     // 3. Wait for the listener to accept the connection
//     println!("Step 3: Awaiting listener to accept connection...");
//     let server_token = match listener_handle.recv_result().unwrap().unwrap() {
//         IoOutput::NewConnectionAccepted { connection_token, .. } => {
//             println!("   -> Listener accepted new connection! Token: {}", connection_token);
//             connection_token
//         }
//         output => panic!("Expected NewConnectionAccepted, but got {:?}", output),
//     };

//     // 4. Wait for the connection to be established from the client's perspective
//     println!("Step 4: Awaiting client connection result...");
//     let client_token = match client_connect_handle.recv_result().unwrap().unwrap() {
//         IoOutput::TcpConnectionEstablished { connection_token, .. } => {
//             println!("   -> Client connected successfully! Token: {}", connection_token);
//             connection_token
//         }
//         output => panic!("Expected TcpConnectionEstablished, but got {:?}", output),
//     };

//     // 5. Server prepares to receive a message
//     println!("Step 5: Server submitted receive task...");
//     let server_recv_handle = omega_system.submit_io_task(
//         IoOp::TcpReceive { connection_token: server_token, max_bytes: 1024 },
//         &mut rng
//     ).unwrap();

//     // 6. Client sends a message
//     let message_to_send = b"Hello Omega!".to_vec();
//     println!("Step 6: Client sending message: '{}'", String::from_utf8_lossy(&message_to_send));
//     let client_send_handle = omega_system.submit_io_task(
//         IoOp::TcpSend { connection_token: client_token, data: message_to_send.clone() },
//         &mut rng
//     ).unwrap();
//     client_send_handle.recv_result().unwrap().unwrap(); // Wait for send confirmation
//     println!("   -> Client send confirmed.");

//     // 7. Server waits for the data, then echos it back
//     println!("Step 7: Server awaiting data to echo...");
//     match server_recv_handle.recv_result().unwrap().unwrap() {
//         IoOutput::TcpDataReceived { data } => {
//             println!("   -> Server received: '{}'", String::from_utf8_lossy(&data));
//             assert_eq!(data, message_to_send);
            
//             println!("   -> Server echoing data back...");
//             let server_echo_handle = omega_system.submit_io_task(
//                 IoOp::TcpSend { connection_token: server_token, data: data },
//                 &mut rng
//             ).unwrap();
//             server_echo_handle.recv_result().unwrap().unwrap();
//             println!("   -> Server echo confirmed.");
//         }
//         output => panic!("Expected TcpDataReceived, but got {:?}", output),
//     }

//     // 8. Client prepares to receive the echo
//     println!("Step 8: Client submitted receive task for echo...");
//     let client_recv_echo_handle = omega_system.submit_io_task(
//         IoOp::TcpReceive { connection_token: client_token, max_bytes: 1024 },
//         &mut rng
//     ).unwrap();

//     // 9. Client waits for and verifies the echo
//     println!("Step 9: Client awaiting echo...");
//     match client_recv_echo_handle.recv_result().unwrap().unwrap() {
//         IoOutput::TcpDataReceived { data } => {
//             println!("   -> Client received echo: '{}'", String::from_utf8_lossy(&data));
//             assert_eq!(data, message_to_send);
//         }
//         output => panic!("Expected TcpDataReceived, but got {:?}", output),
//     }

//     println!("\n--- I/O ECHO TEST SUCCEEDED! ---");
    
//     // 11. Clean shutdown
//     omega_system.shutdown_all();
//     println!("System shut down gracefully.");
// }
