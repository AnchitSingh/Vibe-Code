//! The `io` module provides the core abstractions for asynchronous network I/O
//! operations within the CPU Circulatory System.
//!
//! It defines the high-level `IoOp` enum for client-requested I/O actions
//! and the `IoOutput` enum for the results of these operations.
//! This module also re-exports internal components like the `reactor` and `poller`
//! for system-level I/O management.

use std::net::SocketAddr;

/// A unique, monotonic identifier for a specific I/O connection or listener instance.
///
/// This `Token` is used to uniquely identify I/O resources managed by the `GlobalReactor`
/// and helps prevent issues like the "File Descriptor Reuse" race condition.
pub type Token = u64;
use ovp::{DroneId, OmegaSocket};
/// Defines the high-level I/O operations that the client (e.g., P2P library) can request.
///
/// This enum represents the "explicit intent" API for interacting with the
/// `GlobalReactor`, allowing for various network communication patterns.
#[derive(Debug, Clone)]
pub enum IoOp {
    // --- OVP (Omega Volumetric Protocol) Operations ---

    /// Initializes a raw OVP socket on a specific network interface.
    /// This is a one-shot setup operation.
    OvpInit {
        interface: String,
        my_drone_id: DroneId,
    },

    /// Emits an OVP frame using a specific OVP socket. This is a fire-and-forget operation.
    OvpEmit {
        socket_token: Token,
        targets: Option<Vec<DroneId>>, // None or empty Vec for broadcast
        payload: Vec<u8>,
    },

    /// Waits for the next valid OVP frame on a specific socket.
    /// This is a one-shot "wait" operation.
    OvpReceive {
        socket_token: Token,
    },
}

/// Defines the successful outcomes of an `IoOp`.
///
/// This enum represents the data returned to the client in the `Ok()` variant
/// of a `TaskHandle` result after an I/O operation completes successfully.
#[derive(Debug)]
pub enum IoOutput {
    // --- OVP (Omega Volumetric Protocol) ---

    /// Returned on a successful `IoOp::OvpInit`.
    OvpSocketReady {
        /// The token assigned to the newly created OVP socket.
        socket_token: Token,
    },

    /// Confirmation that an `OvpEmit` operation was successfully sent to the network interface.
    OvpEmitSuccess,
    
    /// A valid OVP frame was received.
    OvpFrameReceived {
        /// The payload of the received OVP frame.
        payload: Vec<u8>,
    },
}

// --- Private Reactor Submodule ---
// The implementation details of the `GlobalReactor` are internal and not part of the public API.
pub(crate) mod poller;
pub(crate) mod reactor;
