//! The `reactor` module implements the `GlobalReactor`, an asynchronous I/O event loop
//! responsible for handling network operations and managing their lifecycle.
//!
//! It uses `epoll` (via the `Poller` abstraction) to efficiently monitor file descriptors
//! for readiness events and processes I/O commands submitted by other parts of the system.

use crate::io::poller::Poller;
use crate::io::{IoOp, IoOutput, Token};
use crate::task::{TaskError, TaskId};
use omega::OmegaHashSet;
use omega::ohs::OmegaBucket;
use omega::omega_timer::{TimeoutManager, TimerConfig, ms_to_ticks};
use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr, TcpListener, UdpSocket};
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// Represents an action to be performed by the `GlobalReactor` based on a timeout.
#[derive(Debug, Clone, Copy)]
enum ReactorAction {
    /// Indicates that an I/O operation associated with the given `Token` has timed out.
    IoTimeout(Token),
}

/// Commands that can be sent to the `GlobalReactor` thread.
///
/// These commands allow other parts of the system to request I/O operations
/// or signal the reactor to shut down.
#[derive(Debug)]
pub(crate) enum ReactorCommand {
    /// Submits a new I/O operation to be handled by the reactor.
    SubmitIoOp {
        /// The I/O operation to perform.
        op: IoOp,
        /// The ID of the task that initiated this I/O operation.
        task_id: TaskId,
        /// A sender channel to return the result of the I/O operation.
        result_tx: mpsc::Sender<Result<IoOutput, TaskError>>,
        /// An optional timeout for the I/O operation.
        timeout: Option<Duration>,
    },
    /// Signals the `GlobalReactor` to shut down its event loop.
    Shutdown,
}

/// Represents the current state of an I/O operation or connection within the reactor.
#[derive(Debug, Clone)]
pub(crate) enum IoState {
    /// A TCP listener is active and waiting for `TcpAccept` commands.
    TcpListening,
    /// A TCP listener is currently processing a `TcpAccept` command and waiting for a new connection.
    TcpAccepting,
    /// A UDP socket has sent data and is waiting for a response.
    UdpWaitingForResponse,
    /// A TCP socket is in the process of establishing a connection.
    TcpConnecting,
    /// A TCP connection is currently writing data.
    TcpWriting,
    /// A TCP connection is currently reading data.
    TcpReading,
    /// A TCP connection is idle, not actively reading or writing, but still open.
    TcpIdle,
}

/// Contextual information for an active I/O operation or connection managed by the `GlobalReactor`.
///
/// This struct holds all necessary state for the reactor to manage a specific
/// network resource (e.g., a TCP connection, UDP socket, or TCP listener).
#[derive(Clone)]
pub(crate) struct IoOperationContext {
    /// The ID of the task associated with this I/O operation.
    pub task_id: TaskId,
    /// A unique token identifying this I/O context.
    pub token: Token,
    /// The raw file descriptor associated with this I/O context.
    pub fd: RawFd,
    /// The current state of the I/O operation.
    pub state: IoState,
    /// The sender channel to return the result of the I/O operation to the client.
    pub result_tx: mpsc::Sender<Result<IoOutput, TaskError>>,
    /// A buffer for incoming data (e.g., for TCP reads, UDP responses).
    pub read_buffer: Vec<u8>,
    /// A queue for outgoing data (e.g., for TCP writes).
    pub write_buffer: VecDeque<u8>,
    /// The remote peer's address, if applicable.
    pub peer_address: Option<SocketAddr>,
}

impl Default for IoOperationContext {
    /// Creates a default `IoOperationContext` with a dummy sender and uninitialized fields.
    fn default() -> Self {
        let (tx, _) = mpsc::channel(); // Dummy channel for default
        Self {
            task_id: TaskId::new(),
            token: 0,
            fd: -1,
            state: IoState::TcpIdle,
            result_tx: tx,
            read_buffer: Vec::new(),
            write_buffer: VecDeque::new(),
            peer_address: None,
        }
    }
}

