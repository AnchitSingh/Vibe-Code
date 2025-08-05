//! The user-facing "vibe" interface.
//!
//! This module provides the dead-simple API for users who just want to run
//! code in parallel without thinking about the underlying complexity. It
//! defines the `VibeSystem`, the `Job` handle, and the `collect` utility.

use crate::task::{Priority, TaskHandle};
use crate::utils::timer_init;
use crate::utils::{VibeRng, elapsed_ns};
use crate::vibe_code::UltraVibeSystem;
use std::sync::{Arc, mpsc};
thread_local! {
    static LOCAL_RNG: std::cell::RefCell<VibeRng> = std::cell::RefCell::new(VibeRng::new(elapsed_ns()));
}

/// A handle to a job that is running in the background.
pub struct Job<T> {
    handle: TaskHandle<T>,
}

impl<T> Job<T> {
    /// Waits for the job to finish and returns its result.
    ///
    /// # Panics
    /// Panics if the job failed (e.g., the user's function panicked) or if the
    /// system was shut down before the job could complete.
    pub fn get(self) -> T {
        match self.handle.recv_result() {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => panic!("❌ Your job failed! Check your function for bugs."),
            Err(_) => panic!("❌ Job was cancelled - did you shut down the system?"),
        }
    }

    /// Checks if the job is finished without blocking.
    ///
    /// Returns `Some(result)` if the job is done, or `None` if it's still running.
    ///
    /// # Panics
    /// Panics if the job failed.
    pub fn peek(self) -> Option<T> {
        match self.handle.try_recv_result() {
            Ok(Ok(result)) => Some(result),
            Ok(Err(_)) => panic!("❌ Your job failed! Check your function for bugs."),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => panic!("❌ Job was cancelled"),
        }
    }

    /// Returns `true` if the job has finished (either completed or failed).
    pub fn is_done(&self) -> bool {
        matches!(
            self.handle.try_recv_result(),
            Ok(_) | Err(mpsc::TryRecvError::Disconnected)
        )
    }
}

/// The main entry point for the simple parallel execution system.
pub struct VibeSystem {
    /// A shared pointer to the internal task scheduling and execution engine.
    inner: Arc<UltraVibeSystem>,
}

impl VibeSystem {
    /// Creates a new `VibeSystem` with sensible default settings.
    ///
    /// This initializes the background worker pools and load balancer.
    pub fn new() -> Self {
        let _ = timer_init();
        Self {
            inner: Arc::new(
                UltraVibeSystem::builder()
                    .with_nodes(80)
                    .with_super_nodes(40)
                    .build(),
            ),
        }
    }

    /// Runs a function with input data in parallel.
    ///
    /// This method takes a function and its input data, submits it to the system
    /// for background execution, and immediately returns a `Job` handle.
    ///
    /// # Example
    /// `let job = system.run(process_data, my_data);`
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

    /// Runs a function with no input data in parallel.
    ///
    /// This is a convenience wrapper around `run` for functions that take no arguments.
    ///
    /// # Example
    /// `let job = system.go(|| expensive_calculation());`
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

/// Waits for a vector of jobs to finish and collects their results in order.
pub fn collect<T>(jobs: Vec<Job<T>>) -> Vec<T> {
    jobs.into_iter().map(|job| job.get()).collect()
}
