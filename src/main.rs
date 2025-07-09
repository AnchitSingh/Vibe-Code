use std::time::{Duration, Instant};
use omega::omega_timer::timer_init;
use ultra_omega::{VibeSystem, Job, collect};
use ovp::DroneId;
use serde_json::json;
use omega::omega_timer::elapsed_ns;

// --- Test Constants ---
const NUM_DRONES: usize = 5;
const NETWORK_INTERFACE: &str = "lo";
const TEST_DURATION: Duration = Duration::from_secs(2);
const TELEMETRY_PROCESSING_ITERATIONS: u32 = 25_000;
const MISSION_DATA_PROCESSING_ITERATIONS: u32 = 75_000;

// --- Independent Drone (Each Has Its Own Circulatory System) ---
struct VibeDrone {
    id: DroneId,
    system: VibeSystem,  // Each drone = independent system (like real hardware)
    socket: ultra_omega::vibe::OvpSocket,
}

impl VibeDrone {
    /// Create a new independent drone with its own circulatory system
    fn new(id: DroneId) -> Self {
        let system = VibeSystem::new();  // Independent system per drone
        let socket = system.ovp_socket(NETWORK_INTERFACE, id);
        println!("✓ Drone {} online with independent system", id);
        
        Self { id, system, socket }
    }
    
    /// Send telemetry to another drone and process it locally
    fn send_telemetry_and_process(&self, target_id: DroneId, operation_id: usize) -> Option<Job<Vec<u8>>> {
        let telemetry_data = create_telemetry_data(self.id, operation_id);
        
        // Fire-and-forget send (no manual threads - trust the Mona Lisa!)
        self.socket.send_to(vec![target_id], &telemetry_data);
        
        // Process the telemetry data locally
        let processing_job = self.system.run(move |data| {
            // Simulate telemetry processing
            let mut processed = data;
            for _ in 0..TELEMETRY_PROCESSING_ITERATIONS {
                processed[0] = processed[0].wrapping_add(1);
            }
            processed
        }, telemetry_data);
        
        Some(processing_job)
    }
    
    /// Process mission data locally
    fn process_mission_data(&self, operation_id: usize) -> Option<Job<Vec<u8>>> {
        let mission_data = create_mission_data(operation_id);
        
        let job = self.system.run(move |data| {
            // Simulate heavy mission processing
            let mut processed = data;
            for _ in 0..MISSION_DATA_PROCESSING_ITERATIONS {
                processed[0] = processed[0].wrapping_add(1);
            }
            processed
        }, mission_data);
        
        Some(job)
    }
    
    /// Broadcast emergency and process it locally
    fn broadcast_emergency_and_process(&self, operation_id: usize) -> Option<Job<Vec<u8>>> {
        let emergency_data = create_emergency_data(operation_id);
        
        // Fire-and-forget broadcast (trust the system!)
        self.socket.broadcast(&emergency_data);
        
        // Process emergency data with high priority
        let processing_job = self.system.run(move |data| {
            // Simulate emergency processing
            let mut processed = data;
            for _ in 0..TELEMETRY_PROCESSING_ITERATIONS * 2 {
                processed[0] = processed[0].wrapping_add(1);
            }
            // Simulate potential emergency processing failure
            if processed[0] % 50 == 0 {
                panic!("Emergency processing failed"); // Will be caught by system
            }
            processed
        }, emergency_data);
        
        Some(processing_job)
    }
    
    /// Check for incoming messages (non-blocking)
    fn check_messages(&self) -> Option<Vec<u8>> {
        self.socket.try_message()
    }
}