/// The maximum number of events to retrieve from `epoll_wait` in a single call.
const EVENT_BUFFER_CAPACITY: usize = 1024;
/// The timeout for the `epoll_wait` call in milliseconds.
/// A small timeout ensures the reactor can regularly process commands and timeouts.
const REACTOR_LOOP_TIMEOUT_MS: i32 = 10;

/// The `GlobalReactor` is the central asynchronous I/O event loop.
///
/// It manages all network connections, listeners, and UDP sockets,
/// processing I/O events and commands. It uses `epoll` for efficient
/// event notification and a `TimeoutManager` for handling I/O timeouts.
pub(crate) struct GlobalReactor {
    /// Receiver channel for commands sent to the reactor.
    command_rx: mpsc::Receiver<ReactorCommand>,
    /// The `Poller` instance used for `epoll` operations.
    poller: Poller,
    /// A special `eventfd` used to signal the reactor to shut down.
    shutdown_event_fd: RawFd,
    /// Atomic counter for generating unique `Token`s for I/O contexts.
    next_token: AtomicU64,
    /// A hash set storing active `IoOperationContext`s, keyed by their `Token`.
    connections: OmegaHashSet<u64, IoOperationContext>,
    /// Manages scheduled timeouts for I/O operations.
    timeout_manager: TimeoutManager<ReactorAction>,
}

/// Converts a `libc::sockaddr_in` structure to a `std::net::SocketAddr`.
///
/// This is a helper function for converting C-style socket addresses
/// received from system calls into Rust's `SocketAddr` type.
fn sockaddr_in_to_socket_addr(addr: &libc::sockaddr_in) -> SocketAddr {
    let ip = std::net::Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));
    let port = u16::from_be(addr.sin_port);
    SocketAddr::new(std::net::IpAddr::V4(ip), port)
}

impl GlobalReactor {
    /// Creates a new `GlobalReactor` instance.
    ///
    /// Initializes the `Poller`, creates a shutdown `eventfd`, and sets up
    /// the `TimeoutManager`. The shutdown `eventfd` is added to the poller
    /// to allow external shutdown signals.
    ///
    /// # Arguments
    ///
    /// * `command_rx` - The receiver channel for `ReactorCommand`s.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `GlobalReactor` instance on success, or an `io::Error`.
    pub fn new(command_rx: mpsc::Receiver<ReactorCommand>) -> io::Result<Self> {
        let poller = Poller::new()?;
        let shutdown_event_fd = crate::io::poller::create_shutdown_eventfd()?;

        // Add the shutdown event FD to the poller, identified by token 0.
        poller.add_fd_for_read(shutdown_event_fd, 0)?;

        let timer_config = TimerConfig::default();
        let timeout_manager = TimeoutManager::with_config(timer_config);

        Ok(Self {
            command_rx,
            poller,
            shutdown_event_fd,
            next_token: AtomicU64::new(1), // Start tokens from 1, as 0 is reserved for shutdown_event_fd
            connections: OmegaHashSet::new_u64_map(1024), // Initial capacity for connections
            timeout_manager,
        })
    }

    /// Handles an `IoTimeout` action, cleaning up the timed-out connection.
    ///
    /// When an I/O operation times out, this method removes its context,
    /// sends a `TaskError::TimedOut` to the client, and closes the associated FD.
    ///
    /// # Arguments
    ///
    /// * `token` - The `Token` of the I/O operation that timed out.
    fn handle_io_timeout(&mut self, token: Token) {
        if let Some(context) = self.connections.remove(&token) {
            let _ = context.result_tx.send(Err(TaskError::TimedOut));
            let _ = self.poller.remove_fd(context.fd);
            unsafe {
                libc::close(context.fd);
            }
        }
    }

