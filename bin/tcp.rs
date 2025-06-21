
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

// Assuming your crate structure is set up correctly
use ultra_omega::{
    io::{IoOp, IoOutput},
    task::{Priority, TaskError, TaskHandle},
    UltraOmegaSystem,
};

// Use your custom RNG and Timer
use omega::borrg::OmegaRng;
use omega::omega_timer::{elapsed_ns, timer_init};

use serde_json::json;
use sha2::{Digest, Sha256};

// --- Simulation Constants ---
const NUM_DRONE_NODES: usize = 5;
const BASE_DRONE_PORT: u32 = 11000; // Use a different port base to avoid conflicts
const TELEMETRY_PROCESSING_ITERATIONS: u32 = 25_000;
const MISSION_DATA_PROCESSING_ITERATIONS: u32 = 75_000;
const NETWORK_TIMEOUT_SHORT: Duration = Duration::from_millis(200);
const NETWORK_TIMEOUT_LONG: Duration = Duration::from_secs(2);
// In both files, at the top
const TEST_DURATION: Duration = Duration::from_secs(2);

// --- Main Test Runner ---
fn main() {
    timer_init().expect("Failed to initialize omega_timer");
    println!("=======================================================");
    println!("  CORRECTED TCP-POWERED MULTI-INSTANCE SWARM SIMULATION");
    println!("=======================================================\n");

    let total_time = run_mixed_io_cpu_drone_swarm_test_tcp();
    println!("\n=== TEST EXECUTION TIME ===");
    println!("Total program time: {:.2?}", total_time);
    println!("===========================");
}

// The main simulation function, refactored for multi-instance TCP
fn run_mixed_io_cpu_drone_swarm_test_tcp() -> Duration {
    let start_time = Instant::now();
    let mut rng = OmegaRng::new(12345);

    // Phase 1: Setup independent systems and listeners for each drone
    println!("Phase 1: Setting up drone network infrastructure...");
    let setup_start = Instant::now();
    let drones = setup_tcp_drones(&mut rng);
    let setup_time = setup_start.elapsed();
    println!("  ✓ Setup completed in {:.2?}", setup_time);
    if drones.is_empty() {
        println!("✗ Critical error: No drones could be initialized. Aborting test.");
        return start_time.elapsed();
    }

    // Phase 2: Establish peer-to-peer connections between independent systems
    println!("\nPhase 2: Establishing peer-to-peer connections...");
    let connect_start = Instant::now();
    // This new map holds tokens for BOTH sides of the connection
    let connections = establish_tcp_mesh(&drones, &mut rng);
    let connect_time = connect_start.elapsed();
    println!("  ✓ Connections established in {:.2?}", connect_time);

    // Phase 3: Mixed operations simulation
    println!("\nPhase 3: Running mixed I/O and CPU operations...");
    let ops_start = Instant::now();
    run_drone_operations_tcp(&drones, &connections, &mut rng);
    let ops_time = ops_start.elapsed();

    let total_time = start_time.elapsed();

    println!("\n--- DRONE SWARM SIMULATION TIMING (TCP) ---");
    println!("Setup time:         {:.2?}", setup_time);
    println!("Connection time:    {:.2?}", connect_time);
    println!("Operations time:    {:.2?}", ops_time);
    println!("Total test time:    {:.2?}", total_time);
    println!("------------------------------------\n");

    println!("--- DRONE SWARM SIMULATION COMPLETED ---");
    total_time
}

// Represents a drone in the TCP simulation
struct TcpDrone {
    system: Arc<UltraOmegaSystem>,
    listener_token: u64,
}

/// Initializes a separate UltraOmegaSystem and a TCP listener for each drone.
fn setup_tcp_drones(rng: &mut OmegaRng) -> HashMap<u32, TcpDrone> {
    let mut drone_systems = HashMap::new();

    for drone_id in 0..NUM_DRONE_NODES as u32 {
        println!("  Initializing Drone {}...", drone_id);
        let system = Arc::new(UltraOmegaSystem::builder().with_nodes(2).build());
        let port = BASE_DRONE_PORT + drone_id as u32;
        let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

        match system.submit_io_task(IoOp::TcpListen { addr }, Some(Duration::from_secs(3)), rng) {
            Ok(handle) => match handle.recv_result() {
                Ok(Ok(IoOutput::TcpListenerReady { listener_token, .. })) => {
                    println!("    ✓ Drone {} listener ready on port {} (token: {})", drone_id, port, listener_token);
                    drone_systems.insert(drone_id, TcpDrone { system, listener_token });
                }
                res => println!("    ✗ Drone {} listener failed: {:?}", drone_id, res),
            },
            Err(e) => println!("    ✗ Failed to submit listener task for drone {}: {:?}", drone_id, e),
        }
    }
    drone_systems
}

