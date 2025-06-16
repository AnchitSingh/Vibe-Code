//! Defines the `OmegaNode`, a core processing unit within the CPU Circulatory System.
//!
//! An `OmegaNode` manages a task queue and a pool of worker threads to execute
//! CPU-bound tasks. It implements dynamic thread scaling based on queue pressure
//! and communicates its state (e.g., overload, idle) via `SystemSignal`s.

use crate::queue::{OmegaQueue, QueueError};
use crate::signals::{NodeId, SystemSignal};
use crate::task::{Task, TaskExecutionOutcome};
use crate::types::{LocalStats, NodeError};
use omega::omega_timer::elapsed_ns;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Represents the current pressure level of an `OmegaNode`'s task queue.
///
/// This enum provides a qualitative measure of how busy a node is,
/// based on its queue length relative to its capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureLevel {
    /// The node's task queue is empty.
    Empty,
    /// The node's task queue has a low number of tasks.
    Low,
    /// The node's task queue is at a normal, healthy level.
    Normal,
    /// The node's task queue is approaching full capacity, indicating high load.
    High,
    /// The node's task queue is at or beyond its maximum capacity.
    Full,
}

/// An `OmegaNode` is a processing unit responsible for executing tasks.
///
/// Each node maintains its own task queue and a pool of worker threads.
/// It dynamically scales its worker threads based on the current queue pressure
/// and reports its state to the `UltraOmegaSystem` via `SystemSignal`s.
pub struct OmegaNode {
    /// The unique identifier for this node.
    pub node_id: NodeId,
    /// The queue where tasks are submitted to this node.
    pub task_queue: OmegaQueue<Task>,
    /// A collection of `JoinHandle`s for the worker threads managed by this node.
    worker_threads_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    /// Atomic counter for the number of currently active worker threads.
    active_thread_count: Arc<AtomicUsize>,
    /// The minimum number of worker threads that should always be running.
    pub min_threads: usize,
    /// The maximum number of worker threads this node can spawn.
    pub max_threads: usize,
    /// Sender channel for sending `SystemSignal`s to the central system.
    signal_tx: mpsc::Sender<SystemSignal>,
    /// Local statistics tracking for this node's performance.
    local_stats: Arc<LocalStats>,
    /// Atomic flag indicating if the node is in the process of shutting down.
    is_shutting_down: Arc<AtomicBool>,
    /// The desired number of worker threads, used for scaling decisions.
    desired_thread_count: Arc<AtomicUsize>,
    /// The last time a scaling operation (up or down) occurred, in nanoseconds.
    last_scaling_time: Arc<Mutex<u64>>,
    /// The last time this node reported itself as overloaded, in nanoseconds.
    last_self_overload_time: Arc<Mutex<u64>>,
    /// Cooldown period (in nanoseconds) before the node can scale down threads.
    pub scale_down_cooldown: u64,
    /// Atomic representation of the node's current pressure (0-100%).
    pressure: Arc<AtomicUsize>,
}

impl OmegaNode {
    /// Creates a new `OmegaNode` instance.
    ///
    /// Initializes the node with a task queue, sets up thread limits,
    /// and spawns the minimum number of worker threads.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The unique identifier for this node.
    /// * `queue_capacity` - The maximum number of tasks the node's queue can hold.
    /// * `min_threads` - The minimum number of worker threads to maintain.
    /// * `max_threads` - The maximum number of worker threads to allow.
    /// * `signal_tx` - A sender for `SystemSignal`s to communicate with the central system.
    /// * `scale_down_cooldown_override` - An optional override for the default scale-down cooldown.
    ///
    /// # Returns
    ///
    /// A `Result` containing the initialized `OmegaNode` on success, or a `String` error
    /// if `min_threads` is zero or `max_threads` is less than `min_threads`.
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

