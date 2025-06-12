use crate::queue::{OmegaQueue, PressureLevel, QueueError};
use crate::signals::{NodeId, SystemSignal};
use crate::task::Task;
use crate::types::{LocalStats, NodeError};
// New import for the omega timer functionality
use omega::omega_timer::omega_time_ns;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, AtomicU64, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

// Cache-friendly atomic time representation (nanoseconds since omega timer epoch)
type AtomicInstant = AtomicU64;

// The `instant_to_nanos` and `nanos_to_instant` functions have been removed,
// as we now use `omega_time_ns()` from `omega_timer.rs` directly.

pub struct OmegaNode<Output: Send + 'static> {
    pub node_id: NodeId,
    pub task_queue: OmegaQueue<Output>,
    
    // Hot path atomics - grouped for cache locality
    active_thread_count: Arc<AtomicUsize>,
    desired_thread_count: Arc<AtomicUsize>,
    pressure: Arc<AtomicUsize>,
    is_shutting_down: Arc<AtomicBool>,
    
    // Timing atomics - lock-free for better performance
    last_scaling_time: Arc<AtomicInstant>,
    last_self_overload_time: Arc<AtomicInstant>,
    
    // Cold path data
    worker_threads_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    pub min_threads: usize,
    pub max_threads: usize,
    signal_tx: mpsc::Sender<SystemSignal>,
    local_stats: Arc<LocalStats>,
    pub scale_down_cooldown: Duration,
    scale_down_cooldown_nanos: u64, // Pre-computed for faster comparisons
}


