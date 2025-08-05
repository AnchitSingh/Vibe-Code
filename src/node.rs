//! Defines `VibeNode`, a self-contained processing unit.
//!
//! A `VibeNode` is like a single worker in the system's factory. It has its own
//! task queue and a pool of threads to execute those tasks. It's designed to be
//! self-managing, automatically adjusting the number of active threads based on
//! its current workload (pressure).

use crate::queue::{QueueError, VibeQueue};
use crate::signals::{NodeId, SystemSignal};
use crate::task::{Task, TaskExecutionOutcome};
use crate::types::{LocalStats, NodeError};
use crate::utils::elapsed_ns;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// A qualitative measure of a node's current workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureLevel {
    /// The task queue is empty.
    Empty,
    /// The task queue has a few tasks.
    Low,
    /// The task queue is at a healthy, normal level.
    Normal,
    /// The task queue is nearly full, indicating high load.
    High,
    /// The task queue is at maximum capacity.
    Full,
}

/// A processing unit that executes tasks.
///
/// Each `VibeNode` contains a task queue and a dynamic pool of worker threads.
/// It reports its status (e.g., overloaded, idle) to the central system, which
/// helps with load balancing.
pub struct VibeNode {
    /// A unique identifier for this node.
    pub node_id: NodeId,
    /// The queue that holds tasks waiting to be executed by this node.
    pub task_queue: VibeQueue<Task>,
    /// Handles to the worker threads managed by this node.
    worker_threads_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    /// The number of currently active worker threads.
    active_thread_count: Arc<AtomicUsize>,
    /// The minimum number of worker threads to keep alive.
    pub min_threads: usize,
    /// The maximum number of worker threads this node can spawn.
    pub max_threads: usize,
    /// A channel to send signals (like "I'm overloaded!") to the central system.
    signal_tx: mpsc::Sender<SystemSignal>,
    /// Performance statistics for this specific node.
    local_stats: Arc<LocalStats>,
    /// A flag to indicate that the node is in the process of shutting down.
    is_shutting_down: Arc<AtomicBool>,
    /// The target number of worker threads, adjusted dynamically based on load.
    desired_thread_count: Arc<AtomicUsize>,
    /// The timestamp of the last scaling event (adding or removing a thread).
    last_scaling_time: Arc<Mutex<u64>>,
    /// The timestamp of the last time this node's queue was full.
    last_self_overload_time: Arc<Mutex<u64>>,
    /// A cooldown period to prevent scaling down threads too aggressively.
    pub scale_down_cooldown: u64,
    /// A percentage (0-100) representing the current queue load.
    pressure: Arc<AtomicUsize>,
}

impl VibeNode {
    /// Creates and initializes a new `VibeNode`.
    ///
    /// This sets up the task queue, thread limits, and spawns the minimum
    /// number of worker threads to start processing tasks.
    pub fn new(
        node_id: NodeId,
        queue_capacity: usize,
        min_threads: usize,
        max_threads: usize,
        signal_tx: mpsc::Sender<SystemSignal>,
        scale_down_cooldown_override: Option<u64>,
    ) -> Result<Self, String> {
        if min_threads == 0 {
            return Err("min_threads cannot be 0".to_string());
        }
        if max_threads < min_threads {
            return Err("max_threads cannot be less than min_threads".to_string());
        }

        let task_queue = VibeQueue::new_with_signal(node_id, queue_capacity, signal_tx.clone());
        let pressure_arc = Arc::new(AtomicUsize::new(0));
        const NANOS_PER_SEC: u64 = 1_000_000_000;

        let node = Self {
            node_id,
            task_queue,
            worker_threads_handles: Arc::new(Mutex::new(Vec::new())),
            active_thread_count: Arc::new(AtomicUsize::new(0)),
            min_threads,
            max_threads,
            signal_tx,
            local_stats: Arc::new(LocalStats::new()),
            is_shutting_down: Arc::new(AtomicBool::new(false)),
            desired_thread_count: Arc::new(AtomicUsize::new(min_threads)),
            last_scaling_time: Arc::new(Mutex::new(elapsed_ns())),
            last_self_overload_time: Arc::new(Mutex::new(
                elapsed_ns().saturating_sub(3600 * NANOS_PER_SEC),
            )),
            scale_down_cooldown: scale_down_cooldown_override.unwrap_or(5 * NANOS_PER_SEC),
            pressure: pressure_arc,
        };

        // Spawn the initial set of worker threads.
        for _ in 0..node.min_threads {
            node.spawn_worker_thread(false);
        }
        node.update_pressure();

        Ok(node)
    }

