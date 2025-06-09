//! # Omega - High-performance Rust Collections
//!
//! A collection of high-performance, memory-efficient data structures and algorithms.
//! Optimized for performance-critical applications.

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]
#![allow(dead_code)]
#![allow(unused_imports)]

pub mod ohs;
pub mod orrg;
pub mod os;
pub mod oufh;
pub mod borrg;

// Re-export commonly used items at the crate root for easier access
pub use ohs::OmegaHashSet;
pub use orrg::OmegaRng;
pub use os::random_k_combination_optimized;
pub use os::random_k_combination_zero_indexed;
pub use oufh::UltraFastHash;

/// Prelude module for convenient importing of common types and traits.
pub mod prelude {
    pub use crate::{
        ohs::OmegaHashSet,
        orrg::OmegaRng,
        os::random_k_combination_optimized,
        os::random_k_combination_zero_indexed,
        oufh::UltraFastHash,
    };
}