// A map from (client_id, server_id) -> (client_token, server_accepted_token)
type ConnectionMap = HashMap<(u32, u32), (u64, u64)>;

/// Establishes a mesh of TCP connections between the independent drone systems.
fn establish_tcp_mesh(drones: &HashMap<u32, TcpDrone>, rng: &mut OmegaRng) -> ConnectionMap {
    let mut connections: ConnectionMap = HashMap::new();
    let mut accept_handles = Vec::new();
    let mut connect_handles = Vec::new();

    let connection_pairs: Vec<(u32, u32)> = vec![(0, 1), (0, 2), (1, 2), (1, 3), (2, 3), (2, 4), (3, 4)];

    // For each planned connection, the "server" side must be ready to accept.
    for (_, drone_b_id) in &connection_pairs {
        let server_drone = drones.get(drone_b_id).unwrap();
        if let Ok(handle) = server_drone.system.submit_io_task(
            IoOp::TcpAccept { listener_token: server_drone.listener_token },
            Some(NETWORK_TIMEOUT_LONG),
            rng,
        ) {
            accept_handles.push((*drone_b_id, handle));
        }
    }

    // Now, have the "client" side initiate the connections.
    for (drone_a_id, drone_b_id) in &connection_pairs {
        let client_drone = drones.get(drone_a_id).unwrap();
        let port = BASE_DRONE_PORT + *drone_b_id as u32;
        let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

        println!("  Connecting drone {} to drone {}...", drone_a_id, drone_b_id);

        if let Ok(handle) = client_drone.system.submit_io_task(
            IoOp::TcpConnect { peer_addr: addr },
            Some(NETWORK_TIMEOUT_LONG),
            rng,
        ) {
            connect_handles.push(((*drone_a_id, *drone_b_id), handle));
        }
    }

    // Process all connection results. This is complex because we need to match client tokens with server tokens.
    let mut client_tokens: HashMap<(u32, u32), u64> = HashMap::new();
    for ((drone_a, drone_b), handle) in connect_handles {
        if let Ok(Ok(IoOutput::TcpConnectionEstablished { connection_token, .. })) = handle.recv_result() {
            client_tokens.insert((drone_a, drone_b), connection_token);
        }
    }

    let mut server_tokens: HashMap<u32, Vec<u64>> = HashMap::new();
     for (drone_b, handle) in accept_handles {
        if let Ok(Ok(IoOutput::NewConnectionAccepted { connection_token, .. })) = handle.recv_result() {
            server_tokens.entry(drone_b).or_default().push(connection_token);
        }
    }

    // Now, match them up to build the final connection map.
    // This is a simplification; a real system would need a more robust matching mechanism.
    for (drone_a, drone_b) in &connection_pairs {
         if let (Some(client_tok), Some(server_toks)) = (client_tokens.get(&(*drone_a, *drone_b)), server_tokens.get_mut(drone_b)) {
            if let Some(server_tok) = server_toks.pop() {
                connections.insert((*drone_a, *drone_b), (*client_tok, server_tok));
                 println!("    ✓ Connection established: {} -> {} (tokens: C={}, S={})", drone_a, drone_b, client_tok, server_tok);
            }
        }
    }
    
    connections
}


// --- RUN OPERATIONS & HELPERS ---
// The rest of the functions need to be adapted to take the `drones` map and the new `ConnectionMap`.
// The logic inside them (submitting CPU tasks) remains largely the same, but the I/O submission
// part needs to carefully select the correct drone's system and the correct token.

pub fn run_drone_operations_tcp(drones: &HashMap<u32, TcpDrone>, connections: &ConnectionMap, rng: &mut OmegaRng) {
    // let mut operation_handles = Vec::new();

    // for i in 0..20 {
    //     let op_type = i % 3;
    //     let handles = match op_type {
    //         0 => run_telemetry_exchange_tcp(drones, connections, rng, i),
    //         1 => run_mission_data_processing_tcp(drones, rng, i),
    //         2 => run_emergency_alert_chain_tcp(drones, connections, rng, i),
    //         _ => unreachable!(),
    //     };

    //     if let Some(h) = handles {
    //         operation_handles.extend(h);
    //     }
    //     thread::sleep(Duration::from_millis(50));
    // }
    
    // analyze_operation_results(operation_handles);
    let start_time = Instant::now();
    let mut operation_handles = Vec::new();
    let mut i = 0; // Operation counter

    // Loop for a fixed duration, NOT a fixed number of iterations
    while start_time.elapsed() < TEST_DURATION {
        let op_type = i % 3;
        let handles = match op_type {
            0 => run_telemetry_exchange_tcp(drones, connections, rng, i),
            1 => run_mission_data_processing_tcp(drones, rng, i),
            2 => run_emergency_alert_chain_tcp(drones, connections, rng, i),
            _ => unreachable!(),
        };
        if let Some(h) = handles {
            operation_handles.extend(h);
        }

        i += 1;
        // DO NOT SLEEP! Let the loop run as fast as possible.
    }

    // Now, analyze the results of all the operations we managed to queue.
    analyze_operation_results(operation_handles); 
    start_time.elapsed();

}

