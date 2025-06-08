// src/node.rs

use crate::queue::{OmegaQueue, PressureLevel, QueueError};
use crate::signals::{NodeId, SystemSignal};
use crate::task::Task;
use crate::types::{LocalStats, NodeError};

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

// --- OmegaNode ---
pub struct OmegaNode<Output: Send + 'static> {
    pub node_id: NodeId,
    pub task_queue: OmegaQueue<Output>,
    worker_threads_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    active_thread_count: Arc<AtomicUsize>,
    pub min_threads: usize,
    pub max_threads: usize,
    signal_tx: mpsc::Sender<SystemSignal>,
    local_stats: Arc<LocalStats>,
    is_shutting_down: Arc<AtomicBool>,

    // --- Fields for Scale-Down Logic ---
    desired_thread_count: Arc<AtomicUsize>,
    last_scaling_time: Arc<Mutex<Instant>>,
    last_self_overload_time: Arc<Mutex<Instant>>,
    pub scale_down_cooldown: Duration,

    // --- NEW Fields for Pressure-Based Routing ---
    pressure: Arc<AtomicUsize>, // New: Current pressure of the node
                                // queue_capacity is available via self.task_queue.capacity()
                                // max_threads is already a field: self.max_threads
}
impl<Output: Send + 'static> OmegaNode<Output> {
    // In OmegaNode struct:
    // pub scale_down_cooldown: Duration, // Keep it pub for direct access if OmegaNode is mut

    pub fn new(
        node_id: NodeId,
        queue_capacity: usize,
        min_threads: usize,
        max_threads: usize,
        signal_tx: mpsc::Sender<SystemSignal>,
        scale_down_cooldown_override: Option<Duration>,
    ) -> Result<Self, String> {
        if min_threads == 0 {
            return Err("min_threads cannot be 0".to_string());
        }
        if max_threads < min_threads {
            return Err("max_threads cannot be less than min_threads".to_string());
        }

        let task_queue = OmegaQueue::new_with_signal(node_id, queue_capacity, signal_tx.clone());
        let local_stats = Arc::new(LocalStats::new());
        let active_thread_count_arc = Arc::new(AtomicUsize::new(0));
        let worker_threads_handles_arc = Arc::new(Mutex::new(Vec::new()));
        let is_shutting_down_arc = Arc::new(AtomicBool::new(false));

        let desired_thread_count_val = min_threads;
        let desired_thread_count_arc = Arc::new(AtomicUsize::new(desired_thread_count_val));
        let now = Instant::now();
        let distant_past = now.checked_sub(Duration::from_secs(3600)).unwrap_or(now);

        let final_scale_down_cooldown =
            scale_down_cooldown_override.unwrap_or(Duration::from_secs(5));

        // Pressure will be properly set after initial workers are spawned.
        // Initialize with a placeholder, then call update_pressure().
        let pressure_arc = Arc::new(AtomicUsize::new(0));

        let node = Self {
            node_id,
            task_queue,
            worker_threads_handles: worker_threads_handles_arc,
            active_thread_count: active_thread_count_arc,
            min_threads,
            max_threads,
            signal_tx,
            local_stats,
            is_shutting_down: is_shutting_down_arc,
            desired_thread_count: desired_thread_count_arc,
            last_scaling_time: Arc::new(Mutex::new(now)), // last_scaling_time is now, because we are "scaling" to min_threads
            last_self_overload_time: Arc::new(Mutex::new(distant_past)),
            scale_down_cooldown: final_scale_down_cooldown,
            pressure: pressure_arc,
        };

        for _ in 0..node.min_threads {
            node.spawn_worker_thread(false); // false as not triggered by overload
        }

        // Ensure desired_thread_count matches active after initial spawn
        node.desired_thread_count.store(
            node.active_thread_count.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );

        // Set initial pressure accurately after workers are spawned
        node.update_pressure();

        Ok(node)
    }

    /// Updates the node's internal pressure value.
    /// Should be called whenever queue length or active thread count changes.
    fn update_pressure(&self) {
        let q_len = self.task_queue.len();
        let active_threads = self.active_thread_count.load(Ordering::Relaxed);
        let current_pressure = q_len + active_threads;
        self.pressure.store(current_pressure, Ordering::Relaxed);
        // Optional: Log pressure changes for debugging
        // println!("[Node {}] Pressure updated: {} (Q: {}, A: {})", self.node_id, current_pressure, q_len, active_threads);
    }

    /// Gets the current pressure of the node.
    /// This is intended for external callers (like a router/sampler).
    pub fn get_pressure(&self) -> usize {
        self.pressure.load(Ordering::Relaxed)
    }

    /// Gets the maximum possible pressure for this node.
    /// A node is considered "full" or "saturated" if its pressure reaches this value.
    pub fn max_pressure(&self) -> usize {
        self.task_queue.capacity() + self.max_threads
    }
    // In OmegaNode impl
    // src/node.rs
    // Inside impl<Output: Send + 'static> OmegaNode<Output>

    // src/node.rs
    // Inside impl<Output: Send + 'static> OmegaNode<Output>

    fn spawn_worker_thread(&self, triggered_by_overload: bool) {
        let current_active_val = self.active_thread_count.load(Ordering::Relaxed);
        if current_active_val >= self.max_threads {
            return;
        }

        let previous_active_count_val = self.active_thread_count.fetch_add(1, Ordering::SeqCst);

        if previous_active_count_val >= self.max_threads {
            self.active_thread_count.fetch_sub(1, Ordering::SeqCst);
            return;
        }

        let current_active_threads_after_increment = previous_active_count_val + 1;
        self.desired_thread_count
            .store(current_active_threads_after_increment, Ordering::SeqCst);

        let now = Instant::now();
        {
            let mut last_scale_guard = self
                .last_scaling_time
                .lock()
                .expect("Mutex poisoned for last_scaling_time");
            *last_scale_guard = now;
        }
        if triggered_by_overload {
            let mut last_overload_guard = self
                .last_self_overload_time
                .lock()
                .expect("Mutex poisoned for last_self_overload_time");
            *last_overload_guard = now;
        }

        self.update_pressure();

        let worker_id_for_name = current_active_threads_after_increment;
        let node_id_clone = self.node_id;
        let task_queue_clone = self.task_queue.clone();
        let local_stats_clone = Arc::clone(&self.local_stats); // Still needed for LocalStats updates
        let signal_tx_clone = self.signal_tx.clone();
        let is_shutting_down_clone = Arc::clone(&self.is_shutting_down);
        // active_thread_count_clone_for_worker is no longer strictly needed here for OpportunisticInfo
        // but the worker loop still uses it for its final decrement.
        let active_thread_count_clone_for_worker = Arc::clone(&self.active_thread_count);
        let worker_check_context = self.clone_for_worker_checks();

        let join_handle = thread::Builder::new()
            .name(format!(
                "omega-node-{}-worker-{}",
                self.node_id.0, worker_id_for_name
            ))
            .spawn(move || {
                let mut retired_by_choice = false;
                loop {
                    if task_queue_clone.is_empty() {
                        if is_shutting_down_clone.load(Ordering::Relaxed) {
                            break;
                        }
                        if !is_shutting_down_clone.load(Ordering::Relaxed) {
                            worker_check_context.consider_reducing_desired_threads();
                            if worker_check_context.check_and_attempt_self_retirement() {
                                retired_by_choice = true;
                                worker_check_context.update_pressure_from_context();
                                break;
                            }
                        }
                        std::thread::yield_now();
                    }

                    let task_option = task_queue_clone.dequeue();
                    if task_option.is_some() {
                        worker_check_context.update_pressure_from_context();
                    }

                    if task_option.is_none() {
                        if is_shutting_down_clone.load(Ordering::Relaxed) {
                            break;
                        }
                        continue;
                    }

                    match task_option {
                        Some(task) => {
                            let task_id = task.id;
                            let start_time = Instant::now();
                            let result = task.execute();
                            let duration = start_time.elapsed();
                            let success = result.is_ok();

                            // Record outcome locally (LocalStats still tracks rolling windows internally)
                            if !is_shutting_down_clone.load(Ordering::Relaxed) {
                                local_stats_clone.record_task_outcome(duration, success);

                                // Construct signals WITHOUT OpportunisticInfo
                                let signal = if success {
                                    SystemSignal::TaskCompleted {
                                        node_id: node_id_clone,
                                        task_id,
                                        duration_micros: duration.as_micros() as u64,
                                        // No opportunistic field
                                    }
                                } else {
                                    SystemSignal::TaskFailed {
                                        node_id: node_id_clone,
                                        task_id,
                                        // No opportunistic field
                                    }
                                };
                                // Send signal (ignore send errors during shutdown, though less likely now)
                                let _ = signal_tx_clone.send(signal);
                            }

                            // Check for scale-down after processing a task
                            if !is_shutting_down_clone.load(Ordering::Relaxed) {
                                worker_check_context.consider_reducing_desired_threads();
                                if worker_check_context.check_and_attempt_self_retirement() {
                                    retired_by_choice = true;
                                    worker_check_context.update_pressure_from_context();
                                    break;
                                }
                            }
                        }
                        None => {} // Already handled
                    }
                } // End of worker loop

                if !retired_by_choice {
                    active_thread_count_clone_for_worker.fetch_sub(1, Ordering::SeqCst);
                    worker_check_context.update_pressure_from_context();
                }
                // Optional: Log worker stopping
                // println!("[Node {}] Worker {} stopping. Retired by choice: {}", node_id_clone, worker_id_for_name, retired_by_choice);
            })
            .expect("Failed to spawn worker thread");

        let mut workers_guard = self
            .worker_threads_handles
            .lock()
            .expect("Worker threads handles mutex poisoned");
        workers_guard.push(join_handle);
    }
    /// Creates a lightweight clone of the necessary Arcs/values for worker checks.
    // In OmegaNode impl
    fn clone_for_worker_checks(&self) -> WorkerCheckContext<Output> {
        WorkerCheckContext {
            node_id: self.node_id,
            active_thread_count: Arc::clone(&self.active_thread_count),
            desired_thread_count: Arc::clone(&self.desired_thread_count),
            min_threads: self.min_threads,
            task_queue: self.task_queue.clone(),
            last_scaling_time: Arc::clone(&self.last_scaling_time),
            last_self_overload_time: Arc::clone(&self.last_self_overload_time),
            scale_down_cooldown: self.scale_down_cooldown,
            node_pressure_atomic: Arc::clone(&self.pressure), // <<< --- PASS THE PRESSURE ARC ---
        }
    }

    
    // In OmegaNode impl
    // src/node.rs
    // Inside impl<Output: Send + 'static> OmegaNode<Output>

    pub fn submit_task(&self, task: Task<Output>) -> Result<(), NodeError> {
        if self.is_shutting_down.load(Ordering::Relaxed) {
            return Err(NodeError::NodeShuttingDown);
        }
        self.local_stats.task_submitted(); // This only increments a counter, doesn't affect pressure directly

        match self.task_queue.enqueue(task) {
            Ok(()) => {
                self.update_pressure(); // <<< --- CALL TO UPDATE PRESSURE ---
                Ok(())
            }
            Err(QueueError::Full) => {
                {
                    let mut last_overload_guard =
                        self.last_self_overload_time.lock().expect("Mutex poisoned");
                    *last_overload_guard = Instant::now();
                }
                // spawn_worker_thread will call update_pressure internally if it scales up
                self.spawn_worker_thread(true);
                Err(NodeError::QueueFull)
            }
            Err(QueueError::Closed) => Err(NodeError::QueueClosed),
            Err(QueueError::SendError) => Err(NodeError::SignalSendError),
            Err(QueueError::Empty) => {
                // This error from enqueue is unexpected and likely indicates a logic error
                // if the queue is not supposed to return Empty on enqueue.
                // Mapping to QueueClosed as a safe fallback, but should be investigated.
                eprintln!(
                    "OmegaNode::submit_task received unexpected QueueError::Empty from enqueue"
                );
                Err(NodeError::QueueClosed)
            }
        }
    }
    pub fn shutdown(&self) {
        if self
            .is_shutting_down
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            self.task_queue.close();
            // Also set desired threads to 0 to encourage quick exit of remaining workers
            self.desired_thread_count.store(0, Ordering::SeqCst);

            let mut workers_guard = self
                .worker_threads_handles
                .lock()
                .expect("Worker threads mutex poisoned");
            for handle in workers_guard.drain(..) {
                if let Err(e) = handle.join() {
                    eprintln!(
                        "Node {} worker thread panicked during shutdown: {:?}",
                        self.node_id, e
                    );
                }
            }
        }
    }

    pub fn id(&self) -> NodeId {
        self.node_id
    }
    pub fn active_threads(&self) -> usize {
        self.active_thread_count.load(Ordering::Relaxed)
    }
    pub fn desired_threads(&self) -> usize {
        self.desired_thread_count.load(Ordering::Relaxed)
    }
}

