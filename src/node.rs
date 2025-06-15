// src/node.rs

use crate::queue::{OmegaQueue, QueueError};
use crate::signals::{NodeId, SystemSignal};
// TaskExecutionOutcome is now used by the worker
use crate::task::{Task, TaskExecutionOutcome};
use crate::types::{LocalStats, NodeError};
use omega::omega_timer::elapsed_ns;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureLevel {
    Empty,
    Low,
    Normal,
    High,
    Full,
}

// --- OmegaNode (No longer generic) ---
pub struct OmegaNode {
    pub node_id: NodeId,
    pub task_queue: OmegaQueue<Task>, // Now holds the concrete Task struct
    worker_threads_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    active_thread_count: Arc<AtomicUsize>,
    pub min_threads: usize,
    pub max_threads: usize,
    signal_tx: mpsc::Sender<SystemSignal>,
    local_stats: Arc<LocalStats>,
    is_shutting_down: Arc<AtomicBool>,
    desired_thread_count: Arc<AtomicUsize>,
    last_scaling_time: Arc<Mutex<u64>>,
    last_self_overload_time: Arc<Mutex<u64>>,
    pub scale_down_cooldown: u64,
    pressure: Arc<AtomicUsize>,
}

impl OmegaNode {
    pub fn new(
        node_id: NodeId,
        queue_capacity: usize,
        min_threads: usize,
        max_threads: usize,
        signal_tx: mpsc::Sender<SystemSignal>,
        scale_down_cooldown_override: Option<u64>,
    ) -> Result<Self, String> {
        if min_threads == 0 { return Err("min_threads cannot be 0".to_string()); }
        if max_threads < min_threads { return Err("max_threads cannot be less than min_threads".to_string()); }

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
            last_self_overload_time: Arc::new(Mutex::new(elapsed_ns().saturating_sub(3600 * NANOS_PER_SEC))),
            scale_down_cooldown: scale_down_cooldown_override.unwrap_or(5 * NANOS_PER_SEC),
            pressure: pressure_arc,
        };

        for _ in 0..node.min_threads {
            node.spawn_worker_thread(false);
        }
        node.update_pressure();

        Ok(node)
    }

    // Unchanged methods (update_pressure, get_pressure, etc.)
    fn update_pressure(&self) {
        let q = self.task_queue.len() as f64;
        let c = self.active_thread_count.load(Ordering::Relaxed) as f64;
        let k = self.task_queue.capacity() as f64;
        let pressure_float = if c > 0.0 && k > 0.0 {
            (q/k) * 100.0
        } else if q > 0.0 { 100.0 } else { 0.0 };
        self.pressure.store(pressure_float.max(0.0).min(100.0) as usize, Ordering::Relaxed);
    }

    pub fn get_pressure(&self) -> usize { self.pressure.load(Ordering::Relaxed) }
    pub fn max_pressure(&self) -> usize { 100 }

    pub fn get_pressure_level(&self) -> PressureLevel {
        match self.get_pressure() {
            0 => PressureLevel::Empty,
            1..=25 => PressureLevel::Low,
            26..=75 => PressureLevel::Normal,
            76..=99 => PressureLevel::High,
            _ => PressureLevel::Full,
        }
    }
    
    fn spawn_worker_thread(&self, triggered_by_overload: bool) {
        if self.active_threads() >= self.max_threads { return; }
        if self.is_shutting_down.load(Ordering::Relaxed) { return; }

        self.active_thread_count.fetch_add(1, Ordering::SeqCst);
        self.desired_thread_count.store(self.active_threads(), Ordering::SeqCst);

        let now = elapsed_ns();
        *self.last_scaling_time.lock().unwrap() = now;
        if triggered_by_overload {
            *self.last_self_overload_time.lock().unwrap() = now;
        }

        self.update_pressure();

        let worker_context = self.clone_for_worker();
        let handle = thread::Builder::new()
            .name(format!("omega-node-{}-worker-{}", self.node_id.0, self.active_threads()))
            .spawn(move || worker_context.run_loop())
            .expect("Failed to spawn worker thread");
        
        self.worker_threads_handles.lock().unwrap().push(handle);
    }

    /// Internal method to enqueue a task. Called by UltraOmegaSystem.
    pub(crate) fn submit_task(&self, task: Task) -> Result<(), NodeError> {
        if self.is_shutting_down.load(Ordering::Relaxed) { return Err(NodeError::NodeShuttingDown); }
        self.local_stats.task_submitted();
        match self.task_queue.enqueue(task) {
            Ok(()) => {
                self.update_pressure();
                if self.get_pressure_level() == PressureLevel::High || self.get_pressure_level() == PressureLevel::Full {
                    self.spawn_worker_thread(true);
                }
                Ok(())
            }
            Err(QueueError::Full) => {
                *self.last_self_overload_time.lock().unwrap() = elapsed_ns();
                self.spawn_worker_thread(true);
                Err(NodeError::QueueFull)
            }
            Err(e) => Err(NodeError::from(e)),
        }
    }

    pub fn shutdown(&self) {
        if self.is_shutting_down.compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
            self.task_queue.close();
            self.desired_thread_count.store(0, Ordering::SeqCst);
            let mut workers = self.worker_threads_handles.lock().unwrap();
            for handle in workers.drain(..) { let _ = handle.join(); }
        }
    }

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

    pub fn id(&self) -> NodeId { self.node_id }
    pub fn active_threads(&self) -> usize { self.active_thread_count.load(Ordering::Relaxed) }
    pub fn desired_threads(&self) -> usize { self.desired_thread_count.load(Ordering::Relaxed) }
}

