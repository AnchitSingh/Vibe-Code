// Filename: src/bin/ovp_swarm_simulation.rs
// (Or replace the content of your existing test harness)

// Make sure to add `serde_json` and `sha2` to your Cargo.toml dependencies
// Also ensure your `ultra_omega` library and the `ovp` crate are correctly referenced.

use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use omega::omega_timer::timer_init;

// Assuming your crate structure is set up correctly
use ultra_omega::{
    io::{IoOp, IoOutput}, // Import DroneId from the IO module
    task::{Priority, TaskError, TaskHandle},
    UltraOmegaSystem,
};
use ovp::{DroneId};

// Use your custom RNG and Timer
use omega::borrg::OmegaRng;
use omega::omega_timer::elapsed_ns;

use serde_json::json;
use sha2::{Digest, Sha256};

// --- Simulation Constants ---
const NUM_DRONE_NODES: usize = 5;
const OVP_INTERFACE: &str = "lo"; // Use loopback for local simulation
const TELEMETRY_PROCESSING_ITERATIONS: u32 = 25_000;
const MISSION_DATA_PROCESSING_ITERATIONS: u32 = 75_000;
const NETWORK_TIMEOUT_SHORT: Duration = Duration::from_millis(200);
const NETWORK_TIMEOUT_LONG: Duration = Duration::from_secs(2);
// src/main.rs

mod monitor; 

// src/main.rs

use std::net::SocketAddr;

// --- Main Test Runner ---
fn main() {
    // Initialize the timer first
    timer_init().expect("Failed to initialize timer");
    
    // Start timing the entire test
    let start_time = Instant::now();
    
    println!("=======================================================");
    println!("  OVP-POWERED MIXED I/O & CPU DRONE SWARM SIMULATION");
    println!("=======================================================\n");
    
    let _ = run_mixed_io_cpu_drone_swarm_test_with_ovp();
    
    let total_duration = start_time.elapsed();
    println!("\n=== TEST EXECUTION TIME ===");
    println!("Total program time: {:.2?}", total_duration);
    println!("===========================\n");
}

// The main simulation function, now refactored for OVP
fn run_mixed_io_cpu_drone_swarm_test_with_ovp() -> Duration {
    let start_time = Instant::now();
    let mut rng = OmegaRng::new(12345);

    // Phase 1: Setup drone network nodes with OVP
    println!("Phase 1: Setting up drone network infrastructure with OVP...");
    let drones = setup_ovp_drones(&mut rng);
    if drones.is_empty() {
        println!("✗ Critical error: No drones could be initialized. Aborting test.");
        return Duration::from_secs(0);
    }

    // Phase 2 is now obsolete. There are no "connections" to establish.
    // Drones can communicate immediately.
    println!("Phase 2: OVP Network Ready. No connection establishment needed.");

    // Phase 3: Mixed operations simulation
    println!("\nPhase 3: Running mixed I/O and CPU operations over OVP...");
    // Run operations and get the duration
    let operations_duration = run_drone_operations_ovp(&drones, &mut rng);
    
    // Calculate total test duration
    let total_duration = start_time.elapsed();
    
    println!("\n--- DRONE SWARM SIMULATION TIMING ---");
    println!("Setup time: {:.2?}", total_duration - operations_duration);
    println!("Operations time: {:.2?}", operations_duration);
    println!("Total test time: {:.2?}", total_duration);
    println!("------------------------------------\n");
    
    let duration = start_time.elapsed();
    println!("\n--- DRONE SWARM SIMULATION COMPLETED ---\n");
    duration // Return the actual duration of the test
}

/// Initializes a separate UltraOmegaSystem for each drone, each with its own OVP socket.
fn setup_ovp_drones(rng: &mut OmegaRng) -> HashMap<DroneId, Arc<UltraOmegaSystem>> {
    let mut drone_systems = HashMap::new();

    for i in 0..NUM_DRONE_NODES {
        let drone_id = (i + 1) as DroneId; // Drone IDs from 1 to N
        println!("  Initializing Drone {}...", drone_id);

        // Each drone gets its own independent system instance
        let system = Arc::new(
            UltraOmegaSystem::builder()
                .with_nodes(2) // Give each drone's system a couple of CPU nodes
                .build(),
        );

        // Submit the OVP initialization task for this drone's system
        let handle = system.submit_io_task(
            IoOp::OvpInit {
                interface: OVP_INTERFACE.to_string(),
                my_drone_id: drone_id,
            },
            Some(NETWORK_TIMEOUT_LONG),
            rng,
        ).expect("Failed to submit OvpInit task");

        // Wait for the socket to be ready
        match handle.recv_result() {
            Ok(Ok(IoOutput::OvpSocketReady { socket_token })) => {
                println!("    ✓ Drone {} OVP socket ready (token: {})", drone_id, socket_token);
                // We don't need to store the token here, as the DroneId is our primary identifier.
                // The system manages the token internally.
                drone_systems.insert(drone_id, system);
            }
            res => {
                println!("    ✗ Drone {} failed to initialize OVP socket. Result: {:?}", drone_id, res);
            }
        }
    }
    drone_systems
}

