// src/monitor.rs

use crate::node::OmegaNode;
use omega::omega_timer::omega_time_ns; // UPDATED: Import omega_time_ns
use std::io::{stdout, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use crate::ultra_omega::SharedSubmitterStats;
use std::thread;
use std::time::Duration; // Kept for thread::sleep, which is correct

const MONITOR_REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const MAX_NODES_TO_DISPLAY: usize = 24;

/// Represents the state of the monitor thread.
pub struct Monitor {
    is_done: Arc<AtomicBool>,
}

impl Monitor {
    /// Spawns a new thread that periodically collects and displays system stats.
    pub fn start(
        nodes: Arc<Vec<Arc<OmegaNode>>>,
        stats: Arc<SharedSubmitterStats>,
        // UPDATED: start_time is now a u64 nanosecond timestamp
        start_time_ns: u64,
    ) -> Self {
        let is_done_flag = Arc::new(AtomicBool::new(false));
        let monitor_handle = {
            let done_clone = Arc::clone(&is_done_flag);
            thread::Builder::new()
                .name("live-monitor".to_string())
                .spawn(move || {
                    while !done_clone.load(Ordering::Relaxed) {
                        // UPDATED: Pass the u64 timestamp
                        print_dashboard(&nodes, &stats, start_time_ns);
                        thread::sleep(MONITOR_REFRESH_INTERVAL);
                    }
                    // Final clear screen after finishing
                    print!("\x1B[2J\x1B[H");
                    stdout().flush().unwrap();
                })
                .expect("Failed to spawn monitor thread")
        };
        // We don't need to join the handle in this design, it will stop when the flag is set.
        let _ = monitor_handle;

        Monitor {
            is_done: is_done_flag,
        }
    }

    /// Signals the monitor thread to stop and cleans up the screen.
    pub fn stop(&self) {
        self.is_done.store(true, Ordering::Relaxed);
        // Give it a moment to receive the signal and exit its loop
        thread::sleep(MONITOR_REFRESH_INTERVAL.saturating_add(Duration::from_millis(50)));
    }
}

fn print_dashboard(
    nodes: &[Arc<OmegaNode>],
    stats: &SharedSubmitterStats,
    // UPDATED: start_time is now a u64 nanosecond timestamp
    start_time_ns: u64,
) {
    let mut buffer = String::with_capacity(4096);

    // ANSI codes: Clear screen and move cursor to top-left
    buffer.push_str("\x1B[2J\x1B[H");

    // --- Header ---
    // UPDATED: Calculate elapsed time using omega_time_ns
    let elapsed_ns = omega_time_ns().saturating_sub(start_time_ns);
    let elapsed_ms_total = elapsed_ns / 1_000_000;
    let elapsed_secs_total = elapsed_ms_total / 1000;

    let display_mins = elapsed_secs_total / 60;
    let display_secs = elapsed_secs_total % 60;
    let display_ms = elapsed_ms_total % 1000;

    buffer.push_str(&format!(
        "--- Ultra-Ω System Live Monitor --- Running for: {:02}:{:02}.{:03} ---\n\n",
        display_mins, display_secs, display_ms
    ));

    // --- Submission Stats ---
    buffer.push_str("Submission Progress:\n");
    let attempted = stats.tasks_attempted_submission.load(Ordering::Relaxed);
    let successful = stats.tasks_successfully_submitted.load(Ordering::Relaxed);
    let failed = stats
        .tasks_failed_submission_max_retries
        .load(Ordering::Relaxed);
    buffer.push_str(&format!(
        "  Submitted: {:<7} | Retries Failed: {:<7} | Attempted: {:<7}\n\n",
        successful, failed, attempted
    ));

    // --- Node Stats ---
    buffer.push_str(&format!(
        "{:<4} | {:<6} | {:<16} | {:<16} | {}\n",
        "Node", "Type", "Queue (Cur/Cap)", "Threads (Act/Des/Max)", "Pressure Bar"
    ));
    buffer.push_str(&"-".repeat(80));
    buffer.push('\n');

    for (i, node) in nodes.iter().take(MAX_NODES_TO_DISPLAY).enumerate() {
        let q_len = node.task_queue.len();
        let q_cap = node.task_queue.capacity();
        let pressure = node.get_pressure();
        let max_p = node.max_pressure();
        let active_t = node.active_threads();
        let desired_t = node.desired_threads();

        // Determine node type based on its configuration
        let node_type = if node.max_threads > 3 { "SUPER" } else { "NORM" };

        let bar = render_pressure_bar(pressure, max_p);
        let queue_str = format!("{}/{}", q_len, q_cap);
        let threads_str = format!("{}/{}/{}", active_t, desired_t, node.max_threads);

        buffer.push_str(&format!(
            "#{:<3} | {:<6} | {:<16} | {:<16} | {}\n",
            i, node_type, queue_str, threads_str, bar
        ));
    }
    if nodes.len() > MAX_NODES_TO_DISPLAY {
        buffer.push_str(&format!(
            "... and {} more nodes not shown.\n",
            nodes.len() - MAX_NODES_TO_DISPLAY
        ));
    }

    // --- Print to stdout ---
    print!("{}", buffer);
    let _ = stdout().flush();
}

fn render_pressure_bar(current: usize, max: usize) -> String {
    const BAR_WIDTH: usize = 30;
    let ratio = if max == 0 { 0.0 } else { current as f32 / max as f32 };
    let filled_width = (ratio * BAR_WIDTH as f32).round() as usize;

    let bar: String = "█".repeat(filled_width)
        + &"-".repeat(BAR_WIDTH.saturating_sub(filled_width));
    
    // ANSI color codes
    let color = if ratio > 0.85 {
        "\x1B[91m" // Bright Red
    } else if ratio > 0.6 {
        "\x1B[93m" // Bright Yellow
    } else {
        "\x1B[92m" // Bright Green
    };
    let reset = "\x1B[0m";

    format!("[{}{}{}] {:>3.0}%", color, bar, reset, ratio * 100.0)
}