//! The vibe interface - for coders who just want things to work
//! No channels, no Results, no complexity. Just run stuff and get stuff back.
//! RESPECTS THE MONA LISA - no manual thread management!

use crate::task::{Priority, TaskError, TaskHandle, TaskId};
use crate::types::NodeError;
use crate::utils::VibeRng;
use crate::utils::elapsed_ns;
use crate::vibe_code::UltraOmegaSystem;
use std::sync::{Arc, mpsc};
use std::time::Duration;

thread_local! {
    static LOCAL_RNG: std::cell::RefCell<VibeRng> = std::cell::RefCell::new(VibeRng::new(elapsed_ns()));
}

/// A job running in the background. Call `.get()` when you want the result.
pub struct Job<T> {
    handle: TaskHandle<T>,
}

impl<T> Job<T> {
    /// Get your result. Blocks until done.
    /// If it breaks, your program crashes with a helpful message.
    pub fn get(self) -> T {
        match self.handle.recv_result() {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => panic!("❌ Your job failed! Check your function for bugs."),
            Err(_) => panic!("❌ Job was cancelled - did you shut down the system?"),
        }
    }

    /// Check if your job is done without waiting.
    /// Returns Some(result) if ready, None if still working.
    pub fn peek(self) -> Option<T> {
        match self.handle.try_recv_result() {
            Ok(Ok(result)) => Some(result),
            Ok(Err(_)) => panic!("❌ Your job failed! Check your function for bugs."),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => panic!("❌ Job was cancelled"),
        }
    }

    /// Is the job finished? (doesn't consume the job)
    pub fn is_done(&self) -> bool {
        matches!(
            self.handle.try_recv_result(),
            Ok(_) | Err(mpsc::TryRecvError::Disconnected)
        )
    }
}

/// The dead simple parallel system. Create one, run stuff, get results back.
pub struct VibeSystem {
    inner: Arc<UltraOmegaSystem>,
}

impl VibeSystem {
    /// Create a new system. Just works with good defaults.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(
                UltraOmegaSystem::builder()
                    .with_nodes(80)
                    .with_super_nodes(40)
                    .build(),
            ),
        }
    }

    /// Run a function with data in parallel. Returns a Job you can .get() later.
    ///
    /// Example: `let job = system.run(compress_chunks, my_data);`
    pub fn run<F, T, R>(&self, func: F, data: T) -> Job<R>
    where
        F: FnOnce(T) -> R + Send + 'static,
        T: Send + 'static,
        R: Send + 'static,
    {
        LOCAL_RNG.with(|rng| {
            let work = move || Ok(func(data));

            match self
                .inner
                .submit_cpu_task(Priority::Normal, 10, work, &mut rng.borrow_mut())
            {
                Ok(handle) => Job { handle },
                Err(_) => panic!("🔥 System overloaded! Too many jobs running at once."),
            }
        })
    }

    /// Run a simple function in parallel (no input data needed).
    ///
    /// Example: `let job = system.go(|| expensive_calculation());`
    pub fn go<F, R>(&self, func: F) -> Job<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.run(|_| func(), ())
    }
}

impl Default for VibeSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Wait for all jobs to finish and collect results in order.
pub fn collect<T>(jobs: Vec<Job<T>>) -> Vec<T> {
    jobs.into_iter().map(|job| job.get()).collect()
}
