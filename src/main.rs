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
const CURRENT_SCENARIO: TestScenario = TestScenario::HeterogeneousNodes;

const NUM_NODES_CONF: usize = 8;
const NUM_SUPER_NODES_CONF: usize = 2;
const NUM_SUBMITTER_THREADS_CONF: usize = 16;
const TOTAL_TASKS_PER_SUBMITTER_CONF: usize = 50;
const AVG_TASK_PROCESSING_MS_CONF: u64 = 75;
const TASK_PROCESSING_VARIABILITY_MS_CONF: u64 = 50;
const SUBMISSION_DELAY_MS_CONF: u64 = 5;
const SUBMISSION_DELAY_VARIABILITY_MS_CONF: u64 = 5;
const SYSTEM_MAXED_OUT_BACKOFF_MS_CONF: u64 = 250;
const MAX_SUBMISSION_RETRIES_CONF: usize = 10;
const MONITORING_INTERVAL_MS_CONF: u64 = 1000;
const POST_SUBMISSION_STABILIZATION_S_CONF: u64 = 20;
const TASK_FAILURE_PROBABILITY: f64 = 0.05;




use std::net::SocketAddr;

fn main() {
    omega_timer_init();
    println!("--- Starting Ultra-Ω I/O Echo Test ---");

    // 1. Setup the system
    let nodes_vec: Vec<Arc<OmegaNode>> = (0..2).map(|i| {
        Arc::new(OmegaNode::new(NodeId::new(), 10, 1, 2, mpsc::channel().0, None).unwrap())
    }).collect();
    
    let omega_system = Arc::new(UltraOmegaSystem::new(nodes_vec));
    let mut rng = OmegaRng::new(42);
    let listen_addr: SocketAddr = "127.0.0.1:9999".parse().unwrap();

    // 1. Start the TCP Listener
    println!("Step 1: Starting TCP listener on {}", listen_addr);
    let listener_handle = omega_system.submit_io_task(
        IoOp::TcpListen { addr: listen_addr },
        &mut rng
    ).expect("Failed to submit listener task");

    // 2. Start the Client and Connect
    println!("Step 2: Submitting client connect task...");
    let client_connect_handle = omega_system.submit_io_task(
        IoOp::TcpConnect { peer_addr: listen_addr },
        &mut rng
    ).expect("Failed to submit connect task");
    
    // In a real app, you might do other things here. For the test, we'll wait.
    thread::sleep(Duration::from_millis(100)); // Give reactor time to process

    // 3. Wait for the listener to accept the connection
    println!("Step 3: Awaiting listener to accept connection...");
    let server_token = match listener_handle.recv_result().unwrap().unwrap() {
        IoOutput::NewConnectionAccepted { connection_token, .. } => {
            println!("   -> Listener accepted new connection! Token: {}", connection_token);
            connection_token
        }
        output => panic!("Expected NewConnectionAccepted, but got {:?}", output),
    };

    // 4. Wait for the connection to be established from the client's perspective
    println!("Step 4: Awaiting client connection result...");
    let client_token = match client_connect_handle.recv_result().unwrap().unwrap() {
        IoOutput::TcpConnectionEstablished { connection_token, .. } => {
            println!("   -> Client connected successfully! Token: {}", connection_token);
            connection_token
        }
        output => panic!("Expected TcpConnectionEstablished, but got {:?}", output),
    };

    // 5. Server prepares to receive a message
    println!("Step 5: Server submitted receive task...");
    let server_recv_handle = omega_system.submit_io_task(
        IoOp::TcpReceive { connection_token: server_token, max_bytes: 1024 },
        &mut rng
    ).unwrap();

    // 6. Client sends a message
    let message_to_send = b"Hello Omega!".to_vec();
    println!("Step 6: Client sending message: '{}'", String::from_utf8_lossy(&message_to_send));
    let client_send_handle = omega_system.submit_io_task(
        IoOp::TcpSend { connection_token: client_token, data: message_to_send.clone() },
        &mut rng
    ).unwrap();
    client_send_handle.recv_result().unwrap().unwrap(); // Wait for send confirmation
    println!("   -> Client send confirmed.");

    // 7. Server waits for the data, then echos it back
    println!("Step 7: Server awaiting data to echo...");
    match server_recv_handle.recv_result().unwrap().unwrap() {
        IoOutput::TcpDataReceived { data } => {
            println!("   -> Server received: '{}'", String::from_utf8_lossy(&data));
            assert_eq!(data, message_to_send);
            
            println!("   -> Server echoing data back...");
            let server_echo_handle = omega_system.submit_io_task(
                IoOp::TcpSend { connection_token: server_token, data: data },
                &mut rng
            ).unwrap();
            server_echo_handle.recv_result().unwrap().unwrap();
            println!("   -> Server echo confirmed.");
        }
        output => panic!("Expected TcpDataReceived, but got {:?}", output),
    }

    // 8. Client prepares to receive the echo
    println!("Step 8: Client submitted receive task for echo...");
    let client_recv_echo_handle = omega_system.submit_io_task(
        IoOp::TcpReceive { connection_token: client_token, max_bytes: 1024 },
        &mut rng
    ).unwrap();

    // 9. Client waits for and verifies the echo
    println!("Step 9: Client awaiting echo...");
    match client_recv_echo_handle.recv_result().unwrap().unwrap() {
        IoOutput::TcpDataReceived { data } => {
            println!("   -> Client received echo: '{}'", String::from_utf8_lossy(&data));
            assert_eq!(data, message_to_send);
        }
        output => panic!("Expected TcpDataReceived, but got {:?}", output),
    }

    println!("\n--- I/O ECHO TEST SUCCEEDED! ---");
    
    // 11. Clean shutdown
    omega_system.shutdown_all();
    println!("System shut down gracefully.");
}