    /// The main event loop of the `GlobalReactor`.
    ///
    /// This function continuously:
    /// 1. Polls for ready timeouts from the `TimeoutManager`.
    /// 2. Waits for I/O events using `epoll_wait`.
    /// 3. Processes received I/O events.
    /// 4. Processes incoming `ReactorCommand`s.
    ///
    /// The loop continues until a `Shutdown` command is received or a critical
    /// `epoll_wait` error occurs.
    pub fn run(mut self) {
        let mut events: [libc::epoll_event; EVENT_BUFFER_CAPACITY] =
            [libc::epoll_event { events: 0, u64: 0 }; EVENT_BUFFER_CAPACITY];

        'main_loop: loop {
            // 1. Process any expired timeouts.
            let ready_actions = self.timeout_manager.poll_ready();
            for action in ready_actions {
                match action {
                    ReactorAction::IoTimeout(token) => self.handle_io_timeout(token),
                }
            }

            // 2. Wait for I/O events.
            let num_events = match self.poller.wait(&mut events, REACTOR_LOOP_TIMEOUT_MS) {
                Ok(num) => num,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue, // Retry on EINTR
                Err(_e) => {
                    continue; // Log error and continue loop
                }
            };

            // 3. Process received I/O events.
            for event in events.iter().take(num_events) {
                let token = event.u64;

                // Token 0 is reserved for the shutdown eventfd.
                if token == 0 {
                    break 'main_loop; // Received shutdown signal
                }
                self.handle_event(token, event.events);
            }

            // 4. Process incoming commands from other threads.
            while let Ok(command) = self.command_rx.try_recv() {
                match command {
                    ReactorCommand::SubmitIoOp { .. } => {
                        self.handle_command(command);
                    }
                    ReactorCommand::Shutdown => {
                        break 'main_loop; // Received shutdown command
                    }
                }
            }
        }
        // Perform final shutdown procedures.
        self.shutdown();
    }

    /// Handles an incoming `ReactorCommand`.
    ///
    /// This method dispatches the command to the appropriate handler based on its type.
    /// Currently, it only handles `SubmitIoOp` commands.
    ///
    /// # Arguments
    ///
    /// * `command` - The `ReactorCommand` to process.
    fn handle_command(&mut self, command: ReactorCommand) {
        let ReactorCommand::SubmitIoOp {
            op,
            task_id,
            result_tx,
            timeout,
        } = command
        else {
            return; // Only SubmitIoOp commands are handled here
        };

        // Helper closure to schedule a timeout for an I/O operation.
        let schedule_timeout = |timeout_manager: &mut TimeoutManager<ReactorAction>,
                                token: Token| {
            if let Some(timeout_duration) = timeout {
                let delay_ticks = ms_to_ticks(timeout_duration.as_millis() as u64, 10);
                timeout_manager.schedule(ReactorAction::IoTimeout(token), delay_ticks);
            }
        };

        match op {
            IoOp::UdpSendAndListenOnce {
                peer_addr,
                data_to_send,
            } => {
                let socket = match UdpSocket::bind("0.0.0.0:0") {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = result_tx.send(Err(TaskError::ExecutionFailed(Box::new(e))));
                        return;
                    }
                };
                socket
                    .set_nonblocking(true)
                    .expect("Failed to set UDP socket to non-blocking");

                if let Err(e) = socket.send_to(&data_to_send, peer_addr) {
                    let _ = result_tx.send(Err(TaskError::ExecutionFailed(Box::new(e))));
                    return;
                }

                let fd = socket.as_raw_fd();
                let token = self.next_token.fetch_add(1, Ordering::Relaxed);
                std::mem::forget(socket); // Prevent socket from being closed when it goes out of scope

                let context = IoOperationContext {
                    task_id,
                    token,
                    fd,
                    state: IoState::UdpWaitingForResponse,
                    result_tx,
                    read_buffer: Vec::new(),
                    write_buffer: VecDeque::new(),
                    peer_address: Some(peer_addr),
                };

                if let Err(e) = self.poller.add_fd_for_read(fd, token) {
                    let _ = context
                        .result_tx
                        .send(Err(TaskError::ExecutionFailed(Box::new(e))));
                    unsafe {
                        libc::close(fd); // Close FD if adding to poller fails
                    }
                    return;
                }

                self.connections.insert(token, context);
                schedule_timeout(&mut self.timeout_manager, token);
            }
            IoOp::TcpListen { addr } => {
                match TcpListener::bind(addr) {
                    Ok(listener) => {
                        listener
                            .set_nonblocking(true)
                            .expect("Failed to set listener to non-blocking");
                        let fd = listener.as_raw_fd();
                        let actual_addr = listener.local_addr().expect("Failed to get local_addr from newly bound listener"); // Get the actual bound address
                        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
                        std::mem::forget(listener); // Prevent listener from being closed

                        let context = IoOperationContext {
                            task_id,
                            token,
                            fd,
                            state: IoState::TcpListening, // Initial state: just listening
                            result_tx,
                            read_buffer: Vec::new(),
                            write_buffer: VecDeque::new(),
                            peer_address: Some(actual_addr),
                        };

                        if let Err(e) = self.poller.add_fd_for_read(fd, token) {
                            let _ = context
                                .result_tx
                                .send(Err(TaskError::ExecutionFailed(Box::new(e))));
                            unsafe {
                                libc::close(fd); // Close FD if adding to poller fails
                            }
                            return;
                        }

                        // Send the success result immediately, as the listener is ready.
                        let _ = context.result_tx.send(Ok(IoOutput::TcpListenerReady {
                            listener_token: token,
                            local_addr: actual_addr,
                        }));

                        self.connections.insert(token, context);
                        // No timeout scheduled for TcpListen itself, only for subsequent TcpAccept.
                    }
                    Err(e) => {
                        let _ = result_tx.send(Err(TaskError::ExecutionFailed(Box::new(e))));
                    }
                }
            }
            IoOp::TcpAccept { listener_token } => {
                // Find the existing listener's context.
                if let Some(listener_context) = self.connections.get_mut(&listener_token) {
                    // Update its state to show it's now actively waiting for an accept.
                    // Also, importantly, replace its result sender with the one for THIS task.
                    listener_context.state = IoState::TcpAccepting;
                    listener_context.result_tx = result_tx;
                    listener_context.task_id = task_id; // Associate this new task's ID

                    // Schedule a timeout for this specific accept operation if requested.
                    schedule_timeout(&mut self.timeout_manager, listener_token);

                    // Re-arm epoll just in case, to ensure we get notified of pending connections.
                    let _ = self
                        .poller
                        .rearm_for_read(listener_context.fd, listener_token);
                } else {
                    // Listener token not found, send an error back.
                    let _ =
                        result_tx.send(Err(TaskError::ExecutionFailed(Box::new(io::Error::new(
                            io::ErrorKind::NotFound,
                            "Listener token not found for TcpAccept",
                        )))));
                }
            }
            IoOp::TcpConnect { peer_addr } => {
                let socket_fd = unsafe {
                    libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_NONBLOCK, 0)
                };

                if socket_fd < 0 {
                    let _ = result_tx.send(Err(TaskError::ExecutionFailed(Box::new(
                        io::Error::last_os_error(),
                    ))));
                    return;
                }

                let addr = socket_addr_to_sockaddr_in(&peer_addr);

                // Attempt to connect. This will likely return EINPROGRESS for non-blocking sockets.
                unsafe {
                    libc::connect(
                        socket_fd,
                        &addr as *const _ as *const libc::sockaddr,
                        std::mem::size_of::<libc::sockaddr_in>() as u32,
                    );
                };

                let connect_err = io::Error::last_os_error();

                // If connect returns an error other than EINPROGRESS, it's a failure.
                if connect_err.raw_os_error() != Some(libc::EINPROGRESS) {
                    let _ = result_tx.send(Err(TaskError::ExecutionFailed(Box::new(connect_err))));
                    unsafe {
                        libc::close(socket_fd);
                    }
                    return;
                }

                let token = self.next_token.fetch_add(1, Ordering::Relaxed);
                let context = IoOperationContext {
                    task_id,
                    token,
                    fd: socket_fd,
                    state: IoState::TcpConnecting, // Set state to connecting
                    result_tx,
                    read_buffer: Vec::new(),
                    write_buffer: VecDeque::new(),
                    peer_address: Some(peer_addr),
                };

                // Add FD to poller for write events (to detect connect completion).
                if let Err(e) = self.poller.add_fd_for_write(socket_fd, token) {
                    let _ = context
                        .result_tx
                        .send(Err(TaskError::ExecutionFailed(Box::new(e))));
                    unsafe {
                        libc::close(socket_fd);
                    }
                    return;
                }

                self.connections.insert(token, context);
                schedule_timeout(&mut self.timeout_manager, token);
            }
            IoOp::TcpSend {
                connection_token,
                data,
            } => {
                if let Some(context) = self.connections.get_mut(&connection_token) {
                    context.result_tx = result_tx; // Update result sender for this specific send op
                    context.state = IoState::TcpWriting; // Set state to writing
                    context.write_buffer.extend(data); // Add data to write buffer
                    let _ = self.poller.rearm_for_write(context.fd, context.token); // Re-arm for write events
                    schedule_timeout(&mut self.timeout_manager, connection_token);
                } else {
                    let _ =
                        result_tx.send(Err(TaskError::ExecutionFailed(Box::new(io::Error::new(
                            io::ErrorKind::NotFound,
                            "Connection token not found for TcpSend",
                        )))));
                }
            }
            IoOp::TcpReceive {
                connection_token, ..
            } => {
                if let Some(context) = self.connections.get_mut(&connection_token) {
                    context.result_tx = result_tx; // Update result sender for this specific receive op
                    context.state = IoState::TcpReading; // Set state to reading
                    let _ = self.poller.rearm_for_read(context.fd, context.token); // Re-arm for read events
                    schedule_timeout(&mut self.timeout_manager, connection_token);
                } else {
                    let _ =
                        result_tx.send(Err(TaskError::ExecutionFailed(Box::new(io::Error::new(
                            io::ErrorKind::NotFound,
                            "Connection token not found for TcpReceive",
                        )))));
                }
            }
            IoOp::CloseConnection { connection_token } => {
                if let Some(context) = self.connections.remove(&connection_token) {
                    let _ = self.poller.remove_fd(context.fd); // Remove from poller
                    unsafe {
                        libc::close(context.fd); // Close the raw file descriptor
                    }
                    let _ = result_tx.send(Ok(IoOutput::ConnectionClosed)); // Confirm closure
                } else {
                    let _ =
                        result_tx.send(Err(TaskError::ExecutionFailed(Box::new(io::Error::new(
                            io::ErrorKind::NotFound,
                            "Connection token not found for CloseConnection",
                        )))));
                }
            }
        }
    }

    /// Handles a single I/O event received from the `Poller`.
    ///
    /// This method dispatches the event to the appropriate handler based on the
    /// `IoState` of the associated `IoOperationContext`.
    ///
    /// # Arguments
    ///
    /// * `token` - The `Token` of the `IoOperationContext` that triggered the event.
    /// * `_event_flags` - The raw `epoll` event flags (currently unused, but available for future use).
    fn handle_event(&mut self, token: u64, _event_flags: u32) {
        let mut context_is_finished = false;
        let mut new_connection_to_add: Option<(Token, IoOperationContext)> = None;

        if let Some(context) = self.connections.get_mut(&token) {
            match context.state {
                IoState::TcpAccepting => {
                    // Attempt to accept a new connection.
                    let mut peer_addr_storage: libc::sockaddr_in = unsafe { std::mem::zeroed() };
                    let mut peer_addr_len =
                        std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;

                    let new_fd = unsafe {
                        libc::accept(
                            context.fd,
                            &mut peer_addr_storage as *mut _ as *mut libc::sockaddr,
                            &mut peer_addr_len,
                        )
                    };

                    if new_fd >= 0 {
                        // Set the new accepted socket to non-blocking.
                        unsafe {
                            let flags = libc::fcntl(new_fd, libc::F_GETFL, 0);
                            libc::fcntl(new_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                        }

                        let peer_addr = sockaddr_in_to_socket_addr(&peer_addr_storage);
                        let new_token = self.next_token.fetch_add(1, Ordering::Relaxed);

                        // Create a new context for the accepted connection.
                        let (new_tx, _) = mpsc::channel(); // Dummy channel for new context initially
                        let new_context = IoOperationContext {
                            task_id: TaskId::new(), // New task ID for the accepted connection
                            token: new_token,
                            fd: new_fd,
                            state: IoState::TcpIdle, // New connection starts in idle state
                            result_tx: new_tx,
                            read_buffer: Vec::new(),
                            write_buffer: VecDeque::new(),
                            peer_address: Some(peer_addr),
                        };

                        new_connection_to_add = Some((new_token, new_context));

                        // Send the result of the TcpAccept operation.
                        let _ = context.result_tx.send(Ok(IoOutput::NewConnectionAccepted {
                            connection_token: new_token,
                            peer_addr,
                            listener_token: token,
                        }));
                        // IMPORTANT: Set the listener's state back to idle listening,
                        // so it can accept more connections if requested.
                        context.state = IoState::TcpListening;
                    } else {
                        let err = io::Error::last_os_error();
                        if err.kind() != io::ErrorKind::WouldBlock {
                            // A real error occurred during accept. Fail the TcpAccept task.
                            let _ = context
                                .result_tx
                                .send(Err(TaskError::ExecutionFailed(Box::new(err))));
                            context.state = IoState::TcpListening; // Revert listener state
                        } else {
                            // Spurious wakeup or no connection yet, re-arm and wait again.
                            let _ = self.poller.rearm_for_read(context.fd, token);
                        }
                    }
                }

                IoState::TcpListening => {
                    // An event on a listener that is NOT in an accepting state is
                    // noted, but no action is taken until a `TcpAccept` op is submitted.
                    // We just re-arm to ensure we don't miss the event later.
                    let _ = self.poller.rearm_for_read(context.fd, token);
                }
                IoState::TcpConnecting => {
                    // Check for connection completion status.
                    let mut error: libc::c_int = 0;
                    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;

                    unsafe {
                        libc::getsockopt(
                            context.fd,
                            libc::SOL_SOCKET,
                            libc::SO_ERROR,
                            &mut error as *mut _ as *mut libc::c_void,
                            &mut len,
                        );
                    };

                    if error == 0 {
                        // Connection successful.
                        let _ = context
                            .result_tx
                            .send(Ok(IoOutput::TcpConnectionEstablished {
                                connection_token: token,
                                peer_addr: context.peer_address.expect("Connecting context must have a peer_address"),
                            }));

                        context.state = IoState::TcpIdle; // Connection is now idle
                    } else {
                        // Connection failed.
                        let err = io::Error::from_raw_os_error(error);
                        let _ = context
                            .result_tx
                            .send(Err(TaskError::ExecutionFailed(Box::new(err))));

                        context_is_finished = true; // Mark context for removal
                    }
                }

                IoState::TcpReading => {
                    let mut read_buf = vec![0; 2048]; // Buffer for reading data
                    match unsafe {
                        libc::read(context.fd, read_buf.as_mut_ptr() as *mut _, read_buf.len())
                    } {
                        -1 => {
                            let err = io::Error::last_os_error();
                            if err.kind() != io::ErrorKind::WouldBlock {
                                // A real error occurred during read.
                                let _ = context
                                    .result_tx
                                    .send(Err(TaskError::ExecutionFailed(Box::new(err))));
                                context_is_finished = true; // Mark context for removal
                            } else {
                                // No data available yet, re-arm for read.
                                let _ = self.poller.rearm_for_read(context.fd, context.token);
                            }
                        }
                        0 => {
                            // Peer closed the connection gracefully.
                            let _ = context
                                .result_tx
                                .send(Ok(IoOutput::TcpDataReceived { data: vec![] }));
                            context_is_finished = true; // Mark context for removal
                        }
                        n => {
                            // Data received.
                            read_buf.truncate(n as usize);
                            let _ = context
                                .result_tx
                                .send(Ok(IoOutput::TcpDataReceived { data: read_buf }));
                            context.state = IoState::TcpIdle; // Return to idle state
                        }
                    }
                }

                IoState::TcpWriting => {
                    let data_to_write = context.write_buffer.make_contiguous(); // Get contiguous slice
                    match unsafe {
                        libc::write(
                            context.fd,
                            data_to_write.as_ptr() as *const _,
                            data_to_write.len(),
                        )
                    } {
                        -1 => {
                            let err = io::Error::last_os_error();
                            if err.kind() != io::ErrorKind::WouldBlock {
                                // A real error occurred during write.
                                let _ = context
                                    .result_tx
                                    .send(Err(TaskError::ExecutionFailed(Box::new(err))));
                                context_is_finished = true; // Mark context for removal
                            } else {
                                // Cannot write all data yet, re-arm for write.
                                let _ = self.poller.rearm_for_write(context.fd, context.token);
                            }
                        }
                        n if n > 0 => {
                            // Some data was written.
                            let bytes_written = n as usize;
                            // Calculate total bytes sent for the current operation.
                            let total_sent_previously =
                                data_to_write.len() - context.write_buffer.len() + bytes_written;
                            context.write_buffer.drain(..bytes_written); // Remove written bytes from buffer
                            if context.write_buffer.is_empty() {
                                // All data sent.
                                context.state = IoState::TcpIdle; // Return to idle state
                                let _ = context.result_tx.send(Ok(IoOutput::TcpDataSent {
                                    bytes_sent: total_sent_previously,
                                }));
                            } else {
                                // More data to write, re-arm for write.
                                let _ = self.poller.rearm_for_write(context.fd, context.token);
                            }
                        }
                        _ => { /* 0 bytes written, or other unexpected case. Do nothing for now. */
                        }
                    }
                }

                IoState::UdpWaitingForResponse => {
                    let mut read_buf = vec![0; 4096]; // Buffer for UDP response
                    let mut peer_addr_storage: libc::sockaddr_in = unsafe { std::mem::zeroed() };
                    let mut peer_addr_len =
                        std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;

                    match unsafe {
                        libc::recvfrom(
                            context.fd,
                            read_buf.as_mut_ptr() as *mut _,
                            read_buf.len(),
                            0,
                            &mut peer_addr_storage as *mut _ as *mut libc::sockaddr,
                            &mut peer_addr_len,
                        )
                    } {
                        -1 => {
                            let err = io::Error::last_os_error();
                            if err.kind() != io::ErrorKind::WouldBlock {
                                // A real error occurred during recvfrom.
                                let _ = context
                                    .result_tx
                                    .send(Err(TaskError::ExecutionFailed(Box::new(err))));
                                context_is_finished = true; // Mark context for removal
                            }
                            // If WouldBlock, just wait for next event.
                        }
                        n => {
                            // Data received.
                            read_buf.truncate(n as usize);

                            let from_addr = sockaddr_in_to_socket_addr(&peer_addr_storage);
                            let _ = context.result_tx.send(Ok(IoOutput::UdpResponse {
                                data: read_buf,
                                from_addr,
                            }));
                            context_is_finished = true; // Mark context for removal
                        }
                    }
                }
                IoState::TcpIdle => {
                    // If an event occurs on an idle TCP connection, it might indicate
                    // a peer disconnect (read returns 0 bytes).
                    let mut buf = [0u8; 0]; // Use a zero-sized buffer for a non-blocking read check
                    if unsafe { libc::read(context.fd, buf.as_mut_ptr() as *mut _, 0) } == 0 {
                        // Read of 0 bytes indicates EOF/peer closed connection.
                        context_is_finished = true;
                    }
                    // Otherwise, it's a spurious wakeup or an event we don't care about in idle state.
                }
            }
        }

        // If a new connection was accepted, add its context to the reactor.
        if let Some((token_to_add, context_to_add)) = new_connection_to_add {
            if let Err(e) = self.poller.add_fd_for_read(context_to_add.fd, token_to_add) {
                let _ = context_to_add
                    .result_tx
                    .send(Err(TaskError::ExecutionFailed(Box::new(e))));
                unsafe {
                    libc::close(context_to_add.fd);
                }
            } else {
                self.connections.insert(token_to_add, context_to_add);
            }
        }

        // If the current context is finished (e.g., connection closed, error, timeout), remove it.
        if context_is_finished {
            if let Some(context) = self.connections.remove(&token) {
                let _ = self.poller.remove_fd(context.fd);
                unsafe {
                    libc::close(context.fd);
                }
            }
        }
    }

    /// Shuts down the `GlobalReactor` and all managed I/O resources.
    ///
    /// This method iterates through all active connections, sends an error
    /// to their respective clients, removes them from the poller, and closes
    /// their file descriptors. Finally, it closes the shutdown `eventfd`.
    fn shutdown(&mut self) {
        // Drain all remaining connections.
        loop {
            let mut found_token = None;

            // Iterate through the internal storage of OmegaHashSet to find any remaining entries.
            for bucket in &self.connections.storage {
                match bucket {
                    OmegaBucket::Empty => continue,
                    OmegaBucket::Inline { entries, len } => {
                        if *len > 0 {
                            found_token = Some(unsafe { entries.get_unchecked(0).0 });
                            break;
                        }
                    }
                    OmegaBucket::Overflow { entries } => {
                        if !entries.is_empty() {
                            found_token = Some(entries[0].0);
                            break;
                        }
                    }
                }
            }

            let Some(token) = found_token else {
                break; // No more connections to shut down.
            };

            if let Some(context) = self.connections.remove(&token) {
                // Notify the client that the reactor is shutting down.
                let _ = context
                    .result_tx
                    .send(Err(TaskError::ExecutionFailed(Box::new(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "Reactor is shutting down",
                    )))));
                let _ = self.poller.remove_fd(context.fd); // Remove from poller
                unsafe {
                    libc::close(context.fd); // Close the raw file descriptor
                }
            }
        }

        // Close the shutdown eventfd.
        unsafe {
            libc::close(self.shutdown_event_fd);
        }
    }
}

/// Converts a `std::net::SocketAddr` to a `libc::sockaddr_in` structure.
///
/// This is a helper function for converting Rust's `SocketAddr` type
/// into a C-style `sockaddr_in` for use with `libc` functions.
///
/// # Panics
///
/// Panics if an IPv6 address is provided, as this implementation currently
/// only supports IPv4.
fn socket_addr_to_sockaddr_in(addr: &SocketAddr) -> libc::sockaddr_in {
    match addr {
        SocketAddr::V4(a) => libc::sockaddr_in {
            sin_family: libc::AF_INET as u16,
            sin_port: a.port().to_be(), // Convert to network byte order
            sin_addr: libc::in_addr {
                s_addr: u32::from_be(u32::from(*a.ip())), // Convert to network byte order
            },
            sin_zero: [0; 8], // Padding
        },
        SocketAddr::V6(_) => panic!("IPv6 not yet supported for this reactor implementation"),
    }
}