fn main() {
    timer_init().expect("Failed to initialize timer");
    
    println!("=== VIBE DRONE SWARM TEST (PHYSICS-RESPECTING) ===");
    println!("Creating {} independent drones...\n", NUM_DRONES);
    
    // Create independent drones (each with own circulatory system)
    let drones: Vec<VibeDrone> = (1..=NUM_DRONES)
        .map(|i| VibeDrone::new(i as DroneId))
        .collect();
    
    println!("\n🚀 Starting distributed operations for {:?}...\n", TEST_DURATION);
    
    let start_time = Instant::now();
    let mut operation_count = 0;
    let mut processing_jobs = Vec::new();
    let mut message_check_counter = 0;
    
    // Run operations until time expires
    while start_time.elapsed() < TEST_DURATION {
        let operation_type = operation_count % 3;
        
        // Distribute operations across drones (like the original test)
        let primary_drone_idx = operation_count % drones.len();
        let secondary_drone_idx = (operation_count + 1) % drones.len();
        
        let job = match operation_type {
            0 => {
                // Telemetry exchange between different drones
                let sender = &drones[primary_drone_idx];
                let receiver_id = drones[secondary_drone_idx].id;
                
                println!("  📡 Drone {} sending telemetry to Drone {}", sender.id, receiver_id);
                sender.send_telemetry_and_process(receiver_id, operation_count)
            },
            1 => {
                // Mission data processing
                let drone = &drones[primary_drone_idx];
                println!("  🧠 Drone {} processing mission data", drone.id);
                drone.process_mission_data(operation_count)
            },
            2 => {
                // Emergency alert
                let drone = &drones[primary_drone_idx];
                println!("  🚨 Drone {} broadcasting emergency", drone.id);
                drone.broadcast_emergency_and_process(operation_count)
            },
            _ => unreachable!(),
        };
        
        if let Some(job) = job {
            processing_jobs.push(job);
        }
        
        operation_count += 1;
        
        // Periodically check for incoming messages
        message_check_counter += 1;
        if message_check_counter % 10 == 0 {
            for drone in &drones {
                if let Some(message) = drone.check_messages() {
                    println!("  📨 Drone {} received: {} bytes", drone.id, message.len());
                }
            }
        }
    }
    
    println!("\n⏰ Time's up! Collecting results from {} jobs...", processing_jobs.len());
    
    // Collect all processing results (blocks until complete)
    let start_collect = Instant::now();
    let results = collect(processing_jobs);
    let collect_time = start_collect.elapsed();
    
    // Final message check
    println!("\n📬 Final message check...");
    for drone in &drones {
        let mut message_count = 0;
        while let Some(_message) = drone.check_messages() {
            message_count += 1;
            if message_count >= 5 { break; } // Don't spam output
        }
        if message_count > 0 {
            println!("  📨 Drone {} had {} pending messages", drone.id, message_count);
        }
    }
    
    // Calculate and display results
    let total_duration = start_time.elapsed();
    let ops_per_second = operation_count as f64 / total_duration.as_secs_f64();
    
    println!("\n=== VIBE DRONE SWARM RESULTS ===");
    println!("Operations submitted: {}", operation_count);
    println!("Processing jobs completed: {}", results.len());
    println!("Operation duration: {:?}", total_duration);
    println!("Collection duration: {:?}", collect_time);
    
    // Check for any processing failures
    let successful_jobs = results.len();
    let failed_jobs = operation_count - successful_jobs;
    if failed_jobs > 0 {
        println!("Failed jobs: {} (emergency processing failures)", failed_jobs);
    }
    
    let success_rate = (successful_jobs as f64 / operation_count as f64) * 100.0;
    println!("Success rate: {:.1}%", success_rate);
    println!("Throughput: {:.0} ops/sec", ops_per_second);
    
    // Performance evaluation
    if ops_per_second > 35000.0 {
        println!("🚀 PHENOMENAL! {} ops/sec - Mona Lisa performance!", ops_per_second as usize);
    } else if ops_per_second > 25000.0 {
        println!("🎉 EXCELLENT! {} ops/sec - Close to Mona Lisa!", ops_per_second as usize);
    } else if ops_per_second > 15000.0 {
        println!("👍 GOOD! {} ops/sec - Respectable performance", ops_per_second as usize);
    } else {
        println!("🤔 SLOW... {} ops/sec - Something's wrong", ops_per_second as usize);
    }
    
    if success_rate > 90.0 {
        println!("✅ DRONE SWARM TEST PASSED - Physics respected, Mona Lisa preserved!");
    } else {
        println!("❌ DRONE SWARM NEEDS OPTIMIZATION");
    }
}

// --- Data Creation Functions (Unchanged from Original) ---

fn create_telemetry_data(drone_id: DroneId, operation_id: usize) -> Vec<u8> {
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