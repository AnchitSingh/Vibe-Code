//! Provides a live monitoring dashboard for the CPU Circulatory System.
//!
//! This module implements a `Monitor` that periodically collects and displays
//! real-time statistics about the `UltraOmegaSystem`, including task submission
//! progress and the state of individual `OmegaNode`s.

use crate::node::OmegaNode;
use crate::ultra_omega::SharedSubmitterStats;
use omega::omega_timer::elapsed_ns;
use std::io::{stdout, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

/// The interval at which the monitor dashboard refreshes, in milliseconds.
const MONITOR_REFRESH_INTERVAL: Duration = Duration::from_millis(250);
/// The maximum number of `OmegaNode`s to display in the dashboard.
const MAX_NODES_TO_DISPLAY: usize = 24;

/// Represents the state and control for the system's live monitor thread.
///
/// The `Monitor` is responsible for spawning a background thread that
/// periodically updates a console-based dashboard with system metrics.
pub struct Monitor {
    /// An atomic flag used to signal the monitor thread to stop.
    is_done: Arc<AtomicBool>,
}

impl Monitor {
    /// Spawns a new thread that periodically collects and displays system statistics.
    ///
    /// The monitor thread will run in the background until `Monitor::stop()` is called.
    /// It clears the console and prints a fresh dashboard at each refresh interval.
    ///
    /// # Arguments
    ///
    /// * `nodes` - An `Arc` to a vector of `OmegaNode`s to monitor.
    /// * `stats` - An `Arc` to `SharedSubmitterStats` for system-wide submission metrics.
    /// * `start_time_ns` - The system's start time in nanoseconds, used for calculating uptime.
    ///
    /// # Returns
    ///
    /// A `Monitor` instance that can be used to stop the monitoring thread.
    ///
    /// # Panics
    ///
    /// Panics if the monitor thread fails to spawn.
    pub fn start(
        nodes: Arc<Vec<Arc<OmegaNode>>>,
        stats: Arc<SharedSubmitterStats>,
        start_time_ns: u64,
    ) -> Self {
        let is_done_flag = Arc::new(AtomicBool::new(false));
        let monitor_handle = {
            let done_clone = Arc::clone(&is_done_flag);
            thread::Builder::new()
                .name("live-monitor".to_string())
                .spawn(move || {
                    while !done_clone.load(Ordering::Relaxed) {
                        print_dashboard(&nodes, &stats, start_time_ns);
                        thread::sleep(MONITOR_REFRESH_INTERVAL);
                    }
                    // Final clear screen after the monitor thread finishes.
                    print!("\x1B[2J\x1B[H"); // ANSI codes: Clear screen and move cursor to top-left
                    stdout().flush().unwrap();
                })
                .expect("Failed to spawn monitor thread")
        };
        // The handle is stored but not explicitly joined here; it will exit when `is_done` is true.
        let _ = monitor_handle;

        Monitor {
            is_done: is_done_flag,
        }
    }

    /// Signals the monitor thread to stop and attempts to clean up the console screen.
    ///
    /// This method sets the internal `is_done` flag, causing the monitor thread's
    /// loop to terminate. A small delay is added to allow the thread to exit gracefully
    /// and perform its final screen clear.
    pub fn stop(&self) {
        self.is_done.store(true, Ordering::Relaxed);
        // Give the monitor thread a moment to receive the signal and exit its loop.
        thread::sleep(MONITOR_REFRESH_INTERVAL.saturating_add(Duration::from_millis(50)));
    }
}

/// Prints the live system dashboard to the console.
///
/// This function calculates and formats various system metrics, including uptime,
/// task submission statistics, and individual node details (queue length, threads, pressure).
/// It uses ANSI escape codes to clear the screen and position the cursor for a live update effect.
///
/// # Arguments
///
/// * `nodes` - A slice of `Arc<OmegaNode>` representing the nodes to display.
/// * `stats` - A reference to `SharedSubmitterStats` for system-wide metrics.
/// * `start_time_ns` - The system's start time in nanoseconds.
fn print_dashboard(nodes: &[Arc<OmegaNode>], stats: &SharedSubmitterStats, start_time_ns: u64) {
    let mut buffer = String::with_capacity(4096);

    // ANSI codes: Clear screen and move cursor to top-left.
    buffer.push_str("\x1B[2J\x1B[H");

    // --- Header ---
    let elapsed_ns = elapsed_ns().saturating_sub(start_time_ns);
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

    // --- Node Stats Header ---
    buffer.push_str(&format!(
        "{:<4} | {:<6} | {:<16} | {:<16} | {}\n",
        "Node", "Type", "Queue (Cur/Cap)", "Threads (Act/Des/Max)", "Pressure Bar"
    ));
    buffer.push_str(&"-".repeat(80)); // Separator line
    buffer.push('\n');

    // --- Individual Node Stats ---
    for (i, node) in nodes.iter().take(MAX_NODES_TO_DISPLAY).enumerate() {
        let q_len = node.task_queue.len();
        let q_cap = node.task_queue.capacity();
        let pressure = node.get_pressure();
        let max_p = node.max_pressure();
        let active_t = node.active_threads();
        let desired_t = node.desired_threads();

        // Determine node type based on its configured max threads (a simple heuristic).
        let node_type = if node.max_threads > 4 { "SUPER" } else { "NORM" };

        let bar = render_pressure_bar(pressure, max_p);
        let queue_str = format!("{}/{}", q_len, q_cap);
        let threads_str = format!("{}/{}/{}", active_t, desired_t, node.max_threads);

        buffer.push_str(&format!(
            "#{:<3} | {:<6} | {:<16} | {:<16} | {}\n",
            i, node_type, queue_str, threads_str, bar
        ));
    }
    // Indicate if there are more nodes than displayed.
    if nodes.len() > MAX_NODES_TO_DISPLAY {
        buffer.push_str(&format!(
            "... and {} more nodes not shown.\n",
            nodes.len() - MAX_NODES_TO_DISPLAY
        ));
    }

    // --- Print to stdout ---
    print!("{}", buffer);
    let _ = stdout().flush(); // Ensure the buffer is written to the console.
}

/// Renders a text-based pressure bar with color coding.
///
/// The bar visually represents the current pressure as a percentage,
/// changing color based on the pressure level (green, yellow, red).
///
/// # Arguments
///
/// * `current` - The current pressure value (0-100).
/// * `max` - The maximum pressure value (typically 100).
///
/// # Returns
///
/// A `String` containing the formatted pressure bar with ANSI color codes.
fn render_pressure_bar(current: usize, max: usize) -> String {
    const BAR_WIDTH: usize = 30;
    let ratio = if max == 0 { 0.0 } else { current as f32 / max as f32 };
    let filled_width = (ratio * BAR_WIDTH as f32).round() as usize;

    let bar: String = "█".repeat(filled_width) + &"-".repeat(BAR_WIDTH.saturating_sub(filled_width));

    // ANSI color codes for pressure levels.
    let color = if ratio > 0.85 {
        "\x1B[91m" // Bright Red for high pressure
    } else if ratio > 0.6 {
        "\x1B[93m" // Bright Yellow for medium pressure
    } else {
        "\x1B[92m" // Bright Green for low pressure
    };
    let reset = "\x1B[0m"; // ANSI reset code

    format!("[{}{}{}] {:>3.0}%", color, bar, reset, ratio * 100.0)
}