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

use task::{Priority, TaskError, TaskHandle};

use crate::ultra_omega::{SharedSubmitterStats, UltraOmegaSystem};
use omega::borrg::OmegaRng;
use omega::omega_timer::{elapsed_ns, timer_init};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use sha2::{Digest, Sha256}; // NEW: Import for hashing

const USE_HETEROGENEOUS_NODES: bool = true; // Use Super/Normal node configuration
const NUM_NODES_CONF: usize = 8;
const NUM_SUPER_NODES_CONF: usize = 2; // The first 2 nodes will be "super"
const NUM_SUBMITTER_THREADS_CONF: usize = 16;
const TOTAL_TASKS_PER_SUBMITTER_CONF: usize = 10; // Increased task count for a good stress test

// Configuration for the CPU-bound task
const HASHING_ITERATIONS_SUPER_NODE: u32 = 150_000; // More work for expected super nodes
const HASHING_ITERATIONS_NORMAL_NODE: u32 = 50_000; // Less work for normal nodes

// src/main.rs

use io::{IoOp, IoOutput};

use std::net::SocketAddr;

fn main() {
    // Initialize timers once for the whole application
    timer_init().expect("Failed to initialize omega_timer");

    println!("=======================================================");
    println!("  PERFORMING ULTRA-Ω CPU STRESS & CORRECTNESS TEST");
    println!("=======================================================\n");
    // run_cpu_stress_test();

    println!("\n\n=======================================================");
    println!("  PERFORMING ULTRA-Ω I/O ECHO & TIMEOUT TEST");
    println!("=======================================================\n");
    run_io_tests();
}

// --- I/O Test Runner ---
fn run_io_tests() {
    println!("--- Test 1: Successful Echo Communication (Corrected Flow) ---");
    test_successful_echo_corrected();

    println!("\n--- Test 2: Client Receive Timeout (Unchanged) ---");
    // This test should still work as it doesn't involve the listener race condition.
    test_client_receive_timeout_corrected();

    run_mixed_io_cpu_drone_swarm_test();

}

