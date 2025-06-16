// src/io/mod.rs

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
   
   // --- TCP Operations ---

    /// Binds a TCP listener to an address and prepares it for accepting connections.
    /// This is a one-shot setup operation.
    TcpListen {
        addr: SocketAddr,
    },

    /// Waits for the next incoming connection on a specific listener.
    /// This is a one-shot "wait" operation.
    TcpAccept {
        listener_token: Token,
    },

    /// Initiates a non-blocking TCP connection to a peer.
    TcpConnect {
        peer_addr: SocketAddr,
    },

    /// Sends data over an existing, established TCP connection.
    TcpSend {
        connection_token: Token,
        data: Vec<u8>,
    },

    /// Issues a request to receive data from an established TCP connection.
    TcpReceive {
        connection_token: Token,
        max_bytes: usize,
    },

    /// Closes an established TCP connection or an active listener.
    CloseConnection {
        connection_token: Token,
    },

}

/// Defines the successful outcomes of an `IoOp`.
/// This is what the client receives in the `Ok()` variant of a `TaskHandle` result.
#[derive(Debug)]
pub enum IoOutput {
    // --- UDP ---
    UdpResponse {
        data: Vec<u8>,
        from_addr: SocketAddr,
    },
    
    // --- TCP ---

    /// Returned on a successful `IoOp::TcpListen`.
    TcpListenerReady {
        listener_token: Token,
        local_addr: SocketAddr,
    },
    
    /// Returned on a successful `IoOp::TcpAccept`.
    NewConnectionAccepted {
        /// The token for the newly accepted client connection.
        connection_token: Token,
        peer_addr: SocketAddr,
        /// The token of the listener that accepted this connection.
        listener_token: Token,
    },
    
    /// Returned on a successful `IoOp::TcpConnect`.
    TcpConnectionEstablished {
        connection_token: Token,
        peer_addr: SocketAddr,
    },
    
    /// Returned on a successful `IoOp::TcpSend`.
    TcpDataSent {
        bytes_sent: usize,
    },
    
    /// Data received from a `IoOp::TcpReceive`. An empty Vec indicates graceful shutdown by the peer.
    TcpDataReceived {
        data: Vec<u8>,
    },
    
    /// Confirmation that a connection or listener was closed.
    ConnectionClosed,
}


// --- Private Reactor Submodule ---
// The implementation details of the GlobalReactor are not part of the public API.
pub(crate) mod reactor;
pub(crate) mod poller;