//! The Vibe System is a Rust library designed for efficient and scalable
//! task management and execution across multiple processing units (nodes).
//!
//! It provides a robust framework for submitting, routing, and processing
//! both CPU-bound and I/O-bound tasks, leveraging asynchronous I/O and
//! intelligent load balancing strategies.
//!
//! # Features
//!
//! - **`VibeNode`**: Individual processing units capable of executing tasks.
//! - **`UltraOmegaSystem`**: The central orchestrator for managing `VibeNode`s
//!   and routing tasks.
//! - **Asynchronous I/O**: Integrated `GlobalReactor` for non-blocking network operations.
//! - **Task Queues**: Efficient, bounded queues for managing task backpressure.
//! - **Load Balancing**: "Power of K Choices" routing strategy for optimal task distribution.
//! - **System Signals**: Real-time feedback on node and task states.
//! - **Builder Pattern**: Flexible system configuration through `UltraOmegaBuilder`.
//!
//! # Modules
//!
//! - `node`: Defines the `VibeNode` and its worker threads.
//! - `queue`: Implements the `VibeQueue` for task buffering.
//! - `signals`: Defines system-wide communication signals and unique identifiers.
//! - `task`: Defines the `Task` abstraction and `TaskHandle` for result retrieval.
//! - `types`: Contains common error types and statistical structures.
//! - `vibe_code`: The main system orchestration logic, including `UltraOmegaSystem` and its builder.
//! - `io`: The asynchronous I/O subsystem, including `Poller` and `GlobalReactor`.

#![allow(dead_code)] // Temporarily allow dead code during development
#![allow(unused_imports)] // Temporarily allow unused imports during development

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

// --- Core Modules ---
pub mod node;
pub mod queue;
pub mod signals;
pub mod task;
pub mod types;
pub mod utils;
pub mod vibe_code; // I/O Subsystem Module
// --- Public API Re-exports ---
// This section defines the clean public API for external crates (e.g., a P2P library) to use.

/// Re-exports the main system entry point for creating and managing the Vibe System.
pub use vibe_code::{UltraOmegaBuilder, UltraOmegaSystem};

/// Re-exports core types for interacting with tasks:
/// - `TaskHandle`: For awaiting task results.
/// - `TaskError`: Errors that can occur during task execution.
/// - `Priority`: Task priority levels.
pub use task::{Priority, TaskError, TaskHandle};

/// Re-exports `NodeError`, representing errors specific to `VibeNode` operations.
pub use types::NodeError;
pub mod vibe;
pub use vibe::{Job, VibeSystem, collect};