    /// Calculates and updates the node's pressure based on queue fullness.
    fn update_pressure(&self) {
        let q = self.task_queue.len() as f64;
        let c = self.active_thread_count.load(Ordering::Relaxed) as f64;
        let k = self.task_queue.capacity() as f64;

        let pressure_float = if c > 0.0 && k > 0.0 {
            (q / k) * 100.0
        } else if q > 0.0 {
            100.0
        } else {
            0.0
        };
        self.pressure
            .store(pressure_float.clamp(0.0, 100.0) as usize, Ordering::Relaxed);
    }

    /// Returns the current pressure of the node (a percentage from 0 to 100).
    pub fn get_pressure(&self) -> usize {
        self.pressure.load(Ordering::Relaxed)
    }

    /// Returns the maximum possible pressure value (always 100).
    pub fn max_pressure(&self) -> usize {
        100
    }

    /// Returns a qualitative `PressureLevel` based on the current numeric pressure.
    pub fn get_pressure_level(&self) -> PressureLevel {
        match self.get_pressure() {
            0 => PressureLevel::Empty,
            1..=25 => PressureLevel::Low,
            26..=75 => PressureLevel::Normal,
            76..=99 => PressureLevel::High,
            _ => PressureLevel::Full,
        }
    }

    /// Spawns a new worker thread if the node is under load and below its max thread count.
    fn spawn_worker_thread(&self, triggered_by_overload: bool) {
        if self.active_threads() >= self.max_threads {
            return;
        }
        if self.is_shutting_down.load(Ordering::Relaxed) {
            return;
        }

        self.active_thread_count.fetch_add(1, Ordering::SeqCst);
        self.desired_thread_count
            .store(self.active_threads(), Ordering::SeqCst);

        let now = elapsed_ns();
        *self
            .last_scaling_time
            .lock()
            .expect("Mutex should not be poisoned") = now;
        if triggered_by_overload {
            *self
                .last_self_overload_time
                .lock()
                .expect("Mutex should not be poisoned") = now;
        }

        self.update_pressure();

        let worker_context = self.clone_for_worker();
        let handle = thread::Builder::new()
            .name(format!(
                "vibe-node-{}-worker-{}",
                self.node_id.0,
                self.active_threads()
            ))
            .spawn(move || worker_context.run_loop())
            .expect("Failed to spawn worker thread");

        self.worker_threads_handles
            .lock()
            .expect("Mutex should not be poisoned")
            .push(handle);
    }

    /// Submits a task to this node's queue.
    ///
    /// This is an internal method called by the system's task router. It may
    /// trigger spawning a new worker thread if the queue pressure becomes high.
    pub(crate) fn submit_task(&self, task: Task) -> Result<(), NodeError> {
        if self.is_shutting_down.load(Ordering::Relaxed) {
            return Err(NodeError::NodeShuttingDown);
        }
        self.local_stats.task_submitted();
        match self.task_queue.enqueue(task) {
            Ok(()) => {
                self.update_pressure();
                if self.get_pressure_level() == PressureLevel::High
                    || self.get_pressure_level() == PressureLevel::Full
                {
                    self.spawn_worker_thread(true);
                }
                Ok(())
            }
            Err(QueueError::Full) => {
                *self
                    .last_self_overload_time
                    .lock()
                    .expect("Mutex should not be poisoned") = elapsed_ns();
                self.spawn_worker_thread(true);
                Err(NodeError::QueueFull)
            }
            Err(e) => Err(NodeError::from(e)),
        }
    }