fn run_telemetry_exchange_tcp(
    drones: &HashMap<u32, TcpDrone>,
    connections: &ConnectionMap,
    rng: &mut OmegaRng,
    operation_id: usize,
) -> Option<Vec<TaskHandle<Vec<u8>>>> {
    println!("  Running telemetry exchange operation {}...", operation_id);
    let connection_keys: Vec<_> = connections.keys().collect();
    if connection_keys.is_empty() { return None; }

    let &(drone_a_id, drone_b_id) = connection_keys[rng.range(0, (connection_keys.len()-1) as u64) as usize];
    let (client_token, server_token) = connections.get(&(drone_a_id, drone_b_id))?;
    let drone_a_system = &drones.get(&drone_a_id)?.system;
    let drone_b_system = &drones.get(&drone_b_id)?.system;

    let telemetry_data = create_telemetry_data(drone_a_id, operation_id);

    // Drone B (server) listens
    let _ = drone_b_system.submit_io_task(IoOp::TcpReceive { connection_token: *server_token, max_bytes: 1024 }, Some(NETWORK_TIMEOUT_SHORT), rng);
    
    // Drone A (client) sends
    let _ = drone_a_system.submit_io_task(IoOp::TcpSend { connection_token: *client_token, data: telemetry_data.clone() }, Some(NETWORK_TIMEOUT_SHORT), rng);

    // The CPU task can be submitted to any drone's system.
    let cpu_handle = drone_a_system.submit_cpu_task(
        Priority::Normal,
        TELEMETRY_PROCESSING_ITERATIONS,
        move || { /* Hashing logic */ Ok(telemetry_data) },
        rng,
    ).ok()?;

    println!("    → Telemetry processing queued for drones {} -> {}", drone_a_id, drone_b_id);
    Some(vec![cpu_handle])
}

// Other run_* functions (mission_data, heartbeat, emergency) would be refactored similarly,
// carefully picking the correct system and token for each I/O operation.
// I've omitted them for brevity, but the pattern established in run_telemetry_exchange_tcp applies.
// The analyze_operation_results and create_*_data functions remain unchanged.
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
pub fn analyze_operation_results(handles: Vec<TaskHandle<Vec<u8>>>) {
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
fn run_mission_data_processing_tcp(
    drones: &HashMap<u32, TcpDrone>,
    rng: &mut OmegaRng,
    operation_id: usize,
) -> Option<Vec<TaskHandle<Vec<u8>>>> {
    println!(
        "  Running mission data processing operation {}...",
        operation_id
    );

    // This simulates a more complex operation with chained CPU tasks
    let mission_data = create_mission_data(operation_id);
    let drone_id = rng.range(0,(drones.len()-1) as u64) as u32;

    let system = &drones.get(&drone_id).unwrap().system;
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
                    hasher.update(i.to_le_bytes());
                    processed = hasher.finalize_reset().to_vec();
                }

                // Simulate validation failure (10% chance)
                if processed[0] % 10 == 0 {
                    return Err(TaskError::ExecutionFailed(
                        "Mission data validation failed".into(),
                    ));
                }

                Ok(processed)
            }
        },
        rng,
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
                    hasher.update(i.to_le_bytes());
                    processed = hasher.finalize_reset().to_vec();
                }

                Ok(processed)
            }
        },
        rng,
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