impl<Output: Send + 'static> OmegaNode<Output> {
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
        let worker_threads_handles_arc = Arc::new(Mutex::new(Vec::with_capacity(max_threads)));
        let is_shutting_down_arc = Arc::new(AtomicBool::new(false));

        let desired_thread_count_arc = Arc::new(AtomicUsize::new(min_threads));

        // Use omega_time_ns for all time points.
        // This requires omega_timer_init() to have been called at application startup.
        let now_ns = omega_time_ns();
        let one_hour_in_ns = Duration::from_secs(3600).as_nanos() as u64;
        let distant_past_ns = now_ns.saturating_sub(one_hour_in_ns);

        let final_scale_down_cooldown = scale_down_cooldown_override.unwrap_or(Duration::from_secs(5));
        let scale_down_cooldown_nanos = final_scale_down_cooldown.as_nanos() as u64;

        let pressure_arc = Arc::new(AtomicUsize::new(0));
        let last_scaling_time_arc = Arc::new(AtomicInstant::new(now_ns));
        let last_self_overload_time_arc = Arc::new(AtomicInstant::new(distant_past_ns));

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
            last_scaling_time: last_scaling_time_arc,
            last_self_overload_time: last_self_overload_time_arc,
            scale_down_cooldown: final_scale_down_cooldown,
            scale_down_cooldown_nanos,
            pressure: pressure_arc,
        };

        // Pre-spawn minimum threads
        for _ in 0..node.min_threads {
            node.spawn_worker_thread_internal(false);
        }

        node.desired_thread_count.store(
            node.active_thread_count.load(Ordering::Acquire),
            Ordering::Release,
        );

        node.update_pressure_fast();

        Ok(node)
    }

    #[inline]
    fn update_pressure_fast(&self) {
        let q_len = self.task_queue.len();
        let active_threads = self.active_thread_count.load(Ordering::Relaxed);
        let current_pressure = q_len.saturating_add(active_threads);
        self.pressure.store(current_pressure, Ordering::Relaxed);
    }

    #[inline]
    pub fn get_pressure(&self) -> usize {
        self.pressure.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn max_pressure(&self) -> usize {
        self.task_queue.capacity().saturating_add(self.max_threads)
    }

    fn spawn_worker_thread(&self, triggered_by_overload: bool) {
        self.spawn_worker_thread_internal(triggered_by_overload);
    }

    #[inline]
    fn spawn_worker_thread_internal(&self, triggered_by_overload: bool) {
        let current_active = self.active_thread_count.load(Ordering::Acquire);
        if current_active >= self.max_threads {
            return;
        }

        // Atomic increment with bounds check
        let previous_active = match self.active_thread_count.compare_exchange_weak(
            current_active,
            current_active + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(prev) => prev,
            Err(_) => return, // Another thread beat us to it
        };

        if previous_active >= self.max_threads {
            self.active_thread_count.fetch_sub(1, Ordering::AcqRel);
            return;
        }

        let new_thread_count = previous_active + 1;
        self.desired_thread_count.store(new_thread_count, Ordering::Release);

        let now_nanos = omega_time_ns();
        self.last_scaling_time.store(now_nanos, Ordering::Release);
        
        if triggered_by_overload {
            self.last_self_overload_time.store(now_nanos, Ordering::Release);
        }

        self.update_pressure_fast();

        // Create worker context with all necessary data
        let worker_context = WorkerContext {
            node_id: self.node_id,
            task_queue: self.task_queue.clone(),
            local_stats: Arc::clone(&self.local_stats),
            signal_tx: self.signal_tx.clone(),
            is_shutting_down: Arc::clone(&self.is_shutting_down),
            active_thread_count: Arc::clone(&self.active_thread_count),
            desired_thread_count: Arc::clone(&self.desired_thread_count),
            min_threads: self.min_threads,
            last_scaling_time: Arc::clone(&self.last_scaling_time),
            last_self_overload_time: Arc::clone(&self.last_self_overload_time),
            scale_down_cooldown_nanos: self.scale_down_cooldown_nanos,
            pressure: Arc::clone(&self.pressure),
        };

        let join_handle = thread::Builder::new()
            .name(format!("omega-node-{}-worker-{}", self.node_id.0, new_thread_count))
            .spawn(move || worker_thread_main(worker_context))
            .expect("Failed to spawn worker thread");

        // Only lock when we know we need to add the handle
        if let Ok(mut workers_guard) = self.worker_threads_handles.try_lock() {
            workers_guard.push(join_handle);
        } else {
            // If we can't get the lock immediately, spawn a detached thread to handle it
            let handles = Arc::clone(&self.worker_threads_handles);
            thread::spawn(move || {
                if let Ok(mut workers_guard) = handles.lock() {
                    workers_guard.push(join_handle);
                }
            });
        }
    }

    pub fn submit_task(&self, task: Task<Output>) -> Result<(), NodeError> {
        if self.is_shutting_down.load(Ordering::Acquire) {
            return Err(NodeError::NodeShuttingDown);
        }
        
        self.local_stats.task_submitted();

        match self.task_queue.enqueue(task) {
            Ok(()) => {
                self.update_pressure_fast();
                Ok(())
            }
            Err(QueueError::Full) => {
                let now_nanos = omega_time_ns();
                self.last_self_overload_time.store(now_nanos, Ordering::Release);
                self.spawn_worker_thread_internal(true);
                Err(NodeError::QueueFull)
            }
            Err(QueueError::Closed) => Err(NodeError::QueueClosed),
            Err(QueueError::SendError) => Err(NodeError::SignalSendError),
            Err(QueueError::Empty) => {
                eprintln!("OmegaNode::submit_task received unexpected QueueError::Empty from enqueue");
                Err(NodeError::QueueClosed)
            }
        }
    }

    pub fn shutdown(&self) {
        if self.is_shutting_down.compare_exchange(
            false,
            true,
            Ordering::AcqRel,
            Ordering::Acquire,
        ).is_ok() {
            self.task_queue.close();
            self.desired_thread_count.store(0, Ordering::Release);

            if let Ok(mut workers_guard) = self.worker_threads_handles.lock() {
                for handle in workers_guard.drain(..) {
                    if let Err(e) = handle.join() {
                        eprintln!("Node {} worker thread panicked during shutdown: {:?}", self.node_id, e);
                    }
                }
            }
        }
    }

    #[inline]
    pub fn id(&self) -> NodeId {
        self.node_id
    }

    #[inline]
    pub fn active_threads(&self) -> usize {
        self.active_thread_count.load(Ordering::Acquire)
    }

    #[inline]
    pub fn desired_threads(&self) -> usize {
        self.desired_thread_count.load(Ordering::Acquire)
    }
}

// Consolidated worker context to reduce parameter passing
struct WorkerContext<Output: Send + 'static> {
    node_id: NodeId,
    task_queue: OmegaQueue<Output>,
    local_stats: Arc<LocalStats>,
    signal_tx: mpsc::Sender<SystemSignal>,
    is_shutting_down: Arc<AtomicBool>,
    active_thread_count: Arc<AtomicUsize>,
    desired_thread_count: Arc<AtomicUsize>,
    min_threads: usize,
    last_scaling_time: Arc<AtomicInstant>,
    last_self_overload_time: Arc<AtomicInstant>,
    scale_down_cooldown_nanos: u64,
    pressure: Arc<AtomicUsize>,
}