    /// Begins the shutdown process for the node.
    ///
    /// This closes the task queue to new submissions and waits for all existing
    /// worker threads to finish their current tasks and exit gracefully.
    pub fn shutdown(&self) {
        if self
            .is_shutting_down
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            self.task_queue.close();
            self.desired_thread_count.store(0, Ordering::SeqCst);
            let mut workers = self
                .worker_threads_handles
                .lock()
                .expect("Mutex should not be poisoned");
            for handle in workers.drain(..) {
                let _ = handle.join();
            }
        }
    }

    /// Clones the necessary context for a new worker thread.
    ///
    /// This bundles all the shared data (`Arc`s) that a worker needs to operate.
    fn clone_for_worker(&self) -> WorkerContext {
        WorkerContext {
            node_id: self.node_id,
            active_thread_count: Arc::clone(&self.active_thread_count),
            desired_thread_count: Arc::clone(&self.desired_thread_count),
            min_threads: self.min_threads,
            max_threads: self.max_threads,
            task_queue: self.task_queue.clone(),
            last_scaling_time: Arc::clone(&self.last_scaling_time),
            last_self_overload_time: Arc::clone(&self.last_self_overload_time),
            scale_down_cooldown: self.scale_down_cooldown,
            node_pressure_atomic: Arc::clone(&self.pressure),
            is_shutting_down: Arc::clone(&self.is_shutting_down),
            signal_tx: self.signal_tx.clone(),
            local_stats: Arc::clone(&self.local_stats),
        }
    }

    /// Returns the node's unique ID.
    pub fn id(&self) -> NodeId {
        self.node_id
    }

    /// Returns the current number of active worker threads.
    pub fn active_threads(&self) -> usize {
        self.active_thread_count.load(Ordering::Relaxed)
    }

    /// Returns the desired number of worker threads.
    pub fn desired_threads(&self) -> usize {
        self.desired_thread_count.load(Ordering::Relaxed)
    }
}

/// Contains the shared state and logic for a single worker thread.
struct WorkerContext {
    node_id: NodeId,
    active_thread_count: Arc<AtomicUsize>,
    desired_thread_count: Arc<AtomicUsize>,
    min_threads: usize,
    max_threads: usize,
    task_queue: VibeQueue<Task>,
    last_scaling_time: Arc<Mutex<u64>>,
    last_self_overload_time: Arc<Mutex<u64>>,
    scale_down_cooldown: u64,
    node_pressure_atomic: Arc<AtomicUsize>,
    is_shutting_down: Arc<AtomicBool>,
    signal_tx: mpsc::Sender<SystemSignal>,
    local_stats: Arc<LocalStats>,
}