// Helper struct to pass necessary Arcs and values to worker threads for checks
// This avoids cloning the entire OmegaNode or too many individual Arcs in the spawn closure.
// src/node.rs

// Helper struct to pass necessary Arcs and values to worker threads for checks
struct WorkerCheckContext<Output: Send + 'static> {
    node_id: NodeId,
    active_thread_count: Arc<AtomicUsize>,
    desired_thread_count: Arc<AtomicUsize>,
    min_threads: usize,
    task_queue: OmegaQueue<Output>, // Used for task_queue.len() in pressure update
    last_scaling_time: Arc<Mutex<Instant>>,
    last_self_overload_time: Arc<Mutex<Instant>>,
    scale_down_cooldown: Duration,
    // --- NEW field for WorkerCheckContext ---
    node_pressure_atomic: Arc<AtomicUsize>, // To allow worker to update main node's pressure
}

// Implement methods on WorkerCheckContext that mirror OmegaNode's check methods
impl<OQ: Send + 'static> WorkerCheckContext<OQ> {
    fn consider_reducing_desired_threads(&self) {
        // This logic is identical to OmegaNode::consider_reducing_desired_threads
        // but operates on the fields of WorkerCheckContext.
        let _current_active = self.active_thread_count.load(Ordering::SeqCst); // Not used directly in this decision path
        let current_desired = self.desired_thread_count.load(Ordering::SeqCst);

        if current_desired <= self.min_threads {
            return;
        }

        let now = Instant::now();
        {
            let last_scale_guard = self.last_scaling_time.lock().expect("Mutex poisoned");
            if now.duration_since(*last_scale_guard) < self.scale_down_cooldown {
                return;
            }
        }
        {
            let last_overload_guard = self.last_self_overload_time.lock().expect("Mutex poisoned");
            if now.duration_since(*last_overload_guard) < self.scale_down_cooldown {
                return;
            }
        }

        match self.task_queue.pressure_level() {
            PressureLevel::Empty | PressureLevel::Low => {
                if self
                    .desired_thread_count
                    .compare_exchange(
                        current_desired,
                        current_desired - 1,
                        Ordering::SeqCst,
                        Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    let mut last_scale_guard =
                        self.last_scaling_time.lock().expect("Mutex poisoned");
                    *last_scale_guard = Instant::now();
                    // println!("[Node {}] Worker ctx decided to scale down. Desired threads: {} -> {}", self.node_id, current_desired, current_desired - 1);
                }
            }
            _ => {}
        }
    }

    fn update_pressure_from_context(&self) {
        let q_len = self.task_queue.len();
        let active_threads = self.active_thread_count.load(Ordering::Relaxed);
        let current_pressure = q_len + active_threads;
        self.node_pressure_atomic
            .store(current_pressure, Ordering::Relaxed);
        // Optional: Log from worker context for debugging
        // println!("[Node {} WorkerCtx] Pressure updated: {} (Q: {}, A: {})", self.node_id, current_pressure, q_len, active_threads);
    }
    fn check_and_attempt_self_retirement(&self) -> bool {
        // This logic is identical to OmegaNode::check_and_attempt_self_retirement
        let current_active = self.active_thread_count.load(Ordering::SeqCst);
        let desired_active = self.desired_thread_count.load(Ordering::SeqCst);

        if current_active <= self.min_threads || current_active <= desired_active {
            return false;
        }

        match self.active_thread_count.compare_exchange(
            current_active,
            current_active - 1,
            Ordering::SeqCst,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                let mut last_scale_guard = self.last_scaling_time.lock().expect("Mutex poisoned");
                *last_scale_guard = Instant::now();
                // println!("[Node {}] Worker ctx retiring. Active threads: {} -> {}", self.node_id, current_active, current_active - 1);
                true
            }
            Err(_) => false,
        }
    }
}

impl<Output: Send + 'static> Drop for OmegaNode<Output> {
    fn drop(&mut self) {
        if !self.is_shutting_down.load(Ordering::Relaxed) {
            // println!("[Node {}] Dropping, initiating shutdown.", self.node_id);
            self.shutdown();
        }
    }
}