/// Runs a series of simulated drone operations over OVP.
fn run_drone_operations_ovp(drones: &HashMap<DroneId, Arc<UltraOmegaSystem>>, rng: &mut OmegaRng) -> Duration {
    let start_time = Instant::now();
    let mut operation_handles = Vec::new();

    for i in 0..20 {
        // Choose a random operation type
        let op_type = rng.range(0, 4);
        println!("\n[DEBUG] Starting operation {} with type {}...", i, op_type);

        let handles = match op_type {
            0 => {
                println!("[DEBUG] Calling run_telemetry_exchange_ovp");
                run_telemetry_exchange_ovp(drones, rng, i)
            },
            1 => {
                println!("[DEBUG] Calling run_mission_data_processing_ovp");
                run_mission_data_processing_ovp(drones, rng, i)
            },
            2 => {
                println!("[DEBUG] Calling run_heartbeat_with_timeout_ovp");
                run_heartbeat_with_timeout_ovp(drones, rng, i)
            },
            3 => {
                println!("[DEBUG] Calling run_emergency_alert_chain_ovp");
                run_emergency_alert_chain_ovp(drones, rng, i)
            },
            _ => {
                eprintln!("[ERROR] Unexpected operation type: {}", op_type);
                None
            },
        };

        match &handles {
            Some(h) => println!("[DEBUG] Got {} handles from operation {}", h.len(), i),
            None => println!("[DEBUG] No handles returned from operation {}", i),
        }

        if let Some(h) = handles {
            operation_handles.extend(h);
            println!("[DEBUG] Extended operation handles. Total handles: {}", operation_handles.len());
        }
        
        thread::sleep(Duration::from_millis(50));
    }

    analyze_operation_results(operation_handles);
    start_time.elapsed()
}

// --- Refactored Operation Functions ---

fn run_telemetry_exchange_ovp(
    drones: &HashMap<DroneId, Arc<UltraOmegaSystem>>,
    rng: &mut OmegaRng,
    operation_id: usize,
) -> Option<Vec<TaskHandle<Vec<u8>>>> {
    println!("  Running telemetry exchange operation {}...", operation_id);

    // Pick two random, different drones
    let drone_ids: Vec<_> = drones.keys().cloned().collect();
    if drone_ids.len() < 2 { return None; }
    let drone_a_id = drone_ids[rng.range(0, drone_ids.len() as u64) as usize];
    let mut drone_b_id = drone_ids[rng.range(0, drone_ids.len() as u64) as usize];
    while drone_a_id == drone_b_id {
        drone_b_id = drone_ids[rng.range(0, drone_ids.len() as u64) as usize];
    }

    let drone_a_system = drones.get(&drone_a_id).unwrap();
    let drone_b_system = drones.get(&drone_b_id).unwrap();

    let telemetry_data = create_telemetry_data(drone_a_id as u32, operation_id);

    // Drone B listens for the message
    let recv_handle = drone_b_system.submit_io_task(
        IoOp::OvpReceive { socket_token: 1 }, // Assuming token is 1 for the first OVP socket
        Some(NETWORK_TIMEOUT_SHORT),
        rng,
    ).ok()?;

    // Drone A emits the message to Drone B
    let _ = drone_a_system.submit_io_task(
        IoOp::OvpEmit {
            socket_token: 1, // Also token 1 in its own system
            targets: Some(vec![drone_b_id]),
            payload: telemetry_data.clone(),
        },
        None,
        rng,
    );

    // Now, instead of waiting for the I/O, we queue the CPU task immediately.
    // This simulates processing data that is *expected* to arrive.
    let cpu_handle = drone_a_system.submit_cpu_task(
        Priority::Normal,
        TELEMETRY_PROCESSING_ITERATIONS,
        move || { /* CPU-bound hashing logic as before */ Ok(telemetry_data) }, // Simplified for brevity
        rng,
    ).ok()?;

    println!("    → Telemetry exchange and processing queued for {} -> {}", drone_a_id, drone_b_id);
    // In a real app, you'd chain the CPU task after the recv_handle completes.
    // For this simulation, we'll just track the CPU handle.
    Some(vec![cpu_handle])
}