impl WorkerContext {
    /// The main loop for a worker thread.
    ///
    /// The worker continuously pulls tasks from the queue, processes them, and
    /// checks if it should scale down or retire.
    fn run_loop(self) {
        let mut retired_by_choice = false;
        loop {
            if self.is_shutting_down.load(Ordering::Relaxed) {
                break;
            }

            if let Some(task) = self.task_queue.dequeue() {
                self.update_pressure_from_context();
                let _ = self.signal_tx.send(SystemSignal::TaskDequeuedByWorker {
                    node_id: self.node_id,
                    task_id: task.id,
                });
                self.process_task(task);
            } else {
                if self.is_shutting_down.load(Ordering::Relaxed) {
                    break;
                }

                self.consider_scaling_down();
                if self.check_and_attempt_self_retirement() {
                    retired_by_choice = true;
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
        }

        if !retired_by_choice {
            self.active_thread_count.fetch_sub(1, Ordering::SeqCst);
            self.update_pressure_from_context();
        }
    }

    /// Executes a single task and records the outcome.
    fn process_task(&self, task: Task) {
        let task_id = task.id;
        let start_time_ns = elapsed_ns();

        let outcome = task.run();

        let duration_ns = elapsed_ns().saturating_sub(start_time_ns);

        let was_logically_successful = outcome == TaskExecutionOutcome::Success;
        self.local_stats
            .record_task_outcome(duration_ns, was_logically_successful);

        if !self.is_shutting_down.load(Ordering::Relaxed) {
            let signal = SystemSignal::TaskProcessed {
                node_id: self.node_id,
                task_id,
                duration_micros: duration_ns / 1000,
            };
            let _ = self.signal_tx.send(signal);
        }
    }

    /// Checks if the node is idle and if a thread can be scaled down.
    fn consider_scaling_down(&self) {
        let current_desired = self.desired_thread_count.load(Ordering::SeqCst);
        if current_desired <= self.min_threads {
            return;
        }

        let now = elapsed_ns();
        let last_scale_time = *self
            .last_scaling_time
            .lock()
            .expect("Mutex should not be poisoned");
        let last_overload_time = *self
            .last_self_overload_time
            .lock()
            .expect("Mutex should not be poisoned");

        if now.saturating_sub(last_scale_time) < self.scale_down_cooldown {
            return;
        }
        if now.saturating_sub(last_overload_time) < self.scale_down_cooldown {
            return;
        }

        let pressure = self.get_pressure_from_context();
        let pressure_level = self.get_pressure_level_from_pressure(pressure);

        if (pressure_level == PressureLevel::Empty || pressure_level == PressureLevel::Low)
            && self
                .desired_thread_count
                .compare_exchange(
                    current_desired,
                    current_desired - 1,
                    Ordering::SeqCst,
                    Ordering::Relaxed,
                )
                .is_ok()
        {
            *self
                .last_scaling_time
                .lock()
                .expect("Mutex should not be poisoned") = now;
        }
    }

    /// Checks if this worker thread is now superfluous and can shut down.
    fn check_and_attempt_self_retirement(&self) -> bool {
        let current_active = self.active_thread_count.load(Ordering::SeqCst);
        if current_active <= self.min_threads
            || current_active <= self.desired_thread_count.load(Ordering::SeqCst)
        {
            return false;
        }

        if self
            .active_thread_count
            .compare_exchange(
                current_active,
                current_active - 1,
                Ordering::SeqCst,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            *self
                .last_scaling_time
                .lock()
                .expect("Mutex should not be poisoned") = elapsed_ns();
            self.update_pressure_from_context();
            true
        } else {
            false
        }
    }

    /// Helper for a worker to calculate the node's current pressure.
    fn get_pressure_from_context(&self) -> usize {
        let q = self.task_queue.len() as f64;
        let c = self.active_thread_count.load(Ordering::Relaxed) as f64;
        let k = self.task_queue.capacity() as f64;
        let pressure_float = if c > 0.0 && k > 0.0 {
            (q / k) * 100.0
        } else if q > 0.0 {
            100.0
        } else {
            0.0
        };
        pressure_float.clamp(0.0, 100.0) as usize
    }

    /// Helper to get a `PressureLevel` enum from a numeric pressure value.
    fn get_pressure_level_from_pressure(&self, pressure: usize) -> PressureLevel {
        match pressure {
            0 => PressureLevel::Empty,
            1..=25 => PressureLevel::Low,
            26..=75 => PressureLevel::Normal,
            76..=99 => PressureLevel::High,
            _ => PressureLevel::Full,
        }
    }

    /// Updates the node's shared pressure atomic from the worker's context.
    fn update_pressure_from_context(&self) {
        let pressure = self.get_pressure_from_context();
        self.node_pressure_atomic.store(pressure, Ordering::Relaxed);
    }
}

impl Drop for VibeNode {
    /// Ensures the node is properly shut down when it goes out of scope.
    fn drop(&mut self) {
        if !self.is_shutting_down.load(Ordering::Relaxed) {
            self.shutdown();
        }
    }
}

impl From<QueueError> for NodeError {
    /// Converts a `QueueError` into a `NodeError`.
    fn from(qe: QueueError) -> Self {
        match qe {
            QueueError::Full => NodeError::QueueFull,
            QueueError::Closed => NodeError::QueueClosed,
            QueueError::SendError => NodeError::SignalSendError,
            QueueError::Empty => NodeError::QueueClosed,
        }
    }
}
