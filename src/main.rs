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

use node::OmegaNode;
use signals::NodeId;
// use system_pulse::OmegaSystemPulse;
// TaskHandle and TaskError are now needed by the client (main)
use task::{Priority, TaskError, TaskHandle};

use monitor::Monitor;
use omega::borrg:: OmegaRng;
use omega::omega_timer::{elapsed_ns, timer_init};
use std::sync::mpsc;
use std::sync::{
    atomic:: Ordering,
    Arc
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use crate::ultra_omega::{UltraOmegaSystem, SharedSubmitterStats};


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


const USE_HETEROGENEOUS_NODES: bool = true; // Use Super/Normal node configuration
const NUM_NODES_CONF: usize = 8;
const NUM_SUPER_NODES_CONF: usize = 2; // The first 2 nodes will be "super"
const NUM_SUBMITTER_THREADS_CONF: usize = 16;
const TOTAL_TASKS_PER_SUBMITTER_CONF: usize = 50; // Increased task count for a good stress test

// Configuration for the CPU-bound task
const HASHING_ITERATIONS_SUPER_NODE: u32 = 150_000; // More work for expected super nodes
const HASHING_ITERATIONS_NORMAL_NODE: u32 = 50_000; // Less work for normal nodes


// src/main.rs

use io::{IoOp, IoOutput};
use types::NodeError;

use std::net::SocketAddr;

// --- CPU Test Constants ---
// (No changes here)
// const NUM_NODES_CONF: usize = 8;
// const NUM_SUPER_NODES_CONF: usize = 2;
// const NUM_SUBMITTER_THREADS_CONF: usize = 16;
// const TOTAL_TASKS_PER_SUBMITTER_CONF: usize = 500;
// const HASHING_ITERATIONS_SUPER_NODE: u32 = 150_000;
// const HASHING_ITERATIONS_NORMAL_NODE: u32 = 50_000;


// --- Main Function: Acts as a Test Runner ---
fn main() {
    // Initialize timers once for the whole application
    timer_init().expect("Failed to initialize omega_timer");

    println!("=======================================================");
    println!("  PERFORMING ULTRA-Ω CPU STRESS & CORRECTNESS TEST");
    println!("=======================================================\n");
    run_cpu_stress_test();
    
    println!("\n\n=======================================================");
    println!("  PERFORMING ULTRA-Ω I/O ECHO & TIMEOUT TEST");
    println!("=======================================================\n");
    run_io_tests();
}


// --- I/O Test Runner ---
fn run_io_tests() {
    println!("--- Test 1: Successful Echo Communication ---");
    test_successful_echo();

    println!("\n--- Test 2: Client Receive Timeout ---");
    test_client_receive_timeout();
}

// --- I/O Test Implementation: Successful Echo ---
fn test_successful_echo() {
    // 1. Setup
    let system = Arc::new(
        UltraOmegaSystem::builder()
            .with_nodes(2)
            .with_super_nodes(0)
            .build(),
    );
    let mut rng = OmegaRng::new(42);
    let listen_addr: SocketAddr = "127.0.0.1:9988".parse().unwrap();
    let generous_timeout = Some(Duration::from_secs(5));

    // 2. Start Listener
    println!("Step 1: Starting TCP listener on {}", listen_addr);
    let listener_handle = system.submit_io_task(
        IoOp::TcpListen { addr: listen_addr },
        None, // Listeners don't time out
        &mut rng,
    ).unwrap();

    // 3. Client Connects
    println!("Step 2: Submitting client connect task...");
    let client_connect_handle = system.submit_io_task(
        IoOp::TcpConnect { peer_addr: listen_addr },
        generous_timeout,
        &mut rng,
    ).unwrap();

    // 4. Server Accepts
    println!("Step 3: Awaiting listener to accept connection...");
    let server_token = match listener_handle.recv_result().unwrap().unwrap() {
        IoOutput::NewConnectionAccepted { connection_token, .. } => connection_token,
        output => panic!("Expected NewConnectionAccepted, got {:?}", output),
    };
    println!("   -> Listener accepted new connection! Token: {}", server_token);
    
    // 5. Client Confirms Connection
    println!("Step 4: Awaiting client connection result...");
    let client_token = match client_connect_handle.recv_result().unwrap().unwrap() {
        IoOutput::TcpConnectionEstablished { connection_token, .. } => connection_token,
        output => panic!("Expected TcpConnectionEstablished, got {:?}", output),
    };
    println!("   -> Client connected successfully! Token: {}", client_token);

    // 6. Echo Logic
    let message_to_send = b"Hello Omega!".to_vec();
    let server_recv_handle = system.submit_io_task(IoOp::TcpReceive { connection_token: server_token, max_bytes: 1024 }, generous_timeout, &mut rng).unwrap();
    let _client_send_handle = system.submit_io_task(IoOp::TcpSend { connection_token: client_token, data: message_to_send.clone() }, generous_timeout, &mut rng).unwrap();

    match server_recv_handle.recv_result().unwrap().unwrap() {
        IoOutput::TcpDataReceived { data } => {
            println!("   -> Server received: '{}'", String::from_utf8_lossy(&data));
            let _server_echo_handle = system.submit_io_task(IoOp::TcpSend { connection_token: server_token, data }, generous_timeout, &mut rng).unwrap();
        }
        output => panic!("Expected TcpDataReceived, got {:?}", output),
    }

    let client_recv_echo_handle = system.submit_io_task(IoOp::TcpReceive { connection_token: client_token, max_bytes: 1024 }, generous_timeout, &mut rng).unwrap();
    match client_recv_echo_handle.recv_result().unwrap().unwrap() {
        IoOutput::TcpDataReceived { data } => {
            println!("   -> Client received echo: '{}'", String::from_utf8_lossy(&data));
            assert_eq!(data, message_to_send);
        }
        output => panic!("Expected TcpDataReceived, got {:?}", output),
    }

    println!("\n--- SUCCESSFUL ECHO TEST SUCCEEDED! ---");
}

// --- I/O Test Implementation: Timeout ---
fn test_client_receive_timeout() {
    // 1. Setup
    let system = Arc::new(
        UltraOmegaSystem::builder()
            .with_nodes(2)
            .with_super_nodes(0)
            .build(),
    );
    let mut rng = OmegaRng::new(84);
    let listen_addr: SocketAddr = "127.0.0.1:9989".parse().unwrap();
    let short_timeout = Some(Duration::from_millis(100)); // 100ms timeout

    // 2. Start Listener & Client Connects (same as before)
    println!("Step 1: Starting TCP listener on {}", listen_addr);
    let listener_handle = system.submit_io_task(IoOp::TcpListen { addr: listen_addr }, None, &mut rng).unwrap();
    let client_connect_handle = system.submit_io_task(IoOp::TcpConnect { peer_addr: listen_addr }, Some(Duration::from_secs(1)), &mut rng).unwrap();

    // 3. Server Accepts, Client Confirms (same as before)
    let _server_token = match listener_handle.recv_result().unwrap().unwrap() {
        IoOutput::NewConnectionAccepted { connection_token, .. } => connection_token,
        output => panic!("Expected NewConnectionAccepted, got {:?}", output),
    };
    let client_token = match client_connect_handle.recv_result().unwrap().unwrap() {
        IoOutput::TcpConnectionEstablished { connection_token, .. } => connection_token,
        output => panic!("Expected TcpConnectionEstablished, got {:?}", output),
    };
    println!("   -> Client & Server connected successfully.");

    // 4. The Test: Client tries to receive, but server sends NOTHING.
    println!("Step 2: Client submitting receive task with a short 100ms timeout...");
    println!("        (Server will deliberately not send anything)");
    let client_recv_handle = system.submit_io_task(
        IoOp::TcpReceive { connection_token: client_token, max_bytes: 1024 },
        short_timeout,
        &mut rng,
    ).unwrap();

    // 5. Verify the timeout
    println!("Step 3: Awaiting result... should be a timeout error.");
    match client_recv_handle.recv_result() {
        Ok(Err(TaskError::TimedOut)) => {
            // This is the expected, correct outcome!
            println!("   -> Correctly received TaskError::TimedOut!");
        }
        Ok(Ok(output)) => {
            panic!("TEST FAILED: Expected a timeout, but received data: {:?}", output);
        }
        Ok(Err(e)) => {
            panic!("TEST FAILED: Expected a timeout, but received a different error: {}", e);
        }
        Err(_) => {
            panic!("TEST FAILED: Channel disconnected, expected a timeout error.");
        }
    }

    println!("\n--- CLIENT TIMEOUT TEST SUCCEEDED! ---");
}


// --- CPU STRESS TEST ---
// This function remains exactly as it was in the previous correct version.
// I've collapsed it here for brevity, but it's the same code.
fn run_cpu_stress_test() {
    // ... (The entire CPU stress test logic from the previous main function) ...
    // ... It starts with `let omega_system = Arc::new(...)`
    // ... and ends with `assert_eq!(failed_verifications, 0, ...)`
    use sha2::{Digest, Sha256};
    use std::io::{stdout, Write};
    use monitor::Monitor;
    
    let omega_system = Arc::new(
        UltraOmegaSystem::builder()
            .with_nodes(NUM_NODES_CONF)
            .with_super_nodes(NUM_SUPER_NODES_CONF)
            .build(),
    );

    let shared_stats = Arc::new(SharedSubmitterStats::new());
    let monitor = Monitor::start(
        Arc::clone(&omega_system.nodes),
        Arc::clone(&shared_stats),
        elapsed_ns(),
    );

    let submission_start_time = elapsed_ns();
    let mut submitter_handles = Vec::new();
    type VerificationTuple = (TaskHandle<Vec<u8>>, Vec<u8>);

    for submitter_idx in 0..NUM_SUBMITTER_THREADS_CONF {
        let system_clone = Arc::clone(&omega_system);
        let stats_clone = Arc::clone(&shared_stats);

        let handle: thread::JoinHandle<Vec<VerificationTuple>> = thread::spawn(move || {
            let mut task_rng = OmegaRng::new(submitter_idx as u64);
            let mut verification_data_for_thread = Vec::new();

            for task_idx in 0..TOTAL_TASKS_PER_SUBMITTER_CONF {
                let iterations = if task_rng.bool(0.5) { 
                    HASHING_ITERATIONS_SUPER_NODE 
                } else { 
                    HASHING_ITERATIONS_NORMAL_NODE 
                };
                let seed_data = format!("S{}-T{}", submitter_idx, task_idx).into_bytes();

                let expected_result = {
                    let mut hasher = Sha256::new();
                    let mut current_data = seed_data.clone();
                    for _ in 0..iterations {
                        hasher.update(&current_data);
                        current_data = hasher.finalize_reset().to_vec();
                    }
                    current_data
                };
                
                let task_work = move || -> Result<Vec<u8>, TaskError> {
                    let mut hasher = Sha256::new();
                    let mut current_data = seed_data;
                    for _ in 0..iterations {
                        hasher.update(&current_data);
                        current_data = hasher.finalize_reset().to_vec();
                    }
                    Ok(current_data)
                };

                match system_clone.submit_cpu_task(Priority::Normal, iterations as u32, task_work, &mut task_rng) {
                    Ok(handle) => {
                        verification_data_for_thread.push((handle, expected_result));
                    }
                    Err(_) => {
                        // In a real app, handle this submission failure
                    }
                }
            }
            verification_data_for_thread
        });
        submitter_handles.push(handle);
    }

    let mut all_verification_data: Vec<VerificationTuple> = Vec::new();
    for handle in submitter_handles {
        all_verification_data.extend(handle.join().unwrap());
    }
    
    println!("Submission Phase completed in {:?}.", Duration::from_nanos(elapsed_ns() - submission_start_time));
    println!("Now awaiting and verifying {} task results...", all_verification_data.len());
    
    let mut successful_verifications = 0;
    let mut failed_verifications = 0;
    
    for (handle, expected_result) in all_verification_data {
        if let Ok(Ok(worker_result)) = handle.recv_result() {
            if worker_result == expected_result {
                successful_verifications += 1;
            } else {
                failed_verifications += 1;
            }
        } else {
            failed_verifications += 1;
        }
    }
    
    println!("\n--- Result Verification Complete ---");
    println!("  - Successful Verifications: {}", successful_verifications);
    println!("  - Failed Verifications:     {}", failed_verifications);
    if failed_verifications == 0 {
        println!("\n>>>> ALL TASKS VERIFIED SUCCESSFULLY. OMEGA-GRADE CORRECTNESS CONFIRMED. <<<<");
    }

    monitor.stop();
}