        let task_queue = OmegaQueue::new_with_signal(node_id, queue_capacity, signal_tx.clone());
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
            // Initialize last_self_overload_time far in the past to allow immediate scaling up if needed.
            last_self_overload_time: Arc::new(Mutex::new(
                elapsed_ns().saturating_sub(3600 * NANOS_PER_SEC),
            )),
            scale_down_cooldown: scale_down_cooldown_override.unwrap_or(5 * NANOS_PER_SEC),
            pressure: pressure_arc,
        };

        // Spawn initial worker threads up to `min_threads`.
        for _ in 0..node.min_threads {
            node.spawn_worker_thread(false);
        }
        node.update_pressure(); // Initial pressure calculation

        Ok(node)
    }

    /// Calculates and updates the node's current pressure based on queue length and active threads.
    ///
    /// Pressure is a value from 0 to 100, representing the percentage of queue capacity utilized,
    /// potentially influenced by active thread count.
    fn update_pressure(&self) {
        let q = self.task_queue.len() as f64; // Current queue length
        let c = self.active_thread_count.load(Ordering::Relaxed) as f64; // Active thread count
        let k = self.task_queue.capacity() as f64; // Queue capacity

        let pressure_float = if c > 0.0 && k > 0.0 {
            (q / k) * 100.0 // Pressure based on queue fill ratio
        } else if q > 0.0 {
            100.0 // If queue has tasks but no capacity/threads, consider it full pressure
        } else {
            0.0 // No tasks, no pressure
        };
        self.pressure
            .store(pressure_float.clamp(0.0, 100.0) as usize, Ordering::Relaxed);
    }

    /// Returns the current pressure of the node (0-100%).
    pub fn get_pressure(&self) -> usize {
        self.pressure.load(Ordering::Relaxed)
    }

    /// Returns the maximum possible pressure value (100%).
    pub fn max_pressure(&self) -> usize {
        100
    }

    /// Returns the qualitative `PressureLevel` based on the current pressure.
    pub fn get_pressure_level(&self) -> PressureLevel {
        match self.get_pressure() {
            0 => PressureLevel::Empty,
            1..=25 => PressureLevel::Low,
            26..=75 => PressureLevel::Normal,
            76..=99 => PressureLevel::High,
            _ => PressureLevel::Full, // 100%
        }
    }

    /// Spawns a new worker thread for this node.
    ///
    /// A new thread is only spawned if the current active thread count is below
    /// `max_threads` and the node is not shutting down. It updates scaling times
    /// and pressure.
    ///
    /// # Arguments
    ///
    /// * `triggered_by_overload` - `true` if this spawn was a direct response to queue overload.
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
        *self.last_scaling_time.lock().unwrap() = now;
        if triggered_by_overload {
            *self.last_self_overload_time.lock().unwrap() = now;
        }

        self.update_pressure(); // Update pressure after spawning a new thread

        let worker_context = self.clone_for_worker();
        let handle = thread::Builder::new()
            .name(format!(
                "omega-node-{}-worker-{}",
                self.node_id.0,
                self.active_threads()
            ))
            .spawn(move || worker_context.run_loop())
            .expect("Failed to spawn worker thread");

        self.worker_threads_handles.lock().unwrap().push(handle);
    }

    /// Submits a `Task` to this `OmegaNode`'s task queue.
    ///
    /// This is the internal method called by `UltraOmegaSystem` to route tasks.
    /// It updates local statistics and may trigger worker thread spawning if the
    /// queue pressure is high.
    ///
    /// # Arguments
    ///
    /// * `task` - The `Task` to submit.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success (`Ok(())`) or a `NodeError` if the node
    /// is shutting down, the queue is full, or a signal send fails.
    pub(crate) fn submit_task(&self, task: Task) -> Result<(), NodeError> {
        if self.is_shutting_down.load(Ordering::Relaxed) {
            return Err(NodeError::NodeShuttingDown);
        }
        self.local_stats.task_submitted(); // Record task submission
        match self.task_queue.enqueue(task) {
            Ok(()) => {
                self.update_pressure();
                // If pressure is high or full, consider spawning another worker.
                if self.get_pressure_level() == PressureLevel::High
                    || self.get_pressure_level() == PressureLevel::Full
                {
                    self.spawn_worker_thread(true);
                }
                Ok(())
            }
            Err(QueueError::Full) => {
                // If queue is full, record overload time and try to spawn a worker.
                *self.last_self_overload_time.lock().unwrap() = elapsed_ns();
                self.spawn_worker_thread(true);
                Err(NodeError::QueueFull)
            }
            Err(e) => Err(NodeError::from(e)), // Convert other QueueErrors to NodeError
        }
    }

    /// Initiates the shutdown process for the `OmegaNode`.
    ///
    /// This closes the task queue, sets the desired thread count to zero,
    /// and waits for all worker threads to complete their current tasks and exit.
    pub fn shutdown(&self) {
        // Use compare_exchange to ensure shutdown is only initiated once.
        if self
            .is_shutting_down
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            self.task_queue.close(); // Close the queue to prevent new tasks
            self.desired_thread_count.store(0, Ordering::SeqCst); // Signal workers to exit
            let mut workers = self.worker_threads_handles.lock().unwrap();
            for handle in workers.drain(..) {
                let _ = handle.join(); // Wait for each worker thread to finish
            }
        }
    }

    /// Creates a `WorkerContext` clone for a new worker thread.
    ///
    /// This method bundles all necessary `Arc`s and immutable data for a worker
    /// thread to operate independently.
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

    /// Returns the unique `NodeId` of this node.
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