fn run_heartbeat_with_timeout_tcp(
    drones: &HashMap<u32, TcpDrone>,
    connections: &ConnectionMap,
    rng: &mut OmegaRng,
    operation_id: usize,
) -> Option<Vec<TaskHandle<Vec<u8>>>> {
    println!(
        "  Running heartbeat with timeout operation {}...",
        operation_id
    );

    // Find a connection
    let connection_keys: Vec<_> = connections.keys().collect();
    if connection_keys.is_empty() {
        return None;
    }

    let &(drone_a, drone_b) =
        connection_keys[rng.range(0, (connection_keys.len() - 1) as u64) as usize];
    let (connection_token_a, connection_token_b) = connections.get(&(drone_a, drone_b)).unwrap();
    let drone_a_system = &drones.get(&drone_a).unwrap().system;
    let drone_b_system = &drones.get(&drone_b).unwrap().system;


    // Send heartbeat
    let heartbeat_data = create_heartbeat_data(drone_a);
    let _send_result = drone_a_system.submit_io_task(
        IoOp::TcpSend {
            connection_token: *connection_token_a,
            data: heartbeat_data,
        },
        Some(NETWORK_TIMEOUT_SHORT),
        rng,
    );

    // Try to receive response with very short timeout (likely to fail)
    let receive_handle = drone_b_system.submit_io_task(
        IoOp::TcpReceive {
            connection_token: *connection_token_b,
            max_bytes: 1024,
        },
        Some(Duration::from_millis(50)), // Very short timeout
        rng,
    );

    // Process heartbeat response if received
    if let Ok(io_handle) = receive_handle {
        let cpu_handle = drone_a_system.submit_cpu_task(
            Priority::Low,
            1000, // Light processing
            move || -> Result<Vec<u8>, TaskError> {
                // This will likely not run due to I/O timeout
                Ok(b"heartbeat_processed".to_vec())
            },
            rng,
        );

        if let Ok(handle) = cpu_handle {
            println!(
                "    → Heartbeat with expected timeout queued {} -> {}",
                drone_a, drone_b
            );
            return Some(vec![handle]);
        }
    }

    println!("    ✗ Heartbeat operation setup failed");
    None
}

fn run_emergency_alert_chain_tcp(
    drones: &HashMap<u32, TcpDrone>,
    connections: &ConnectionMap,
    rng: &mut OmegaRng,
    operation_id: usize,
) -> Option<Vec<TaskHandle<Vec<u8>>>> {
    println!(
        "  Running emergency alert chain operation {}...",
        operation_id
    );
    if drones.is_empty() {
        eprintln!("No drones available for emergency alert chain operation");
        return None;
    }
    
    // Get all available drone IDs and select one randomly
    let drone_ids: Vec<u32> = drones.keys().cloned().collect();
    println!("Available drone IDs: {:?}", drone_ids);
    
    if drone_ids.is_empty() {
        eprintln!("No drone IDs available!");
        return None;
    }
    
    // Select a random index safely
    let idx = if drone_ids.len() == 1 {
        0
    } else {
        rng.range(0, drone_ids.len() as u64 - 1) as usize
    };
    
    println!("Selected index: {}", idx);
    let drone_id = *drone_ids.get(idx).expect(&format!(
        "Failed to get drone ID at index {} (total: {})", 
        idx, 
        drone_ids.len()
    ));
    
    println!("Selected drone ID: {}", drone_id);
    let drone = drones.get(&drone_id).expect("Failed to get drone system");

    // This simulates a high-priority emergency that needs to be processed and broadcast
    let emergency_data = create_emergency_data(operation_id);

    // High-priority CPU task for emergency processing
    let emergency_handle = drone.system.submit_cpu_task(
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
                    hasher.update(i.to_le_bytes());
                    processed = hasher.finalize_reset().to_vec();
                }

                // Emergency processing is critical - low failure rate (2%)
                if processed[0] % 50 == 0 {
                    return Err(TaskError::ExecutionFailed(
                        "Critical emergency processing failure".into(),
                    ));
                }

                Ok(processed)
            }
        },
        rng,
    );

    // Try to broadcast to multiple connections (fire-and-forget I/O)
    let mut broadcast_count = 0;
    
    // Find all connections that originate from the current drone
    for ((src_id, _), &(client_token, _)) in connections {
        if *src_id == drone_id && broadcast_count < 3 {
            if let Err(e) = drone.system.submit_io_task(
                IoOp::TcpSend {
                    connection_token: client_token,
                    data: emergency_data.clone(),
                },
                Some(NETWORK_TIMEOUT_SHORT),
                rng,
            ) {
                eprintln!("Failed to submit broadcast task: {:?}", e);
            } else {
                broadcast_count += 1;
            }
        }
    }

    match emergency_handle {
        Ok(handle) => {
            println!(
                "    → Emergency alert processing queued (broadcast to {} connections)",
                broadcast_count
            );
            Some(vec![handle])
        }
        Err(e) => {
            println!("    ✗ Failed to submit emergency processing task: {:?}", e);
            None
        }
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