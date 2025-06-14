// src/system_pulse.rs

use crate::signals::SystemSignal;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

pub struct OmegaSystemPulse {
    signal_rx: Receiver<SystemSignal>,
    print_interval: Duration,
    surge_overload_threshold_count: usize,
    // Internal state
    overloaded_nodes: HashSet<usize>,
    all_nodes_seen: HashSet<usize>,
    total_tasks_dequeued: u64,
    total_tasks_processed: u64,
    surge_mode_active: bool,
    last_print_time: Instant,
}

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
            overloaded_nodes: HashSet::new(),
            all_nodes_seen: HashSet::new(),
            total_tasks_dequeued: 0,
            total_tasks_processed: 0,
            surge_mode_active: false,
            last_print_time: Instant::now(),
        }
    }

    pub fn run(mut self) {
        let system_start_time = Instant::now();

        loop {
            // Wait for a signal or timeout for printing
            match self.signal_rx.recv_timeout(self.print_interval) {
                Ok(signal) => {
                    let node_id_val = signal.get_node_id().0;
                    self.all_nodes_seen.insert(node_id_val);

                    match signal {
                        SystemSignal::NodeOverloaded { .. } => {
                            self.overloaded_nodes.insert(node_id_val);
                        }
                        SystemSignal::NodeIdle { .. } => {
                            self.overloaded_nodes.remove(&node_id_val);
                        }
                        SystemSignal::TaskDequeuedByWorker { .. } => {
                            self.total_tasks_dequeued += 1;
                        }
                        SystemSignal::TaskProcessed { .. } => {
                            self.total_tasks_processed += 1;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    // This is expected, just proceed to the periodic checks
                }
                Err(RecvTimeoutError::Disconnected) => {
                    // Channel is disconnected, main thread has likely shut down. Exit gracefully.
                    break;
                }
            }

            // Check if it's time to print the summary
            if self.last_print_time.elapsed() >= self.print_interval {
                self.print_summary(system_start_time.elapsed().as_millis());
                self.last_print_time = Instant::now();
            }

            // Check for surge mode toggle
            let should_be_in_surge_mode = self.overloaded_nodes.len() >= self.surge_overload_threshold_count;
            if should_be_in_surge_mode != self.surge_mode_active {
                self.surge_mode_active = should_be_in_surge_mode;
                // Optional: log surge mode changes
            }
        }
    }

    fn print_summary(&self, elapsed_ms: u128) {
        // This is commented out to keep the main test output clean, but is useful for debugging.
        
        // println!(
        //     "[SystemPulse] {{ {}ms }} Nodes Seen: {} | Overloaded: {} | Dequeued: {} | Processed: {}",
        //     elapsed_ms,
        //     self.all_nodes_seen.len(),
        //     self.overloaded_nodes.len(),
        //     self.total_tasks_dequeued,
        //     self.total_tasks_processed,
        // );
        
    }
}