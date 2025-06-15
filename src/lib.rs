// src/lib.rs

#![allow(dead_code)] 
#![allow(unused_imports)]
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc
};
// --- Core Modules ---
pub mod monitor;
pub mod node;
pub mod queue;
pub mod signals;
pub mod task;
pub mod types;
pub mod ultra_omega;
// --- NEW: I/O Subsystem Module ---
pub mod io;

// --- Public API Re-exports ---
// This section will define the clean public API for our P2P lib to use.

// The main system entry point
pub use ultra_omega::UltraOmegaSystem; 

// Core types the client will interact with
pub use task::{TaskHandle, TaskError, Priority};
pub use types::NodeError;

// NEW: Public I/O related types
pub use io::{IoOp, IoOutput};


