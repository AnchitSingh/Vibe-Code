// src/io/mod.rs

use crate::task::TaskError;
use std::net::SocketAddr;

// --- Public I/O Enums ---

/// A unique, monotonic identifier for a specific I/O connection instance.
/// This is used to prevent the "FD Reuse" race condition.
pub type Token = u64;

/// Defines the high-level I/O operations that the client (P2P lib) can request.
/// This is the "explicit intent" API.
#[derive(Debug, Clone)]
pub enum IoOp {
    /// A one-shot UDP operation. Sends a packet to a peer and listens for a single
    /// response packet from any source. This is useful for discovery or ping/pong.
    UdpSendAndListenOnce {
        /// The address of the peer to send the initial packet to.
        peer_addr: SocketAddr,
        /// The data payload to send.
        data_to_send: Vec<u8>,
        // Note: A timeout will be handled by the Reactor internally.
    },
    /// Initiates a non-blocking TCP connection to a peer.
    TcpConnect {
        /// The address of the peer to connect to.
        peer_addr: SocketAddr,
    },
    /// Sends data over an existing, established TCP connection.
    TcpSend {
        /// The token identifying the established connection.
        connection_token: Token,
        /// The data payload to send.
        data: Vec<u8>,
    },
    /// Issues a request to receive data from an established TCP connection.
    TcpReceive {
        /// The token identifying the established connection.
        connection_token: Token,
        /// The maximum number of bytes to read in this operation.
        max_bytes: usize,
    },
    /// Closes an established TCP connection.
    CloseConnection {
        /// The token identifying the connection to close.
        connection_token: Token,
    },
    TcpListen {
        addr: SocketAddr,
    },
}

/// Defines the successful outcomes of an `IoOp`.
/// This is what the client receives in the `Ok()` variant of a `TaskHandle` result.
#[derive(Debug)]
pub enum IoOutput {
    /// The response received from a `UdpSendAndListenOnce` operation.
    UdpResponse {
        /// The data received.
        data: Vec<u8>,
        /// The address of the peer that sent the response.
        from_addr: SocketAddr,
    },
    /// Indicates that a `TcpConnect` operation was successful.
    TcpConnectionEstablished {
        /// The unique token for the new connection, to be used in subsequent operations.
        connection_token: Token,
        /// The address of the connected peer.
        peer_addr: SocketAddr,
    },
    /// Indicates that a `TcpSend` operation completed successfully.
    TcpDataSent {
        /// The number of bytes successfully written to the send buffer.
        bytes_sent: usize,
    },
    /// The data received from a `TcpReceive` operation.
    TcpDataReceived {
        /// The data buffer. An empty Vec indicates the connection was closed by the peer.
        data: Vec<u8>,
    },
    /// Indicates that a connection was successfully closed.
    ConnectionClosed,
    NewConnectionAccepted {
        /// The unique token for the new connection, to be used for send/receive.
        connection_token: Token,
        /// The address of the new client.
        peer_addr: SocketAddr,
        /// The token of the listener that accepted this connection.
        listener_token: Token,
    },
}


// --- Private Reactor Submodule ---
// The implementation details of the GlobalReactor are not part of the public API.
pub(crate) mod reactor;
pub(crate) mod poller;