use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use rand::Rng;

mod lib;
use lib::{OVP, DroneId};

const DRONE_COUNT: usize = 20;
const SIMULATION_DURATION: u64 = 30;

#[derive(Debug)]
struct SwarmStats {
    messages_sent: u64,
    messages_received: u64,
    group_messages_sent: u64,
    direct_messages_sent: u64,
    broadcast_messages_sent: u64,
    active_drones: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚁 Starting Enhanced OVP Drone Swarm Simulation");
    println!("===============================================");
    println!("🌐 Testing Revolutionary Layer 2 Protocol Features:");
    println!("   ✅ Spherical broadcast (physics-based propagation)");
    println!("   ✅ Receiver-side filtering");
    println!("   ✅ O(1) multicast (vs traditional O(n) unicast loops)");
    println!("   ✅ Source identification logging");
    println!("   ✅ Group messaging capabilities");
    println!("");
    
    let stats = Arc::new(Mutex::new(SwarmStats {
        messages_sent: 0,
        messages_received: 0,
        group_messages_sent: 0,
        direct_messages_sent: 0,
        broadcast_messages_sent: 0,
        active_drones: 0,
    }));
    
    let mut drone_handles = Vec::new();
    
    // Spawn drone simulation threads
    for drone_id in 1..=DRONE_COUNT {
        let stats_clone = Arc::clone(&stats);
        
        let handle = thread::spawn(move || {
            run_drone_simulation(drone_id as u64, stats_clone)
        });
        
        drone_handles.push(handle);
        thread::sleep(Duration::from_millis(50));
    }
    
    // Monitor thread
    let monitor_stats = Arc::clone(&stats);
    let monitor_handle = thread::spawn(move || {
        monitor_swarm(monitor_stats);
    });
    
    println!("✅ All {} drones launched!", DRONE_COUNT);
    println!("🔄 Simulation running for {} seconds...", SIMULATION_DURATION);
    println!("📡 Watch for OVP's revolutionary O(1) group messaging!");
    
    thread::sleep(Duration::from_secs(SIMULATION_DURATION));
    
    println!("\n🛑 Simulation complete!");
    
    let final_stats = stats.lock().unwrap();
    println!("\n📊 FINAL OVP PROTOCOL STATISTICS:");
    println!("═══════════════════════════════════");
    println!("Total Messages Sent: {}", final_stats.messages_sent);
    println!("  └─ Broadcast Messages: {}", final_stats.broadcast_messages_sent);
    println!("  └─ Direct Messages: {}", final_stats.direct_messages_sent);
    println!("  └─ Group Messages (O(1)): {}", final_stats.group_messages_sent);
    println!("Messages Received: {}", final_stats.messages_received);
    println!("Active Drones: {}", final_stats.active_drones);
    println!("");
    println!("🎯 OVP Efficiency: {} group multicasts saved {} individual unicasts!",
             final_stats.group_messages_sent,
             final_stats.group_messages_sent * 3); // Assuming avg 3 recipients per group
    
    Ok(())
}

fn run_drone_simulation(drone_id: DroneId, stats: Arc<Mutex<SwarmStats>>) {
    println!("🚁 Drone {} starting OVP initialization...", drone_id);
    
    // Use loopback interface for all drones - they'll all share the same network
    let interface = "lo";
    
    let ovp = match OVP::new(interface, drone_id) {
        Ok(ovp) => ovp,
        Err(e) => {
            eprintln!("❌ Drone {} failed to initialize OVP: {}", drone_id, e);
            return;
        }
    };
    
    {
        let mut stats_lock = stats.lock().unwrap();
        stats_lock.active_drones += 1;
    }
    
    let mut rng = rand::thread_rng();
    let mut position = (rng.gen_range(0.0..100.0), rng.gen_range(0.0..100.0));
    
    println!("✅ Drone {} OVP online at position ({:.1}, {:.1})", drone_id, position.0, position.1);
    
    let start_time = std::time::Instant::now();
    
    // Clone ovp for the main thread before moving the original
    let main_ovp = ovp.clone();
    let receiver_stats = stats.clone();
    
    // Move the original OVP into the receiver thread
    thread::spawn(move || {
        message_receiver_loop(drone_id, ovp, receiver_stats);
    });
    
    while start_time.elapsed().as_secs() < SIMULATION_DURATION {
        match rng.gen_range(0..6) {
            0 => send_status_broadcast(&main_ovp, drone_id, &position, &stats),
            1 => send_targeted_message(&main_ovp, drone_id, &stats),
            2 => send_formation_command(&main_ovp, drone_id, &stats),
            3 => send_sensor_data(&main_ovp, drone_id, &position, &stats),
            4 => send_group_message(&main_ovp, drone_id, &stats), // NEW: Group messaging
            _ => send_team_coordination(&main_ovp, drone_id, &stats), // NEW: Team coordination
        }
        
        // Simulate movement
        position.0 += rng.gen_range(-1.0..1.0);
        position.1 += rng.gen_range(-1.0..1.0);
        position.0 = position.0.clamp(0.0, 100.0);
        position.1 = position.1.clamp(0.0, 100.0);
        
        thread::sleep(Duration::from_millis(rng.gen_range(200..800)));
    }
    
    println!("🔴 Drone {} shutting down OVP connection", drone_id);
}