#[inline(never)] // Keep this cold to optimize hot paths
fn worker_thread_main<Output: Send + 'static>(ctx: WorkerContext<Output>) {
    let mut retired_by_choice = false;
    let mut idle_cycles = 0u32;
    const MAX_IDLE_CYCLES: u32 = 10;

    loop {
        // Fast path: try to get work immediately
        let task_option = ctx.task_queue.dequeue();
        
        if let Some(task) = task_option {
            idle_cycles = 0;
            execute_task(&ctx, task);
            update_pressure_and_check_retirement(&ctx, &mut retired_by_choice);
            if retired_by_choice {
                break;
            }
            continue;
        }

        // No work available - check if we should exit
        if ctx.is_shutting_down.load(Ordering::Acquire) {
            break;
        }

        // Consider scaling decisions only after some idle cycles
        idle_cycles = idle_cycles.saturating_add(1);
        if idle_cycles >= MAX_IDLE_CYCLES {
            consider_scaling_decisions(&ctx);
            if check_self_retirement(&ctx) {
                retired_by_choice = true;
                break;
            }
            idle_cycles = 0;
        }

        // Yield to scheduler
        thread::yield_now();
    }

    // Clean up if not retired by choice
    if !retired_by_choice {
        ctx.active_thread_count.fetch_sub(1, Ordering::AcqRel);
        update_pressure_only(&ctx);
    }
}

#[inline]
fn execute_task<Output: Send + 'static>(ctx: &WorkerContext<Output>, task: Task<Output>) {
    let task_id = task.id;
    // Measure duration using the consistent omega timer
    let start_ns = omega_time_ns();
    let result = task.execute();
    let end_ns = omega_time_ns();
    let duration = Duration::from_nanos(end_ns.saturating_sub(start_ns));
    let success = result.is_ok();

    if !ctx.is_shutting_down.load(Ordering::Acquire) {
        ctx.local_stats.record_task_outcome(duration, success);

        let signal = if success {
            SystemSignal::TaskCompleted {
                node_id: ctx.node_id,
                task_id,
                duration_micros: duration.as_micros() as u64,
            }
        } else {
            SystemSignal::TaskFailed {
                node_id: ctx.node_id,
                task_id,
            }
        };

        let _ = ctx.signal_tx.send(signal);
    }
}

#[inline]
fn update_pressure_and_check_retirement<Output: Send + 'static>(
    ctx: &WorkerContext<Output>,
    retired_by_choice: &mut bool,
) {
    update_pressure_only(ctx);
    
    if !ctx.is_shutting_down.load(Ordering::Acquire) {
        consider_scaling_decisions(ctx);
        if check_self_retirement(ctx) {
            *retired_by_choice = true;
        }
    }
}

#[inline]
fn update_pressure_only<Output: Send + 'static>(ctx: &WorkerContext<Output>) {
    let q_len = ctx.task_queue.len();
    let active_threads = ctx.active_thread_count.load(Ordering::Relaxed);
    let current_pressure = q_len.saturating_add(active_threads);
    ctx.pressure.store(current_pressure, Ordering::Relaxed);
}

fn consider_scaling_decisions<Output: Send + 'static>(ctx: &WorkerContext<Output>) {
    let current_desired = ctx.desired_thread_count.load(Ordering::Acquire);
    
    if current_desired <= ctx.min_threads {
        return;
    }

    let now_nanos = omega_time_ns();
    
    // Lock-free cooldown checks
    let last_scale_nanos = ctx.last_scaling_time.load(Ordering::Acquire);
    if now_nanos.saturating_sub(last_scale_nanos) < ctx.scale_down_cooldown_nanos {
        return;
    }
    
    let last_overload_nanos = ctx.last_self_overload_time.load(Ordering::Acquire);
    if now_nanos.saturating_sub(last_overload_nanos) < ctx.scale_down_cooldown_nanos {
        return;
    }

    // Check queue pressure and attempt to reduce desired threads
    match ctx.task_queue.pressure_level() {
        PressureLevel::Empty | PressureLevel::Low => {
            if ctx.desired_thread_count.compare_exchange_weak(
                current_desired,
                current_desired - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ).is_ok() {
                ctx.last_scaling_time.store(now_nanos, Ordering::Release);
            }
        }
        _ => {}
    }
}

fn check_self_retirement<Output: Send + 'static>(ctx: &WorkerContext<Output>) -> bool {
    let current_active = ctx.active_thread_count.load(Ordering::Acquire);
    let desired_active = ctx.desired_thread_count.load(Ordering::Acquire);

    if current_active <= ctx.min_threads || current_active <= desired_active {
        return false;
    }

    // Attempt to retire this thread
    match ctx.active_thread_count.compare_exchange_weak(
        current_active,
        current_active - 1,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => {
            let now_nanos = omega_time_ns();
            ctx.last_scaling_time.store(now_nanos, Ordering::Release);
            update_pressure_only(ctx);
            true
        }
        Err(_) => false,
    }
}

impl<Output: Send + 'static> Drop for OmegaNode<Output> {
    fn drop(&mut self) {
        if !self.is_shutting_down.load(Ordering::Acquire) {
            self.shutdown();
        }
    }
}