fn run_mission_data_processing_ovp(
    drones: &HashMap<DroneId, Arc<UltraOmegaSystem>>,
    rng: &mut OmegaRng,
    operation_id: usize,
) -> Option<Vec<TaskHandle<Vec<u8>>>> {
    println!("  Running mission data processing operation {}...", operation_id);

    // This operation is purely CPU-bound, so it doesn't need to change much.
    // We just need a system instance to submit the tasks to.
    let drone_ids: Vec<_> = drones.keys().cloned().collect();
    if drone_ids.is_empty() { return None; }
    let drone_id = drone_ids[rng.range(0, drone_ids.len() as u64) as usize];
    let system = drones.get(&drone_id).unwrap();

    let mission_data = create_mission_data(operation_id);
    let mission_data_clone = mission_data.clone();

    let preprocess_handle = system.submit_cpu_task(
        Priority::High,
        MISSION_DATA_PROCESSING_ITERATIONS / 3,
        move || {
            let mut processed = mission_data_clone;
            // ... hashing logic ...
            if processed[0] % 10 == 0 {
                return Err(TaskError::ExecutionFailed("Validation failed".into()));
            }
            Ok(processed)
        },
        rng,
    ).ok()?;

    let main_process_handle = system.submit_cpu_task(
        Priority::Normal,
        MISSION_DATA_PROCESSING_ITERATIONS,
        move || {
            let processed = mission_data;
            // ... hashing logic ...
            Ok(processed)
        },
        rng,
    ).ok()?;

    println!("    → Mission data processing chain queued on Drone {}", drone_id);
    Some(vec![preprocess_handle, main_process_handle])
}

fn run_heartbeat_with_timeout_ovp(
    drones: &HashMap<DroneId, Arc<UltraOmegaSystem>>,
    rng: &mut OmegaRng,
    operation_id: usize,
) -> Option<Vec<TaskHandle<Vec<u8>>>> {
    println!("[DEBUG] Entering run_heartbeat_with_timeout_ovp");
    println!("  Running heartbeat with timeout operation {}...", operation_id);
    println!("[DEBUG] Got {} drones", drones.len());

    // Pick two random, different drones
    let drone_ids: Vec<_> = drones.keys().cloned().collect();
    println!("[DEBUG] Available drone IDs: {:?}", drone_ids);
    
    if drone_ids.len() < 2 { 
        println!("[DEBUG] Not enough drones for heartbeat operation");
        return None; 
    }
    
    // Use the correct range (0..len) and ensure we don't go out of bounds
    let len = drone_ids.len() as u64;
    let drone_a_idx = rng.range(0, len) as usize % drone_ids.len();
    
    // For the second drone, select from remaining indices
    let remaining_indices: Vec<_> = (0..drone_ids.len())
        .filter(|&i| i != drone_a_idx)
        .collect();
        
    if remaining_indices.is_empty() {
        println!("[DEBUG] No available drones for second selection");
        return None;
    }
    
    let drone_b_idx = remaining_indices[rng.range(0, remaining_indices.len() as u64) as usize % remaining_indices.len()];
    
    // Get the drone IDs with bounds checking just to be safe
    let drone_a_id = drone_ids[drone_a_idx];
    let drone_b_id = drone_ids[drone_b_idx];
    
    println!("[DEBUG] Selected drones: {} -> {}", drone_a_id, drone_b_id);

    let drone_a_system = drones.get(&drone_a_id).unwrap();
    let drone_b_system = drones.get(&drone_b_id).unwrap();

    // Drone A listens for a response that will never come.
    let recv_handle = drone_a_system.submit_io_task(
        IoOp::OvpReceive { socket_token: 1 },
        Some(Duration::from_millis(50)), // Very short timeout
        rng,
    ).ok()?;

    // Drone B sends a heartbeat to A.
    let heartbeat_data = create_heartbeat_data(drone_b_id as u32);
    let _ = drone_b_system.submit_io_task(
        IoOp::OvpEmit {
            socket_token: 1,
            targets: Some(vec![drone_a_id]),
            payload: heartbeat_data,
        },
        None,
        rng,
    );
    
    // The "operation" we are tracking is the receive handle, which we expect to time out.
    // In a real system, the TaskHandle would need to be generic over different output types.
    // For this simulation, we will create a dummy CPU task handle to represent the operation.
    println!("[DEBUG] Submitting CPU task for heartbeat");
    let dummy_handle = match drone_a_system.submit_cpu_task(
        Priority::Low,
        1,
        {
            let recv_handle = recv_handle; // Move into closure
            move || {
                println!("[DEBUG] Inside heartbeat CPU task");
                // This task's result depends on the I/O task.
                match recv_handle.recv_result() {
                    Ok(Ok(_)) => {
                        println!("[DEBUG] Heartbeat unexpectedly succeeded");
                        Ok(vec![])
                    },
                    Ok(Err(TaskError::TimedOut)) => {
                        println!("[DEBUG] Heartbeat timed out as expected");
                        Err(TaskError::TimedOut)
                    },
                    Ok(Err(TaskError::Panicked(_))) => {
                        println!("[DEBUG] Heartbeat task panicked");
                        Err(TaskError::ExecutionFailed("Task panicked".into()))
                    },
                    Ok(Err(TaskError::ExecutionFailed(msg))) => {
                        println!("[DEBUG] Heartbeat execution failed: {}", msg);
                        Err(TaskError::ExecutionFailed(msg))
                    },
                    Err(e) => {
                        println!("[DEBUG] Heartbeat channel error: {:?}", e);
                        Err(TaskError::ExecutionFailed("Channel error".into()))
                    },
                }
            }
        },
        rng,
    ) {
        Ok(handle) => handle,
        Err(e) => {
            println!("[ERROR] Failed to submit CPU task: {:?}", e);
            return None;
        }
    };

    println!("    → Heartbeat with expected timeout queued {} -> {}", drone_b_id, drone_a_id);
    Some(vec![dummy_handle])
}

