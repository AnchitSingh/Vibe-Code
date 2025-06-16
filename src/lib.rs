//! The CPU Circulatory System is a Rust library designed for efficient and scalable
//! task management and execution across multiple processing units (nodes).
//!
//! It provides a robust framework for submitting, routing, and processing
//! both CPU-bound and I/O-bound tasks, leveraging asynchronous I/O and
//! intelligent load balancing strategies.
//!
//! # Features
//!
//! - **`OmegaNode`**: Individual processing units capable of executing tasks.
//! - **`UltraOmegaSystem`**: The central orchestrator for managing `OmegaNode`s
//!   and routing tasks.
//! - **Asynchronous I/O**: Integrated `GlobalReactor` for non-blocking network operations.
//! - **Task Queues**: Efficient, bounded queues for managing task backpressure.
//! - **Load Balancing**: "Power of K Choices" routing strategy for optimal task distribution.
//! - **System Signals**: Real-time feedback on node and task states.
//! - **Builder Pattern**: Flexible system configuration through `UltraOmegaBuilder`.
//!
//! # Modules
//!
//! - `monitor`: (Potentially for system monitoring and metrics)
//! - `node`: Defines the `OmegaNode` and its worker threads.
//! - `queue`: Implements the `OmegaQueue` for task buffering.
//! - `signals`: Defines system-wide communication signals and unique identifiers.
//! - `task`: Defines the `Task` abstraction and `TaskHandle` for result retrieval.
//! - `types`: Contains common error types and statistical structures.
//! - `ultra_omega`: The main system orchestration logic, including `UltraOmegaSystem` and its builder.
//! - `io`: The asynchronous I/O subsystem, including `Poller` and `GlobalReactor`.

#![allow(dead_code)] // Temporarily allow dead code during development
#![allow(unused_imports)] // Temporarily allow unused imports during development

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

// --- Core Modules ---
pub mod io;
pub mod monitor;
pub mod node;
pub mod queue;
pub mod signals;
pub mod task;
pub mod types;
pub mod ultra_omega; // I/O Subsystem Module

// --- Public API Re-exports ---
// This section defines the clean public API for external crates (e.g., a P2P library) to use.

/// Re-exports the main system entry point for creating and managing the CPU Circulatory System.
pub use ultra_omega::UltraOmegaSystem;

/// Re-exports core types for interacting with tasks:
/// - `TaskHandle`: For awaiting task results.
/// - `TaskError`: Errors that can occur during task execution.
/// - `Priority`: Task priority levels.
pub use task::{Priority, TaskError, TaskHandle};

/// Re-exports `NodeError`, representing errors specific to `OmegaNode` operations.
pub use types::NodeError;

/// Re-exports public I/O related types:
/// - `IoOp`: Defines high-level asynchronous I/O operations.
/// - `IoOutput`: Represents the successful results of I/O operations.
pub use io::{IoOp, IoOutput};