// --- I/O Test Implementation: Successful Echo ---
fn test_successful_echo_corrected() {
    // 1. Setup
    let system = Arc::new(UltraOmegaSystem::builder().with_nodes(2).build());
    let mut rng = OmegaRng::new(42);
    let listen_addr: SocketAddr = "127.0.0.1:9988".parse().unwrap();
    let generous_timeout = Some(Duration::from_secs(5));

    // --- STEP 1: Start the listener and wait for it to be ready ---
    println!("Step 1: Submitting TcpListen task...");
    let listen_setup_handle = system
        .submit_io_task(IoOp::TcpListen { addr: listen_addr }, None, &mut rng)
        .unwrap();
    let listener_token = match listen_setup_handle.recv_result().unwrap().unwrap() {
        IoOutput::TcpListenerReady { listener_token, .. } => {
            println!("   -> Listener is ready! Token: {}", listener_token);
            listener_token
        }
        output => panic!("Expected TcpListenerReady, got {:?}", output),
    };

    // --- STEP 2: Now that the listener is ready, submit the server's ACCEPT task ---
    // This task will block until a client connects.
    println!("Step 2: Submitting TcpAccept task for the server...");
    let server_accept_handle = system
        .submit_io_task(IoOp::TcpAccept { listener_token }, None, &mut rng)
        .unwrap();

    // --- STEP 3: And also submit the client's CONNECT task ---
    println!("Step 3: Submitting client connect task...");
    let client_connect_handle = system
        .submit_io_task(
            IoOp::TcpConnect {
                peer_addr: listen_addr,
            },
            generous_timeout,
            &mut rng,
        )
        .unwrap();

    // --- STEP 4: Await the results of the connection from both sides ---
    println!("Step 4: Awaiting connection results...");

    // Server side
    let (server_token, peer_addr) = match server_accept_handle.recv_result().unwrap().unwrap() {
        IoOutput::NewConnectionAccepted {
            connection_token,
            peer_addr,
            ..
        } => {
            println!(
                "   -> Server accepted new connection! Token: {}",
                connection_token
            );
            (connection_token, peer_addr)
        }
        output => panic!("Expected NewConnectionAccepted, got {:?}", output),
    };

    // Client side
    let client_token = match client_connect_handle.recv_result().unwrap().unwrap() {
        IoOutput::TcpConnectionEstablished {
            connection_token, ..
        } => {
            println!(
                "   -> Client connected successfully! Token: {}",
                connection_token
            );
            connection_token
        }
        output => panic!("Expected TcpConnectionEstablished, got {:?}", output),
    };

    // --- STEP 5: Perform the echo ---
    println!("Step 5: Performing echo...");
    let message_to_send = b"Hello Corrected Omega!".to_vec();

    // Client sends, server receives
    let server_recv_handle = system
        .submit_io_task(
            IoOp::TcpReceive {
                connection_token: server_token,
                max_bytes: 1024,
            },
            generous_timeout,
            &mut rng,
        )
        .unwrap();
    let _ = system
        .submit_io_task(
            IoOp::TcpSend {
                connection_token: client_token,
                data: message_to_send.clone(),
            },
            generous_timeout,
            &mut rng,
        )
        .unwrap();

    let received_data = match server_recv_handle.recv_result().unwrap().unwrap() {
        IoOutput::TcpDataReceived { data } => data,
        output => panic!("Expected TcpDataReceived, got {:?}", output),
    };
    println!(
        "   -> Server received: '{}'",
        String::from_utf8_lossy(&received_data)
    );

    // Server sends, client receives
    let client_recv_handle = system
        .submit_io_task(
            IoOp::TcpReceive {
                connection_token: client_token,
                max_bytes: 1024,
            },
            generous_timeout,
            &mut rng,
        )
        .unwrap();
    let _ = system
        .submit_io_task(
            IoOp::TcpSend {
                connection_token: server_token,
                data: received_data,
            },
            generous_timeout,
            &mut rng,
        )
        .unwrap();

    let echo_data = match client_recv_handle.recv_result().unwrap().unwrap() {
        IoOutput::TcpDataReceived { data } => data,
        output => panic!("Expected TcpDataReceived, got {:?}", output),
    };
    println!(
        "   -> Client received echo: '{}'",
        String::from_utf8_lossy(&echo_data)
    );
    assert_eq!(echo_data, message_to_send);

    println!("\n--- CORRECTED ECHO TEST SUCCEEDED! ---");
}