// --- WorkerContext ---
/// Contextual data and methods for an individual worker thread within an `OmegaNode`.
///
/// Each worker thread receives a clone of this context, allowing it to interact
/// with the node's shared resources (queue, atomics, signals) and execute tasks.
struct WorkerContext {
    node_id: NodeId,
    active_thread_count: Arc<AtomicUsize>,
    desired_thread_count: Arc<AtomicUsize>,
    min_threads: usize,
    max_threads: usize,
    task_queue: OmegaQueue<Task>,
    last_scaling_time: Arc<Mutex<u64>>,
    last_self_overload_time: Arc<Mutex<u64>>,
    scale_down_cooldown: u64,
    node_pressure_atomic: Arc<AtomicUsize>,
    is_shutting_down: Arc<AtomicBool>,
    signal_tx: mpsc::Sender<SystemSignal>,
    local_stats: Arc<LocalStats>,
}

impl WorkerContext {
    /// The main execution loop for a worker thread.
    ///
    /// This loop continuously attempts to dequeue and process tasks.
    /// It also handles dynamic scaling down of threads and self-retirement
    /// when the node is idle or over-provisioned.
    fn run_loop(self) {
        let mut retired_by_choice = false;
        loop {
            // Break loop if node is shutting down.
            if self.is_shutting_down.load(Ordering::Relaxed) {
                break;
            }

            // Attempt to dequeue a task.
            if let Some(task) = self.task_queue.dequeue() {
                self.update_pressure_from_context(); // Update pressure after dequeue
                let _ = self.signal_tx.send(SystemSignal::TaskDequeuedByWorker {
                    node_id: self.node_id,
                    task_id: task.id,
                });
                self.process_task(task); // Process the dequeued task
            } else {
                // If no task, check for shutdown again.
                if self.is_shutting_down.load(Ordering::Relaxed) {
                    break;
                }

                // Consider scaling down if conditions are met.
                self.consider_scaling_down();
                // Attempt to retire this worker thread if it's no longer needed.
                if self.check_and_attempt_self_retirement() {
                    retired_by_choice = true;
                    break; // Worker successfully retired
                }
                // If no task and not retiring, sleep briefly to avoid busy-waiting.
                thread::sleep(Duration::from_millis(5));
            }
        }

        // If the worker did not retire by choice (i.e., it was forced to shut down),
        // decrement the active thread count.
        if !retired_by_choice {
            self.active_thread_count.fetch_sub(1, Ordering::SeqCst);
            self.update_pressure_from_context();
        }
    }

    /// Processes a single `Task`.
    ///
    /// Executes the task's work function, records its outcome and duration
    /// in local statistics, and sends a `TaskProcessed` signal.
    ///
    /// # Arguments
    ///
    /// * `task` - The `Task` to process.
    fn process_task(&self, task: Task) {
        let task_id = task.id;
        let start_time_ns = elapsed_ns();

        // Execute the task's work function.
        let outcome = task.run();

        let duration_ns = elapsed_ns().saturating_sub(start_time_ns);

        // Determine if the task was logically successful for local statistics.
        let was_logically_successful = outcome == TaskExecutionOutcome::Success;
        self.local_stats
            .record_task_outcome(duration_ns, was_logically_successful);

        // Always send `TaskProcessed` signal, regardless of the task's logical outcome.
        // This signal indicates the worker has completed its processing cycle for this task.
        if !self.is_shutting_down.load(Ordering::Relaxed) {
            let signal = SystemSignal::TaskProcessed {
                node_id: self.node_id,
                task_id,
                duration_micros: duration_ns / 1000, // Convert nanoseconds to microseconds
            };
            let _ = self.signal_tx.send(signal);
        }
    }

