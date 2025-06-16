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

/// Defines the high-level I/O operations that the client (e.g., P2P library) can request.
///
/// This enum represents the "explicit intent" API for interacting with the
/// `GlobalReactor`, allowing for various network communication patterns.
#[derive(Debug, Clone)]
pub enum IoOp {
    /// A one-shot UDP operation: sends a packet to a peer and listens for a single
    /// response packet from any source on the bound UDP socket.
    ///
    /// This is useful for connectionless protocols like discovery or ping/pong mechanisms.
    UdpSendAndListenOnce {
        /// The address of the peer to send the initial packet to.
        peer_addr: SocketAddr,
        /// The data payload to send.
        data_to_send: Vec<u8>,
        // Note: A timeout for this operation is handled by the `GlobalReactor` internally.
    },

    // --- TCP Operations ---
    /// Binds a TCP listener to a specified address and prepares it for accepting connections.
    ///
    /// This is a one-shot setup operation that returns a `TcpListenerReady` output
    /// upon successful binding. Subsequent `TcpAccept` operations are needed to
    /// handle incoming connections.
    TcpListen {
        /// The local address to bind the TCP listener to.
        addr: SocketAddr,
    },

    /// Waits for the next incoming connection on a specific TCP listener.
    ///
    /// This is a one-shot "wait" operation. When a new connection is accepted,
    /// a `NewConnectionAccepted` output is returned. The listener remains active
    /// and can accept further connections via new `TcpAccept` requests.
    TcpAccept {
        /// The `Token` of the TCP listener to accept a connection from.
        listener_token: Token,
    },

    /// Initiates a non-blocking TCP connection to a remote peer.
    ///
    /// The connection attempt proceeds asynchronously. A `TcpConnectionEstablished`
    /// output is returned upon successful connection.
    TcpConnect {
        /// The address of the remote peer to connect to.
        peer_addr: SocketAddr,
    },

    /// Sends data over an existing, established TCP connection.
    ///
    /// This operation attempts to write the provided data to the socket.
    /// A `TcpDataSent` output indicates how many bytes were successfully sent.
    TcpSend {
        /// The `Token` of the established TCP connection.
        connection_token: Token,
        /// The data to send.
        data: Vec<u8>,
    },

    /// Issues a request to receive data from an established TCP connection.
    ///
    /// This operation attempts to read up to `max_bytes` from the socket.
    /// A `TcpDataReceived` output contains the data read. An empty `Vec<u8>`
    /// in `TcpDataReceived` indicates that the peer has gracefully shut down
    /// their write half of the connection.
    TcpReceive {
        /// The `Token` of the established TCP connection.
        connection_token: Token,
        /// The maximum number of bytes to attempt to read.
        max_bytes: usize,
    },

    /// Closes an established TCP connection or an active listener.
    ///
    /// This operation releases the associated system resources.
    /// A `ConnectionClosed` output confirms the closure.
    CloseConnection {
        /// The `Token` of the connection or listener to close.
        connection_token: Token,
    },
}

/// Defines the successful outcomes of an `IoOp`.
///
/// This enum represents the data returned to the client in the `Ok()` variant
/// of a `TaskHandle` result after an I/O operation completes successfully.
#[derive(Debug)]
pub enum IoOutput {
    // --- UDP ---
    /// Returned on a successful `IoOp::UdpSendAndListenOnce` operation.
    UdpResponse {
        /// The data received in the UDP response.
        data: Vec<u8>,
        /// The address from which the UDP response was received.
        from_addr: SocketAddr,
    },

    // --- TCP ---
    /// Returned on a successful `IoOp::TcpListen` operation.
    TcpListenerReady {
        /// The `Token` assigned to the newly created TCP listener.
        listener_token: Token,
        /// The actual local address the listener is bound to.
        local_addr: SocketAddr,
    },

    /// Returned on a successful `IoOp::TcpAccept` operation.
    NewConnectionAccepted {
        /// The `Token` for the newly accepted client connection.
        connection_token: Token,
        /// The remote address of the newly accepted peer.
        peer_addr: SocketAddr,
        /// The `Token` of the listener that accepted this connection.
        listener_token: Token,
    },

    /// Returned on a successful `IoOp::TcpConnect` operation.
    TcpConnectionEstablished {
        /// The `Token` assigned to the newly established TCP connection.
        connection_token: Token,
        /// The remote address of the connected peer.
        peer_addr: SocketAddr,
    },

    /// Returned on a successful `IoOp::TcpSend` operation.
    TcpDataSent {
        /// The number of bytes successfully sent.
        bytes_sent: usize,
    },

    /// Data received from a `IoOp::TcpReceive` operation.
    /// An empty `Vec` indicates graceful shutdown by the peer.
    TcpDataReceived {
        /// The data received from the connection.
        data: Vec<u8>,
    },

    /// Confirmation that a connection or listener was successfully closed.
    ConnectionClosed,
}

// --- Private Reactor Submodule ---
// The implementation details of the `GlobalReactor` are internal and not part of the public API.
pub(crate) mod poller;
pub(crate) mod reactor;