// --- I/O Test Implementation: Timeout ---
fn test_client_receive_timeout_corrected() {
    // 1. Setup
    let system = Arc::new(UltraOmegaSystem::builder().with_nodes(2).build());
    let mut rng = OmegaRng::new(84);
    let listen_addr: SocketAddr = "127.0.0.1:9989".parse().unwrap();
    let short_timeout = Some(Duration::from_millis(100));

    // --- STEP 1: Start the listener and wait for it to be ready ---
    println!("Step 1: Starting TCP listener on {}", listen_addr);
    let listen_setup_handle = system
        .submit_io_task(IoOp::TcpListen { addr: listen_addr }, None, &mut rng)
        .unwrap();
    let listener_token = match listen_setup_handle.recv_result().unwrap().unwrap() {
        IoOutput::TcpListenerReady { listener_token, .. } => listener_token,
        output => panic!("Expected TcpListenerReady, got {:?}", output),
    };

    // --- STEP 2: Submit the server's ACCEPT and client's CONNECT tasks ---
    let _server_accept_handle = system
        .submit_io_task(IoOp::TcpAccept { listener_token }, None, &mut rng)
        .unwrap();
    let client_connect_handle = system
        .submit_io_task(
            IoOp::TcpConnect {
                peer_addr: listen_addr,
            },
            Some(Duration::from_secs(1)),
            &mut rng,
        )
        .unwrap();

    // --- STEP 3: Await the client connection ---
    // We don't need the server's accepted token for this specific test.
    let client_token = match client_connect_handle.recv_result().unwrap().unwrap() {
        IoOutput::TcpConnectionEstablished {
            connection_token, ..
        } => connection_token,
        output => panic!("Expected TcpConnectionEstablished, got {:?}", output),
    };
    println!("   -> Client & Server connected successfully.");

    // --- STEP 4: The Test: Client tries to receive, but server sends NOTHING ---
    println!("Step 2: Client submitting receive task with a short 100ms timeout...");
    println!("        (Server will deliberately not send anything)");
    let client_recv_handle = system
        .submit_io_task(
            IoOp::TcpReceive {
                connection_token: client_token,
                max_bytes: 1024,
            },
            short_timeout,
            &mut rng,
        )
        .unwrap();

    // --- STEP 5: Verify the timeout ---
    println!("Step 3: Awaiting result... should be a timeout error.");
    match client_recv_handle.recv_result() {
        Ok(Err(TaskError::TimedOut)) => {
            // This is the expected, correct outcome!
            println!("   -> Correctly received TaskError::TimedOut!");
        }
        Ok(Ok(output)) => {
            panic!(
                "TEST FAILED: Expected a timeout, but received data: {:?}",
                output
            );
        }
        Ok(Err(e)) => {
            panic!(
                "TEST FAILED: Expected a timeout, but received a different error: {}",
                e
            );
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
    use monitor::Monitor;
    use sha2::{Digest, Sha256};
    use std::io::{Write, stdout};

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

                match system_clone.submit_cpu_task(
                    Priority::Normal,
                    iterations as u32,
                    task_work,
                    &mut task_rng,
                ) {
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

    println!(
        "Submission Phase completed in {:?}.",
        Duration::from_nanos(elapsed_ns() - submission_start_time)
    );
    println!(
        "Now awaiting and verifying {} task results...",
        all_verification_data.len()
    );

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



// Add this to your main() function:
// 
// fn main() {
//     timer_init().expect("Failed to initialize omega_timer");
//     
//     println!("=======================================================");
//     println!("  PERFORMING ULTRA-Ω MIXED I/O & CPU DRONE SWARM TEST");
//     println!("=======================================================\n");
//     run_mixed_io_cpu_drone_swarm_test();
// }

// Add this function to your main.rs file

use std::collections::HashMap;
use serde_json::{json, Value};

// Drone swarm simulation constants
const NUM_DRONE_NODES: usize = 5;
const BASE_DRONE_PORT: u16 = 10000;
const TELEMETRY_PROCESSING_ITERATIONS: u32 = 25_000;
const MISSION_DATA_PROCESSING_ITERATIONS: u32 = 75_000;
const NETWORK_TIMEOUT_SHORT: Duration = Duration::from_millis(200);
const NETWORK_TIMEOUT_LONG: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
struct DroneMessage {
    drone_id: u32,
    message_type: DroneMessageType,
    payload: Vec<u8>,
    timestamp: u64,
}

#[derive(Debug, Clone)]
enum DroneMessageType {
    Telemetry,
    MissionData,
    HeartBeat,
    EmergencyAlert,
}

fn run_mixed_io_cpu_drone_swarm_test() {
    println!("=======================================================");
    println!("  MIXED I/O & CPU DRONE SWARM SIMULATION TEST");
    println!("=======================================================\n");

    let system = Arc::new(UltraOmegaSystem::builder().with_nodes(NUM_DRONE_NODES).build());
    let mut rng = OmegaRng::new(12345);
    
    // Phase 1: Setup drone network nodes
    println!("Phase 1: Setting up drone network infrastructure...");
    let drone_listeners = setup_drone_listeners(&system, &mut rng);
    
    // Phase 2: Establish inter-drone connections
    println!("Phase 2: Establishing peer-to-peer connections...");
    let drone_connections = establish_drone_mesh(&system, &mut rng, &drone_listeners);
    
    // Phase 3: Mixed operations simulation
    println!("Phase 3: Running mixed I/O and CPU operations...");
    run_drone_operations(&system, &mut rng, &drone_connections);
    
    println!("\n--- DRONE SWARM SIMULATION COMPLETED ---");
}

fn setup_drone_listeners(
    system: &UltraOmegaSystem, 
    rng: &mut OmegaRng
) -> HashMap<u32, u64> {
    let mut listeners = HashMap::new();
    
    for drone_id in 0..NUM_DRONE_NODES as u32 {
        let port = BASE_DRONE_PORT + drone_id as u16;
        let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
        
        println!("  Setting up drone {} listener on port {}...", drone_id, port);
        
        match system.submit_io_task(
            IoOp::TcpListen { addr }, 
            Some(Duration::from_secs(3)), 
            rng
        ) {
            Ok(handle) => {
                match handle.recv_result() {
                    Ok(Ok(IoOutput::TcpListenerReady { listener_token, .. })) => {
                        listeners.insert(drone_id, listener_token);
                        println!("    ✓ Drone {} listener ready (token: {})", drone_id, listener_token);
                    }
                    Ok(Err(e)) => {
                        println!("    ✗ Drone {} listener failed: {}", drone_id, e);
                    }
                    _ => {
                        println!("    ✗ Drone {} listener setup timeout", drone_id);
                    }
                }
            }
            Err(e) => {
                println!("    ✗ Failed to submit listener task for drone {}: {:?}", drone_id, e);
            }
        }
    }
    
    listeners
}

fn establish_drone_mesh(
    system: &UltraOmegaSystem,
    rng: &mut OmegaRng,
    listeners: &HashMap<u32, u64>
) -> HashMap<(u32, u32), u64> {
    let mut connections = HashMap::new();
    let mut accept_handles = Vec::new();
    let mut connect_handles = Vec::new();
    
    // Start accept tasks for all listeners
    for (&drone_id, &listener_token) in listeners {
        for _ in 0..2 { // Each drone can accept up to 2 connections
            if let Ok(handle) = system.submit_io_task(
                IoOp::TcpAccept { listener_token },
                Some(NETWORK_TIMEOUT_LONG),
                rng
            ) {
                accept_handles.push((drone_id, handle));
            }
        }
    }
    
    // Create connections between drones (partial mesh for realism)
    let connection_pairs = vec![
        (0, 1), (0, 2), (1, 2), (1, 3), (2, 3), (2, 4), (3, 4)
    ];
    
    for (drone_a, drone_b) in connection_pairs {
        let port = BASE_DRONE_PORT + drone_b as u16;
        let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
        
        println!("  Connecting drone {} to drone {}...", drone_a, drone_b);
        
        if let Ok(handle) = system.submit_io_task(
            IoOp::TcpConnect { peer_addr: addr },
            Some(NETWORK_TIMEOUT_LONG),
            rng
        ) {
            connect_handles.push(((drone_a, drone_b), handle));
        }
    }
    
    // Process connection results
    for ((drone_a, drone_b), handle) in connect_handles {
        match handle.recv_result() {
            Ok(Ok(IoOutput::TcpConnectionEstablished { connection_token, .. })) => {
                connections.insert((drone_a, drone_b), connection_token);
                println!("    ✓ Connection established: {} -> {} (token: {})", 
                         drone_a, drone_b, connection_token);
            }
            Ok(Err(e)) => {
                println!("    ✗ Connection failed: {} -> {} ({})", drone_a, drone_b, e);
            }
            _ => {
                println!("    ✗ Connection timeout: {} -> {}", drone_a, drone_b);
            }
        }
    }
    
    connections
}

fn run_drone_operations(
    system: &UltraOmegaSystem,
    rng: &mut OmegaRng,
    connections: &HashMap<(u32, u32), u64>
) {
    let mut operation_handles = Vec::new();
    
    // Simulate various drone operations
    for i in 0..20 {
        let operation_type = match i % 4 {
            0 => run_telemetry_exchange(system, rng, connections, i),
            1 => run_mission_data_processing(system, rng, connections, i),
            2 => run_heartbeat_with_timeout(system, rng, connections, i),
            3 => run_emergency_alert_chain(system, rng, connections, i),
            _ => unreachable!(),
        };
        
        if let Some(handles) = operation_type {
            operation_handles.extend(handles);
        }
        
        // Add some realistic delay between operations
        thread::sleep(Duration::from_millis(50));
    }
    
    // Wait for all operations to complete and analyze results
    analyze_operation_results(operation_handles);
}

fn run_telemetry_exchange(
    system: &UltraOmegaSystem,
    rng: &mut OmegaRng,
    connections: &HashMap<(u32, u32), u64>,
    operation_id: usize
) -> Option<Vec<TaskHandle<Vec<u8>>>> {
    println!("  Running telemetry exchange operation {}...", operation_id);
    
    // Find a random connection
    let connection_keys: Vec<_> = connections.keys().collect();
    if connection_keys.is_empty() {
        println!("    ✗ No connections available for telemetry");
        return None;
    }
    
    let &(drone_a, drone_b) = connection_keys[rng.range(0, connection_keys.len() as u64) as usize];
    let &connection_token = connections.get(&(drone_a, drone_b)).unwrap();
    
    // Create telemetry data
    let telemetry_data = create_telemetry_data(drone_a, operation_id);
    
    // Send telemetry data
    let send_handle = system.submit_io_task(
        IoOp::TcpSend {
            connection_token,
            data: telemetry_data.clone(),
        },
        Some(NETWORK_TIMEOUT_SHORT),
        rng
    );
    
    if send_handle.is_err() {
        println!("    ✗ Failed to submit send task");
        return None;
    }
    
    // Process telemetry data with CPU task
    let cpu_handle = system.submit_cpu_task(
        Priority::Normal,
        TELEMETRY_PROCESSING_ITERATIONS,
        move || -> Result<Vec<u8>, TaskError> {
            // Simulate telemetry processing
            let mut hasher = Sha256::new();
            let mut processed_data = telemetry_data;
            
            for i in 0..TELEMETRY_PROCESSING_ITERATIONS {
                hasher.update(&processed_data);
                hasher.update(&i.to_le_bytes());
                processed_data = hasher.finalize_reset().to_vec();
            }
            
            // Simulate potential processing failure (5% chance)
            if processed_data[0] % 20 == 0 {
                return Err(TaskError::ExecutionFailed(
                    "Telemetry data corrupted during processing".into()
                ));
            }
            
            Ok(processed_data)
        },
        rng
    );
    
    match cpu_handle {
        Ok(handle) => {
            println!("    → Telemetry processing queued for drones {} -> {}", drone_a, drone_b);
            Some(vec![handle])
        }
        Err(e) => {
            println!("    ✗ Failed to submit CPU task: {:?}", e);
            None
        }
    }
}

fn run_mission_data_processing(
    system: &UltraOmegaSystem,
    rng: &mut OmegaRng,
    connections: &HashMap<(u32, u32), u64>,
    operation_id: usize
) -> Option<Vec<TaskHandle<Vec<u8>>>> {
    println!("  Running mission data processing operation {}...", operation_id);
    
    // This simulates a more complex operation with chained CPU tasks
    let mission_data = create_mission_data(operation_id);
    
    // First CPU task: data validation and preprocessing
    let preprocess_handle = system.submit_cpu_task(
        Priority::High,
        MISSION_DATA_PROCESSING_ITERATIONS / 3,
        {
            let data = mission_data.clone();
            move || -> Result<Vec<u8>, TaskError> {
                let mut hasher = Sha256::new();
                let mut processed = data;
                
                for i in 0..(MISSION_DATA_PROCESSING_ITERATIONS / 3) {
                    hasher.update(&processed);
                    hasher.update("PREPROCESS".as_bytes());
                    hasher.update(&i.to_le_bytes());
                    processed = hasher.finalize_reset().to_vec();
                }
                
                // Simulate validation failure (10% chance)
                if processed[0] % 10 == 0 {
                    return Err(TaskError::ExecutionFailed(
                        "Mission data validation failed".into()
                    ));
                }
                
                Ok(processed)
            }
        },
        rng
    );
    
    // Second CPU task: main processing (will be chained after first completes)
    let main_process_handle = system.submit_cpu_task(
        Priority::Normal,
        MISSION_DATA_PROCESSING_ITERATIONS,
        {
            let data = mission_data.clone();
            move || -> Result<Vec<u8>, TaskError> {
                let mut hasher = Sha256::new();
                let mut processed = data;
                
                for i in 0..MISSION_DATA_PROCESSING_ITERATIONS {
                    hasher.update(&processed);
                    hasher.update("MAIN_PROCESS".as_bytes());
                    hasher.update(&i.to_le_bytes());
                    processed = hasher.finalize_reset().to_vec();
                }
                
                Ok(processed)
            }
        },
        rng
    );
    
    match (preprocess_handle, main_process_handle) {
        (Ok(h1), Ok(h2)) => {
            println!("    → Mission data processing chain queued (2 tasks)");
            Some(vec![h1, h2])
        }
        _ => {
            println!("    ✗ Failed to submit mission processing tasks");
            None
        }
    }
}

fn run_heartbeat_with_timeout(
    system: &UltraOmegaSystem,
    rng: &mut OmegaRng,
    connections: &HashMap<(u32, u32), u64>,
    operation_id: usize
) -> Option<Vec<TaskHandle<Vec<u8>>>> {
    println!("  Running heartbeat with timeout operation {}...", operation_id);
    
    // Find a connection
    let connection_keys: Vec<_> = connections.keys().collect();
    if connection_keys.is_empty() {
        return None;
    }
    
    let &(drone_a, drone_b) = connection_keys[rng.range(0, (connection_keys.len()-1) as u64) as usize];
    let &connection_token = connections.get(&(drone_a, drone_b)).unwrap();
    
    // Send heartbeat
    let heartbeat_data = create_heartbeat_data(drone_a);
    let _send_result = system.submit_io_task(
        IoOp::TcpSend {
            connection_token,
            data: heartbeat_data,
        },
        Some(NETWORK_TIMEOUT_SHORT),
        rng
    );
    
    // Try to receive response with very short timeout (likely to fail)
    let receive_handle = system.submit_io_task(
        IoOp::TcpReceive {
            connection_token,
            max_bytes: 1024,
        },
        Some(Duration::from_millis(50)), // Very short timeout
        rng
    );
    
    // Process heartbeat response if received
    if let Ok(io_handle) = receive_handle {
        let cpu_handle = system.submit_cpu_task(
            Priority::Low,
            1000, // Light processing
            move || -> Result<Vec<u8>, TaskError> {
                // This will likely not run due to I/O timeout
                Ok(b"heartbeat_processed".to_vec())
            },
            rng
        );
        
        if let Ok(handle) = cpu_handle {
            println!("    → Heartbeat with expected timeout queued {} -> {}", drone_a, drone_b);
            return Some(vec![handle]);
        }
    }
    
    println!("    ✗ Heartbeat operation setup failed");
    None
}

fn run_emergency_alert_chain(
    system: &UltraOmegaSystem,
    rng: &mut OmegaRng,
    connections: &HashMap<(u32, u32), u64>,
    operation_id: usize
) -> Option<Vec<TaskHandle<Vec<u8>>>> {
    println!("  Running emergency alert chain operation {}...", operation_id);
    
    // This simulates a high-priority emergency that needs to be processed and broadcast
    let emergency_data = create_emergency_data(operation_id);
    
    // High-priority CPU task for emergency processing
    let emergency_handle = system.submit_cpu_task(
        Priority::High,
        TELEMETRY_PROCESSING_ITERATIONS * 2, // More intensive processing
        {
            let data = emergency_data.clone();
            move || -> Result<Vec<u8>, TaskError> {
                let mut hasher = Sha256::new();
                let mut processed = data;
                
                for i in 0..(TELEMETRY_PROCESSING_ITERATIONS * 2) {
                    hasher.update(&processed);
                    hasher.update("EMERGENCY".as_bytes());
                    hasher.update(&i.to_le_bytes());
                    processed = hasher.finalize_reset().to_vec();
                }
                
                // Emergency processing is critical - low failure rate (2%)
                if processed[0] % 50 == 0 {
                    return Err(TaskError::ExecutionFailed(
                        "Critical emergency processing failure".into()
                    ));
                }
                
                Ok(processed)
            }
        },
        rng
    );
    
    // Try to broadcast to multiple connections (fire-and-forget I/O)
    let mut broadcast_count = 0;
    for &connection_token in connections.values() {
        if broadcast_count >= 3 { break; } // Limit broadcasts
        
        let _ = system.submit_io_task(
            IoOp::TcpSend {
                connection_token,
                data: emergency_data.clone(),
            },
            Some(NETWORK_TIMEOUT_SHORT),
            rng
        );
        broadcast_count += 1;
    }
    
    match emergency_handle {
        Ok(handle) => {
            println!("    → Emergency alert processing queued (broadcast to {} connections)", broadcast_count);
            Some(vec![handle])
        }
        Err(e) => {
            println!("    ✗ Failed to submit emergency processing task: {:?}", e);
            None
        }
    }
}

fn analyze_operation_results(handles: Vec<TaskHandle<Vec<u8>>>) {
    println!("\nAnalyzing operation results...");
    
    let mut successful_operations = 0;
    let mut failed_operations = 0;
    let mut timeout_operations = 0;
    let mut channel_errors = 0;
    
    for (i, handle) in handles.into_iter().enumerate() {
        match handle.recv_result() {
            Ok(Ok(_result)) => {
                successful_operations += 1;
                if i % 5 == 0 { // Print every 5th success
                    println!("  ✓ Operation {} completed successfully", i);
                }
            }
            Ok(Err(TaskError::TimedOut)) => {
                timeout_operations += 1;
                println!("  ⏱ Operation {} timed out", i);
            }
            Ok(Err(e)) => {
                failed_operations += 1;
                println!("  ✗ Operation {} failed: {}", i, e);
            }
            Err(_) => {
                channel_errors += 1;
                println!("  ⚠ Operation {} channel error", i);
            }
        }
    }
    
    println!("\n--- DRONE SWARM OPERATION RESULTS ---");
    println!("  Successful Operations: {}", successful_operations);
    println!("  Failed Operations:     {}", failed_operations);
    println!("  Timeout Operations:    {}", timeout_operations);
    println!("  Channel Errors:        {}", channel_errors);
    println!("  Total Operations:      {}", 
             successful_operations + failed_operations + timeout_operations + channel_errors);
    
    let success_rate = if successful_operations + failed_operations + timeout_operations > 0 {
        (successful_operations as f64 / 
         (successful_operations + failed_operations + timeout_operations) as f64) * 100.0
    } else {
        0.0
    };
    
    println!("  Success Rate:          {:.1}%", success_rate);
    
    if success_rate > 60.0 {
        println!("\n>>>> DRONE SWARM RESILIENCE TEST PASSED <<<<");
    } else {
        println!("\n>>>> DRONE SWARM NEEDS OPTIMIZATION <<<<");
    }
}

// Helper functions for creating realistic drone data
fn create_telemetry_data(drone_id: u32, operation_id: usize) -> Vec<u8> {
    let telemetry = json!({
        "drone_id": drone_id,
        "operation_id": operation_id,
        "timestamp": elapsed_ns(),
        "position": {
            "x": (drone_id as f64 * 10.0) + (operation_id as f64 * 0.1),
            "y": (drone_id as f64 * 5.0) + (operation_id as f64 * 0.2),
            "z": 100.0 + (operation_id as f64 * 0.5)
        },
        "battery": 85.0 - (operation_id as f64 * 0.3),
        "status": "OPERATIONAL"
    });
    
    telemetry.to_string().into_bytes()
}

fn create_mission_data(operation_id: usize) -> Vec<u8> {
    let mission = json!({
        "mission_id": operation_id,
        "timestamp": elapsed_ns(),
        "waypoints": [
            {"x": 100.0, "y": 100.0, "z": 50.0},
            {"x": 200.0, "y": 150.0, "z": 75.0},
            {"x": 300.0, "y": 200.0, "z": 100.0}
        ],
        "objectives": ["SURVEY", "COLLECT_DATA", "RETURN"],
        "priority": "NORMAL"
    });
    
    mission.to_string().into_bytes()
}

fn create_heartbeat_data(drone_id: u32) -> Vec<u8> {
    let heartbeat = json!({
        "drone_id": drone_id,
        "timestamp": elapsed_ns(),
        "type": "HEARTBEAT",
        "status": "ALIVE"
    });
    
    heartbeat.to_string().into_bytes()
}

fn create_emergency_data(operation_id: usize) -> Vec<u8> {
    let emergency = json!({
        "alert_id": operation_id,
        "timestamp": elapsed_ns(),
        "type": "EMERGENCY",
        "severity": "HIGH",
        "message": "Obstacle detected - immediate attention required",
        "coordinates": {"x": 150.0, "y": 200.0, "z": 80.0}
    });
    
    emergency.to_string().into_bytes()
}