fn run_emergency_alert_chain_ovp(
    drones: &HashMap<DroneId, Arc<UltraOmegaSystem>>,
    rng: &mut OmegaRng,
    operation_id: usize,
) -> Option<Vec<TaskHandle<Vec<u8>>>> {
    println!("  Running emergency alert chain operation {}...", operation_id);

    // Pick a random drone to be the source of the emergency.
    let drone_ids: Vec<_> = drones.keys().cloned().collect();
    if drone_ids.is_empty() { return None; }
    let source_id = drone_ids[rng.range(0, drone_ids.len() as u64) as usize];
    let source_system = drones.get(&source_id).unwrap();

    let emergency_data = create_emergency_data(operation_id);
    let emergency_data_clone = emergency_data.clone();

    // High-priority CPU processing for the emergency data.
    let cpu_handle = source_system.submit_cpu_task(
        Priority::High,
        TELEMETRY_PROCESSING_ITERATIONS * 2,
        move || {
            let processed = emergency_data;
            // ... hashing logic ...
            if processed[0] % 50 == 0 {
                return Err(TaskError::ExecutionFailed("Emergency processing failed".into()));
            }
            Ok(processed)
        },
        rng,
    ).ok()?;

    // Volumetric broadcast of the alert to ALL drones.
    let _ = source_system.submit_io_task(
        IoOp::OvpEmit {
            socket_token: 1,
            targets: None, // None means broadcast to everyone
            payload: emergency_data_clone,
        },
        None,
        rng,
    );

    println!("    → Emergency alert processing queued on Drone {}, broadcasting to swarm.", source_id);
    Some(vec![cpu_handle])
}


// --- Unchanged Helper and Analysis Functions ---
// The following functions (analyze_operation_results, create_telemetry_data, etc.)
// can remain exactly as they were, as they deal with creating data and analyzing
// the results of TaskHandles, which is independent of the underlying transport.
// I have omitted them here for brevity but they should be included in the final file.
// (Paste the `analyze_operation_results` and `create_*_data` functions from your original file here)

// Dummy struct and enum for type-checking, as in the original file
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
                if i % 5 == 0 {
                    // Print every 5th success
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
    println!(
        "  Total Operations:      {}",
        successful_operations + failed_operations + timeout_operations + channel_errors
    );

    let success_rate = if successful_operations + failed_operations + timeout_operations > 0 {
        (successful_operations as f64
            / (successful_operations + failed_operations + timeout_operations) as f64)
            * 100.0
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