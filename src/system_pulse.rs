// src/system_pulse.rs

use crate::signals::{SystemSignal, NodeId}; // OpportunisticInfo is no longer imported

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use omega::OmegaHashSet;
// --- OmegaSystemPulse ---
pub struct OmegaSystemPulse {
    signal_rx: Receiver<SystemSignal>,
    print_interval: Duration,
    surge_overload_threshold_count: usize,

    // Internal state - simplified
    overloaded_nodes: OmegaHashSet<u64, ()>,
    all_nodes_seen: OmegaHashSet<u64, ()>,
    // Removed: sum_all_node_avg_task_time_micros
    // Removed: count_node_avg_time_reports
    total_task_completed_signals: u64, // New: count occurrences
    total_task_failed_signals: u64,    // New: count occurrences
    surge_mode_active: bool,
    last_print_time: Instant,
}

// Helper methods on SystemSignal (get_node_id) are still in signals.rs
// No get_opportunistic_info() is needed here anymore.

impl OmegaSystemPulse {
    pub fn new(
        signal_rx: Receiver<SystemSignal>,
        print_interval_ms: u64,
        surge_overload_threshold_count: usize,
    ) -> Self {
        OmegaSystemPulse {
            signal_rx,
            print_interval: Duration::from_millis(print_interval_ms),
            surge_overload_threshold_count,
            overloaded_nodes: OmegaHashSet::new_u64_map(1024),
            all_nodes_seen: OmegaHashSet::new_u64_map(1024),
            total_task_completed_signals: 0, // Initialize new counter
            total_task_failed_signals: 0,    // Initialize new counter
            surge_mode_active: false,
            last_print_time: Instant::now(),
        }
    }

    pub fn run(mut self) {
        println!("[SystemPulse] Started. Print interval: {:?}, Surge threshold: {} node(s) overloaded.",
            self.print_interval, self.surge_overload_threshold_count);

        let system_start_time = Instant::now();

        loop {
            match self.signal_rx.recv_timeout(self.print_interval) {
                Ok(signal) => {
                    let node_id = signal.get_node_id(); // Still useful
                    self.all_nodes_seen.insert(node_id.0 as u64, ());

                    // Process signal types
                    match signal {
                        SystemSignal::NodeOverloaded { .. } => {
                            self.overloaded_nodes.insert(node_id.0 as u64, ());
                        }
                        SystemSignal::NodeIdle { .. } => {
                            // OmegaHashSet doesn't have a remove method, so we'll just ignore this
                            // since we only need to track overloaded nodes
                        }
                        SystemSignal::TaskCompleted { .. } => {
                            self.total_task_completed_signals += 1;
                        }
                        SystemSignal::TaskFailed { .. } => {
                            self.total_task_failed_signals += 1;
                        }
                    }
                    // No OpportunisticInfo to extract or process
                }
                Err(RecvTimeoutError::Timeout) => {
                    // Fall through to periodic print check
                }
                Err(RecvTimeoutError::Disconnected) => {
                    println!("[SystemPulse] Signal channel disconnected. Performing final summary and exiting.");
                    self.print_summary(system_start_time.elapsed().as_millis());
                    if self.surge_mode_active {
                        println!("[SystemPulse] Final State: Surge Mode OFF!");
                    }
                    break; 
                }
            }

            if self.last_print_time.elapsed() >= self.print_interval {
                self.print_summary(system_start_time.elapsed().as_millis());
                self.last_print_time = Instant::now();
            }

            let should_be_in_surge_mode = self.overloaded_nodes.len() >= self.surge_overload_threshold_count;
            if should_be_in_surge_mode != self.surge_mode_active {
                self.surge_mode_active = should_be_in_surge_mode;
                if self.surge_mode_active {
                    println!("[SystemPulse] Surge Mode ON! ({} node(s) overloaded)", self.overloaded_nodes.len());
                } else {
                    println!("[SystemPulse] Surge Mode OFF! ({} node(s) overloaded)", self.overloaded_nodes.len());
                }
            }
        }
        println!("[SystemPulse] Stopped.");
    }

    fn print_summary(&self, elapsed_ms: u128) {
        // Global Avg Task Time is no longer calculated from opportunistic info
        println!(
            "[SystemPulse] {{ {}ms }} Nodes Seen: {} | Overloaded: {} | TaskCompleted Signals: {} | TaskFailed Signals: {}",
            elapsed_ms,
            self.all_nodes_seen.len(),
            self.overloaded_nodes.len(),
            self.total_task_completed_signals,
            self.total_task_failed_signals,
        );
    }
}

// No tests in this file; SystemPulse is observed via main.rs execution.