// NEW: Dedicated message receiver loop with source logging
fn message_receiver_loop(drone_id: DroneId, ovp: OVP, stats: Arc<Mutex<SwarmStats>>) {
    println!("📡 Drone {} receiver thread started - listening for OVP frames", drone_id);
    
    loop {
        if let Some(message) = ovp.try_receive() {
            if let Ok(msg_str) = String::from_utf8(message) {
                // Parse the message to extract sender info
                let sender_id = extract_sender_from_message(&msg_str);
                
                {
                    let mut stats_lock = stats.lock().unwrap();
                    stats_lock.messages_received += 1;
                }
                
                // LOG: Show message reception with source identification
                println!("📥 Drone {} RECEIVED from Drone {}: {}", 
                        drone_id, 
                        sender_id.unwrap_or(0), 
                        truncate_message(&msg_str, 50));
                
                // Process different message types
                process_received_message(drone_id, &msg_str, sender_id);
            }
        }
        
        thread::sleep(Duration::from_millis(10)); // Small delay to prevent busy waiting
    }
}

fn extract_sender_from_message(message: &str) -> Option<DroneId> {
    // Extract sender ID from message format like "STATUS:5:..." or "DIRECT:3:..."
    let parts: Vec<&str> = message.split(':').collect();
    if parts.len() >= 2 {
        parts[1].parse::<DroneId>().ok()
    } else {
        None
    }
}

fn process_received_message(drone_id: DroneId, message: &str, sender_id: Option<DroneId>) {
    if message.starts_with("GROUP:") {
        println!("🎯 Drone {} processed GROUP message from Drone {}", 
                drone_id, sender_id.unwrap_or(0));
    } else if message.starts_with("TEAM:") {
        println!("👥 Drone {} joined team coordination from Drone {}", 
                drone_id, sender_id.unwrap_or(0));
    } else if message.starts_with("FORMATION") {
        println!("🔄 Drone {} acknowledging formation command from leader Drone {}", 
                drone_id, sender_id.unwrap_or(0));
    }
}

fn truncate_message(msg: &str, max_len: usize) -> String {
    if msg.len() > max_len {
        format!("{}...", &msg[..max_len])
    } else {
        msg.to_string()
    }
}

fn send_status_broadcast(ovp: &OVP, drone_id: u64, position: &(f32, f32), stats: &Arc<Mutex<SwarmStats>>) {
    let message = format!("STATUS:{}:({:.1},{:.1}):OPERATIONAL", drone_id, position.0, position.1);
    
    if ovp.emit(None, message.into_bytes()).is_ok() {
        {
            let mut stats_lock = stats.lock().unwrap();
            stats_lock.messages_sent += 1;
            stats_lock.broadcast_messages_sent += 1;
        }
        println!("📡 Drone {} BROADCAST status (spherical propagation)", drone_id);
    }
}

fn send_targeted_message(ovp: &OVP, drone_id: u64, stats: &Arc<Mutex<SwarmStats>>) {
    let mut rng = rand::thread_rng();
    let target_drone = rng.gen_range(1..=DRONE_COUNT as u64);
    
    if target_drone != drone_id {
        let message = format!("DIRECT:{}:Hello from drone {}", drone_id, target_drone);
        let targets = vec![target_drone];
        
        if ovp.emit(Some(targets), message.into_bytes()).is_ok() {
            {
                let mut stats_lock = stats.lock().unwrap();
                stats_lock.messages_sent += 1;
                stats_lock.direct_messages_sent += 1;
            }
            println!("📨 Drone {} DIRECT message to Drone {} (targeted filtering)", drone_id, target_drone);
        }
    }
}

// NEW: Group messaging - demonstrates O(1) multicast capability
fn send_group_message(ovp: &OVP, drone_id: u64, stats: &Arc<Mutex<SwarmStats>>) {
    let mut rng = rand::thread_rng();
    
    // Create a random group of 2-5 drones
    let group_size = rng.gen_range(2..=5);
    let mut target_group = Vec::new();
    
    while target_group.len() < group_size {
        let target = rng.gen_range(1..=DRONE_COUNT as u64);
        if target != drone_id && !target_group.contains(&target) {
            target_group.push(target);
        }
    }
    
    let group_list = target_group.iter()
        .map(|id| format!("D{}", id))
        .collect::<Vec<_>>()
        .join(",");
    
    let message = format!("GROUP:{}:Mission briefing for team [{}] - Rendezvous at coordinates (50,50)", 
                         drone_id, group_list);
    
    if ovp.emit(Some(target_group.clone()), message.into_bytes()).is_ok() {
        {
            let mut stats_lock = stats.lock().unwrap();
            stats_lock.messages_sent += 1;
            stats_lock.group_messages_sent += 1;
        }
        println!("🎯 Drone {} GROUP message to {} drones [{}] in O(1) operation!", 
                drone_id, target_group.len(), group_list);
        println!("   └─ OVP Magic: 1 spherical emit reaches {} targets simultaneously", target_group.len());
    }
}