// --- WorkerContext (No longer generic) ---
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
    fn run_loop(self) {
        let mut retired_by_choice = false;
        loop {
            if self.is_shutting_down.load(Ordering::Relaxed) { break; }

            // Send Dequeued signal *before* processing
            if let Some(task) = self.task_queue.dequeue() {
                self.update_pressure_from_context();
                let _ = self.signal_tx.send(SystemSignal::TaskDequeuedByWorker {
                    node_id: self.node_id,
                    task_id: task.id,
                });
                self.process_task(task);
            } else {
                if self.is_shutting_down.load(Ordering::Relaxed) { break; }
                
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

    fn process_task(&self, task: Task) {
        let task_id = task.id;
        let start_time_ns = elapsed_ns();

        // The task's run method executes the work, sends the result,
        // and returns the internal outcome.
        let outcome = task.run();

        let duration_ns = elapsed_ns().saturating_sub(start_time_ns);

        // For local stats, we can still decide how to count success.
        // Let's count 'Success' as success, and all others as failure.
        let was_logically_successful = outcome == TaskExecutionOutcome::Success;
        self.local_stats.record_task_outcome(duration_ns, was_logically_successful);

        // Always send TaskProcessed signal, regardless of outcome.
        // This signal is about the worker's lifecycle, not the task's logical result.
        if !self.is_shutting_down.load(Ordering::Relaxed) {
            let signal = SystemSignal::TaskProcessed {
                node_id: self.node_id,
                task_id,
                duration_micros: duration_ns / 1000,
            };
            let _ = self.signal_tx.send(signal);
        }
    }
    
    // Unchanged helper methods (consider_scaling_down, etc.)
    fn consider_scaling_down(&self) {
        let current_desired = self.desired_thread_count.load(Ordering::SeqCst);
        if current_desired <= self.min_threads { return; }
        let now = elapsed_ns();
        let last_scale_time = *self.last_scaling_time.lock().unwrap();
        let last_overload_time = *self.last_self_overload_time.lock().unwrap();
        if now.saturating_sub(last_scale_time) < self.scale_down_cooldown { return; }
        if now.saturating_sub(last_overload_time) < self.scale_down_cooldown { return; }
        let pressure = self.get_pressure_from_context();
        let pressure_level = self.get_pressure_level_from_pressure(pressure);
        if pressure_level == PressureLevel::Empty || pressure_level == PressureLevel::Low {
            if self.desired_thread_count.compare_exchange(current_desired, current_desired - 1, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                *self.last_scaling_time.lock().unwrap() = now;
            }
        }
    }

    fn check_and_attempt_self_retirement(&self) -> bool {
        let current_active = self.active_thread_count.load(Ordering::SeqCst);
        if current_active <= self.min_threads || current_active <= self.desired_thread_count.load(Ordering::SeqCst) {
            return false;
        }
        if self.active_thread_count.compare_exchange(current_active, current_active - 1, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
            *self.last_scaling_time.lock().unwrap() = elapsed_ns();
            self.update_pressure_from_context();
            true
        } else {
            false
        }
    }

    fn get_pressure_from_context(&self) -> usize {
        let q = self.task_queue.len() as f64;
        let c = self.active_thread_count.load(Ordering::Relaxed) as f64;
        let k = self.task_queue.capacity() as f64;
        let pressure_float = if c > 0.0 && k > 0.0 { (q/k) * 100.0 } else if q > 0.0 { 100.0 } else { 0.0 };
        pressure_float.max(0.0).min(100.0) as usize
    }

    fn get_pressure_level_from_pressure(&self, pressure: usize) -> PressureLevel {
        match pressure {
            0 => PressureLevel::Empty, 1..=25 => PressureLevel::Low,
            26..=75 => PressureLevel::Normal, 76..=99 => PressureLevel::High, _ => PressureLevel::Full,
        }
    }

    fn update_pressure_from_context(&self) {
        let pressure = self.get_pressure_from_context();
        self.node_pressure_atomic.store(pressure, Ordering::Relaxed);
    }
}

impl Drop for OmegaNode {
    fn drop(&mut self) {
        if !self.is_shutting_down.load(Ordering::Relaxed) {
            self.shutdown();
        }
    }
}

impl From<QueueError> for NodeError {
    fn from(qe: QueueError) -> Self {
        match qe {
            QueueError::Full => NodeError::QueueFull,
            QueueError::Closed => NodeError::QueueClosed,
            QueueError::SendError => NodeError::SignalSendError,
            QueueError::Empty => NodeError::QueueClosed,
        }
    }
}