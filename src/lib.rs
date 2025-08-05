//! # VibeSystem: Simple Parallelism for Rust
//!
//! The `vibe-code` library, also known as the VibeSystem, provides an incredibly
//! simple way to run your code in parallel. It's designed for developers who want to speed
//! up their applications without getting bogged down in the complexities of manual thread
//! management, channels, or async runtimes.
//!
//! The core idea is to take a list of tasks and run them all at once, intelligently
//! distributing them across your CPU cores.
//!
//! # Key Components
//!
//! - **`VibeSystem`**: The main entry point. You create one of these and use it to run your jobs.
//! - **`Job`**: A handle to a piece of work that is running in the background. You can call
//!   `.get()` on a `Job` to wait for its result.
//! - **`collect`**: A helper function to wait for a `Vec<Job<T>>` to finish and get a `Vec<T>` of results.

#![allow(dead_code)]
#![allow(unused_imports)]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

// Internal modules that power the system.
pub mod node;
pub mod queue;
pub mod signals;
pub mod task;
pub mod types;
pub mod utils;
pub mod vibe;
pub mod vibe_code;

// Re-exports for the public API.
pub use task::{Priority, TaskError, TaskHandle};
pub use types::NodeError;
pub use vibe::{Job, VibeSystem, collect};
pub use vibe_code::{UltraVibeBuilder, UltraVibeSystem};