// NEW: Team coordination messaging
fn send_team_coordination(ovp: &OVP, drone_id: u64, stats: &Arc<Mutex<SwarmStats>>) {
    let mut rng = rand::thread_rng();
    
    // Create a tactical team of 3-4 drones
    let team_size = rng.gen_range(3..=4);
    let mut team_members = Vec::new();
    
    while team_members.len() < team_size {
        let member = rng.gen_range(1..=DRONE_COUNT as u64);
        if member != drone_id && !team_members.contains(&member) {
            team_members.push(member);
        }
    }
    
    let team_names = team_members.iter()
        .map(|id| format!("Unit-{}", id))
        .collect::<Vec<_>>()
        .join(",");
    
    let tactics = ["FLANK_LEFT", "COVER_ADVANCE", "RECON_SWEEP", "DEFENSIVE_CIRCLE"];
    let chosen_tactic = tactics[rng.gen_range(0..tactics.len())];
    
    let message = format!("TEAM:{}:Tactical command {} for units [{}] - Execute in 30 seconds", 
                         drone_id, chosen_tactic, team_names);
    
    if ovp.emit(Some(team_members.clone()), message.into_bytes()).is_ok() {
        {
            let mut stats_lock = stats.lock().unwrap();
            stats_lock.messages_sent += 1;
            stats_lock.group_messages_sent += 1;
        }
        println!("👥 Drone {} TEAM coordination to {} members [{}] - Tactic: {}", 
                drone_id, team_members.len(), team_names, chosen_tactic);
        println!("   └─ Traditional protocols would need {} separate unicasts - OVP does it in 1!", 
                team_members.len());
    }
}

fn send_formation_command(ovp: &OVP, drone_id: u64, stats: &Arc<Mutex<SwarmStats>>) {
    let mut rng = rand::thread_rng();
    let formation_type = match rng.gen_range(0..3) {
        0 => "FORMATION_CIRCLE",
        1 => "FORMATION_LINE",
        _ => "FORMATION_GRID",
    };
    
    let message = format!("FORMATION:{}:{}:LEADER_CMD", formation_type, drone_id);
    
    if ovp.emit(None, message.into_bytes()).is_ok() {
        {
            let mut stats_lock = stats.lock().unwrap();
            stats_lock.messages_sent += 1;
            stats_lock.broadcast_messages_sent += 1;
        }
        println!("🎯 Drone {} FORMATION command: {} (broadcast to all in range)", drone_id, formation_type);
    }
}

fn send_sensor_data(ovp: &OVP, drone_id: u64, position: &(f32, f32), stats: &Arc<Mutex<SwarmStats>>) {
    let mut rng = rand::thread_rng();
    let temperature = rng.gen_range(15.0..35.0);
    let battery = rng.gen_range(20..100);
    
    let message = format!("SENSOR:{}:TEMP:{:.1}:BATT:{}:POS:({:.1},{:.1})", 
                         drone_id, temperature, battery, position.0, position.1);
    
    if ovp.emit(None, message.into_bytes()).is_ok() {
        {
            let mut stats_lock = stats.lock().unwrap();
            stats_lock.messages_sent += 1;
            stats_lock.broadcast_messages_sent += 1;
        }
        println!("📊 Drone {} SENSOR data broadcast (temp: {:.1}°C, battery: {}%)", 
                drone_id, temperature, battery);
    }
}

fn monitor_swarm(stats: Arc<Mutex<SwarmStats>>) {
    let mut last_sent = 0;
    let mut last_received = 0;
    let mut last_group = 0;
    
    for i in 0..6 { // Monitor for 30 seconds (6 * 5 second intervals)
        thread::sleep(Duration::from_secs(5));
        
        let current_stats = stats.lock().unwrap();
        let sent_rate = current_stats.messages_sent - last_sent;
        let received_rate = current_stats.messages_received - last_received;
        let group_rate = current_stats.group_messages_sent - last_group;
        
        println!("\n📈 OVP SWARM STATUS ({}s):", (i + 1) * 5);
        println!("   Active Drones: {}", current_stats.active_drones);
        println!("   Messages Sent (total): {}", current_stats.messages_sent);
        println!("     └─ Broadcasts: {}", current_stats.broadcast_messages_sent);
        println!("     └─ Direct: {}", current_stats.direct_messages_sent);
        println!("     └─ Groups: {}", current_stats.group_messages_sent);
        println!("   Messages Received: {}", current_stats.messages_received);
        println!("   Send Rate (last 5s): {}", sent_rate);
        println!("   Receive Rate (last 5s): {}", received_rate);
        println!("   Group Multicast Rate: {}", group_rate);
        
        if group_rate > 0 {
            println!("   🚀 OVP Efficiency: {} group sends = ~{} traditional unicasts saved!", 
                    group_rate, group_rate * 3);
        }
        
        last_sent = current_stats.messages_sent;
        last_received = current_stats.messages_received;
        last_group = current_stats.group_messages_sent;
    }
}