    /// Considers whether to scale down the number of desired worker threads.
    ///
    /// This method checks if the current desired thread count is above the minimum,
    /// if cooldown periods have passed, and if the node's pressure is low.
    fn consider_scaling_down(&self) {
        let current_desired = self.desired_thread_count.load(Ordering::SeqCst);
        if current_desired <= self.min_threads {
            return; // Cannot scale down below minimum threads
        }

        let now = elapsed_ns();
        let last_scale_time = *self.last_scaling_time.lock().unwrap();
        let last_overload_time = *self.last_self_overload_time.lock().unwrap();

        // Enforce cooldown periods for scaling down.
        if now.saturating_sub(last_scale_time) < self.scale_down_cooldown {
            return;
        }
        if now.saturating_sub(last_overload_time) < self.scale_down_cooldown {
            return;
        }

        let pressure = self.get_pressure_from_context();
        let pressure_level = self.get_pressure_level_from_pressure(pressure);

        // If pressure is low, attempt to decrement desired thread count.
        if (pressure_level == PressureLevel::Empty || pressure_level == PressureLevel::Low)
            && self
                .desired_thread_count
                .compare_exchange(current_desired, current_desired - 1, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
        {
            *self.last_scaling_time.lock().unwrap() = now; // Update last scaling time
        }
    }

    /// Checks if this worker thread should retire and attempts to decrement the active count.
    ///
    /// A worker retires if the active thread count is above the desired count and
    /// above the minimum, and it successfully decrements the active count.
    ///
    /// # Returns
    ///
    /// `true` if the worker successfully retired, `false` otherwise.
    fn check_and_attempt_self_retirement(&self) -> bool {
        let current_active = self.active_thread_count.load(Ordering::SeqCst);
        // Retire if active threads are more than minimum AND more than desired.
        if current_active <= self.min_threads
            || current_active <= self.desired_thread_count.load(Ordering::SeqCst)
        {
            return false;
        }

        // Atomically try to decrement active thread count.
        if self
            .active_thread_count
            .compare_exchange(current_active, current_active - 1, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            *self.last_scaling_time.lock().unwrap() = elapsed_ns(); // Update last scaling time
            self.update_pressure_from_context(); // Update pressure after retirement
            true
        } else {
            false // Failed to retire (e.g., another thread retired first)
        }
    }

    /// Calculates the current pressure of the node from the worker's context.
    ///
    /// This is a helper for workers to get an up-to-date pressure value.
    ///
    /// # Returns
    ///
    /// The calculated pressure (0-100%).
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

    /// Returns the qualitative `PressureLevel` based on a given pressure value.
    ///
    /// This is a helper for workers to interpret pressure.
    ///
    /// # Arguments
    ///
    /// * `pressure` - The pressure value (0-100%).
    ///
    /// # Returns
    ///
    /// The corresponding `PressureLevel`.
    fn get_pressure_level_from_pressure(&self, pressure: usize) -> PressureLevel {
        match pressure {
            0 => PressureLevel::Empty,
            1..=25 => PressureLevel::Low,
            26..=75 => PressureLevel::Normal,
            76..=99 => PressureLevel::High,
            _ => PressureLevel::Full,
        }
    }

    /// Updates the shared atomic pressure value of the node from the worker's context.
    fn update_pressure_from_context(&self) {
        let pressure = self.get_pressure_from_context();
        self.node_pressure_atomic.store(pressure, Ordering::Relaxed);
    }
}

impl Drop for OmegaNode {
    /// Ensures that the `OmegaNode` is properly shut down when it goes out of scope.
    ///
    /// This prevents resource leaks by calling the `shutdown` method if it hasn't
    /// been called already.
    fn drop(&mut self) {
        if !self.is_shutting_down.load(Ordering::Relaxed) {
            self.shutdown();
        }
    }
}

impl From<QueueError> for NodeError {
    /// Converts a `QueueError` into a `NodeError`.
    ///
    /// This allows `QueueError`s originating from the `OmegaQueue` to be
    /// propagated as `NodeError`s.
    fn from(qe: QueueError) -> Self {
        match qe {
            QueueError::Full => NodeError::QueueFull,
            QueueError::Closed => NodeError::QueueClosed,
            QueueError::SendError => NodeError::SignalSendError,
            QueueError::Empty => NodeError::QueueClosed, // An empty queue when trying to dequeue from a closed queue
        }
    }
}