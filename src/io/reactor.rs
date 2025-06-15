// src/io/reactor.rs

use crate::io::poller::{Poller, create_shutdown_eventfd};
use crate::io::{IoOp, IoOutput, Token};
use crate::task::{TaskError, TaskId};
use omega::OmegaHashSet;
use omega::ohs::OmegaBucket;

use std::collections::VecDeque;
use std::io::{self};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::net::{SocketAddr, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::os::unix::io::{ FromRawFd}; 
// --- Internal Reactor Structures (Definitions are the same) ---

#[derive(Debug)]
pub(crate) enum ReactorCommand {
    SubmitIoOp {
        op: IoOp,
        task_id: TaskId,
        result_tx: mpsc::Sender<Result<IoOutput, TaskError>>,
    },
    Shutdown,
}

/// The internal state machine for a single connection/I/O operation.
#[derive(Debug, Clone)]
pub(crate) enum IoState {
    TcpListening,
    UdpWaitingForResponse,
    TcpConnecting,
    TcpWriting,
    TcpReading,
    TcpIdle,
}

/// Holds all the state for a single ongoing I/O operation.
/// It must derive Default and Clone for OmegaHashSet compatibility.
#[derive(Clone)]
pub(crate) struct IoOperationContext {
    pub task_id: TaskId,
    pub token: Token,
    pub fd: RawFd, // CORRECTED: The context now owns its file descriptor
    pub state: IoState,
    pub result_tx: mpsc::Sender<Result<IoOutput, TaskError>>,
    pub read_buffer: Vec<u8>,
    pub write_buffer: VecDeque<u8>,
    pub peer_address: Option<SocketAddr>,
}

impl Default for IoOperationContext {
    fn default() -> Self {
        let (tx, _) = mpsc::channel();
        Self {
            task_id: TaskId::new(),
            token: 0,
            fd: -1, // Invalid FD
            state: IoState::TcpIdle,
            result_tx: tx,
            read_buffer: Vec::new(),
            write_buffer: VecDeque::new(),
            peer_address: None,
        }
    }
}

// --- GlobalReactor Implementation ---

const EVENT_BUFFER_CAPACITY: usize = 1024;
const REACTOR_LOOP_TIMEOUT_MS: i32 = 100;

pub(crate) struct GlobalReactor {
    command_rx: mpsc::Receiver<ReactorCommand>,
    poller: Poller,
    shutdown_event_fd: RawFd,
    next_token: AtomicU64,
    // The primary map is now Token -> Context.
    connections: OmegaHashSet<u64, IoOperationContext>,
}
/// Helper to convert libc::sockaddr_in to std::net::SocketAddr.
fn sockaddr_in_to_socket_addr(addr: &libc::sockaddr_in) -> SocketAddr {
    let ip = std::net::Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));
    let port = u16::from_be(addr.sin_port);
    SocketAddr::new(std::net::IpAddr::V4(ip), port)
}
impl GlobalReactor {
    /// Creates and initializes a new GlobalReactor instance.
    pub fn new(command_rx: mpsc::Receiver<ReactorCommand>) -> io::Result<Self> {
        let poller = Poller::new()?;
        let shutdown_event_fd = crate::io::poller::create_shutdown_eventfd()?;
        // Register the shutdown FD with epoll. We use token 0 for it.
        poller.add_fd_for_read(shutdown_event_fd, 0)?;

        Ok(Self {
            command_rx,
            poller,
            shutdown_event_fd,
            next_token: AtomicU64::new(1), // Token 0 is reserved for shutdown
            connections: OmegaHashSet::new_u64_map(1024),
        })
    }

    /// The main event loop of the reactor. This is intended to run in its own dedicated thread.
    pub fn run(mut self) {
        let mut events: [libc::epoll_event; EVENT_BUFFER_CAPACITY] =
            [libc::epoll_event { events: 0, u64: 0 }; EVENT_BUFFER_CAPACITY];

        'main_loop: loop {
            let num_events = match self.poller.wait(&mut events, REACTOR_LOOP_TIMEOUT_MS) {
                Ok(num) => num,
                Err(e) => {
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    // In a real-world scenario, you might want more robust error logging.
                    eprintln!("[Reactor] epoll_wait error: {}", e);
                    continue;
                }
            };

            for i in 0..num_events {
                let event = &events[i];
                let token = event.u64;

                if token == 0 {
                    // Token 0 is our shutdown signal
                    break 'main_loop;
                }

                self.handle_event(token, event.events);
            }

            // After processing OS events, process all pending commands from workers.
            while let Ok(command) = self.command_rx.try_recv() {
                match command {
                    ReactorCommand::SubmitIoOp {
                        op,
                        task_id,
                        result_tx,
                    } => {
                        self.handle_command(op, task_id, result_tx);
                    }
                    ReactorCommand::Shutdown => {
                        break 'main_loop;
                    }
                }
            }
        }
        // Gracefully shut down all connections.
        self.shutdown();
    }

    /// Handles a new command from an OmegaNode worker.
    fn handle_command(
        &mut self,
        op: IoOp,
        task_id: TaskId,
            result_tx: mpsc::Sender<Result<IoOutput, TaskError>>,
        ) {
            match op {
                IoOp::UdpSendAndListenOnce { peer_addr, data_to_send } => {
                    // 1. Create a non-blocking UDP socket.
                    let socket_fd = unsafe {
                        libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_NONBLOCK, 0)
                    };
    
                    if socket_fd < 0 {
                        let err = io::Error::last_os_error();
                        let _ = result_tx.send(Err(TaskError::ExecutionFailed(Box::new(err))));
                        return;
                    }
                    
                    // 2. Send the data to the specified peer.
                    let peer_sockaddr = socket_addr_to_sockaddr_in(&peer_addr);
                    let send_res = unsafe {
                        libc::sendto(
                            socket_fd,
                            data_to_send.as_ptr() as *const libc::c_void,
                            data_to_send.len(),
                            0, // flags
                            &peer_sockaddr as *const _ as *const libc::sockaddr,
                            std::mem::size_of::<libc::sockaddr_in>() as u32,
                        )
                    };
    
                    if send_res < 0 {
                        let err = io::Error::last_os_error();
                        let _ = result_tx.send(Err(TaskError::ExecutionFailed(Box::new(err))));
                        unsafe { libc::close(socket_fd); }
                        return;
                    }
    
                    // 3. Register for a read event to get the response.
                    let token = self.next_token.fetch_add(1, Ordering::Relaxed);
                    let context = IoOperationContext {
                        task_id,
                        token,
                        fd: socket_fd,
                        state: IoState::UdpWaitingForResponse,
                        result_tx,
                        read_buffer: Vec::new(),
                        write_buffer: VecDeque::new(),
                        peer_address: Some(peer_addr),
                    };
    
                    if let Err(e) = self.poller.add_fd_for_read(socket_fd, token) {
                        let _ = context.result_tx.send(Err(TaskError::ExecutionFailed(Box::new(e))));
                        unsafe { libc::close(socket_fd); }
                        return;
                    }
    
                    self.connections.insert(token, context);
                }
        
                IoOp::TcpListen { addr } => {
                    match TcpListener::bind(addr) {
                        Ok(listener) => {
                            listener
                                .set_nonblocking(true)
                                .expect("Failed to set listener to non-blocking");
                            let fd = listener.as_raw_fd();
                            let token = self.next_token.fetch_add(1, Ordering::Relaxed);
                            
                            // The Reactor now owns this FD. We must forget the listener
                            // so Rust doesn't close the FD when the listener goes out of scope.
                            std::mem::forget(listener);
    
                            let context = IoOperationContext {
                                task_id,
                                token,
                                fd,
                                state: IoState::TcpListening,
                                result_tx, // This sender will be used to report accepted connections.
                                read_buffer: Vec::new(),
                                write_buffer: VecDeque::new(),
                                peer_address: Some(addr),
                            };
    
                            // Register the listener for READ events, which signify incoming connections.
                            if let Err(e) = self.poller.add_fd_for_read(fd, token) {
                                let _ = context
                                    .result_tx
                                    .send(Err(TaskError::ExecutionFailed(Box::new(e))));
                                unsafe { libc::close(fd); }
                                return;
                            }
    
                            self.connections.insert(token, context);
                        }
                        Err(e) => {
                            let _ = result_tx.send(Err(TaskError::ExecutionFailed(Box::new(e))));
                        }
                    }
                }
                IoOp::TcpConnect { peer_addr } => {
                    let socket_fd = unsafe {
                        libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_NONBLOCK, 0)
                    };
    
                    if socket_fd < 0 {
                        let err = io::Error::last_os_error();
                        let _ = result_tx.send(Err(TaskError::ExecutionFailed(Box::new(err))));
                        return;
                    }
    
                    let addr = socket_addr_to_sockaddr_in(&peer_addr);
    
                    unsafe {
                        libc::connect(
                            socket_fd,
                            &addr as *const _ as *const libc::sockaddr,
                            std::mem::size_of::<libc::sockaddr_in>() as u32,
                        )
                    };
    
                    let connect_err = io::Error::last_os_error();
                    if connect_err.raw_os_error() != Some(libc::EINPROGRESS) {
                        let _ = result_tx.send(Err(TaskError::ExecutionFailed(Box::new(connect_err))));
                        unsafe { libc::close(socket_fd); }
                        return;
                    }
    
                    let token = self.next_token.fetch_add(1, Ordering::Relaxed);
                    let context = IoOperationContext {
                        task_id,
                        token,
                        fd: socket_fd,
                        state: IoState::TcpConnecting,
                        result_tx,
                        read_buffer: Vec::new(),
                        write_buffer: VecDeque::new(),
                        peer_address: Some(peer_addr),
                    };
    
                    if let Err(e) = self.poller.add_fd_for_write(socket_fd, token) {
                        let _ = context
                            .result_tx
                            .send(Err(TaskError::ExecutionFailed(Box::new(e))));
                        unsafe { libc::close(socket_fd); }
                        return;
                    }
    
                    self.connections.insert(token, context);
                }
                IoOp::TcpSend {
                    connection_token,
                    data,
                } => {
                    if let Some(context) = self.connections.get_mut(&connection_token) {
                        // This is the "write" half of the operation. Store the result channel
                        // for this specific send operation.
                        context.result_tx = result_tx;
                        context.state = IoState::TcpWriting;
                        context.write_buffer.extend(data);
                        
                        // Arm the poller for a write event. The actual `write` syscall
                        // will happen in `handle_event` when the socket is ready.
                        let _ = self.poller.rearm_for_write(context.fd, context.token);
                    } else {
                        // The connection token was not found.
                        let _ = result_tx.send(Err(TaskError::ExecutionFailed(Box::new(
                            io::Error::new(io::ErrorKind::NotFound, "Connection token not found for TcpSend"),
                        ))));
                    }
                }
                IoOp::TcpReceive {
                    connection_token,
                    .. // max_bytes is used in handle_event
                } => {
                    if let Some(context) = self.connections.get_mut(&connection_token) {
                        // This is the "read" half. Store the result channel for this receive.
                        context.result_tx = result_tx;
                        context.state = IoState::TcpReading;
                        
                        // Arm the poller for a read event. The actual `read` syscall
                        // will happen in `handle_event`.
                        let _ = self.poller.rearm_for_read(context.fd, context.token);
                    } else {
                        let _ = result_tx.send(Err(TaskError::ExecutionFailed(Box::new(
                            io::Error::new(io::ErrorKind::NotFound, "Connection token not found for TcpReceive"),
                        ))));
                    }
                }
                IoOp::CloseConnection { connection_token } => {
                    if let Some(context) = self.connections.remove(&connection_token) {
                        let _ = self.poller.remove_fd(context.fd);
                        unsafe { libc::close(context.fd); }
                        // Use the result_tx from this specific CloseConnection call
                        let _ = result_tx.send(Ok(IoOutput::ConnectionClosed));
                    } else {
                        // The connection was already closed or never existed.
                        let _ = result_tx.send(Err(TaskError::ExecutionFailed(Box::new(
                            io::Error::new(io::ErrorKind::NotFound, "Connection token not found for CloseConnection"),
                        ))));
                    }
                }
            }
        }
        
    // THIS IS THE CORRECTED `handle_event` METHOD

    /// Handles an I/O event from the OS for a specific connection token.
    fn handle_event(&mut self, token: u64, _event_flags: u32) {
        let mut context_is_finished = false;
        let mut new_connection_to_add: Option<(Token, IoOperationContext)> = None;

        if let Some(context) = self.connections.get_mut(&token) {
            match context.state {
                // Event for a listening TCP socket: accept the new connection.
                IoState::TcpListening => {
                    let listener = unsafe { TcpListener::from_raw_fd(context.fd) };
                    match listener.accept() {
                        Ok((stream, peer_addr)) => {
                            stream.set_nonblocking(true).unwrap();
                            let new_fd = stream.as_raw_fd();
                            std::mem::forget(stream); // Reactor now owns the FD.

                            let new_token = self.next_token.fetch_add(1, Ordering::Relaxed);
                            
                            let (new_tx, _) = mpsc::channel();
                            let new_context = IoOperationContext {
                                task_id: TaskId::new(),
                                token: new_token,
                                fd: new_fd,
                                state: IoState::TcpIdle,
                                result_tx: new_tx,
                                read_buffer: Vec::new(),
                                write_buffer: VecDeque::new(),
                                peer_address: Some(peer_addr),
                            };
                            
                            new_connection_to_add = Some((new_token, new_context));
                            
                            // Use the LISTENER's result channel to report the new connection
                            let _ = context.result_tx.send(Ok(IoOutput::NewConnectionAccepted {
                                connection_token: new_token,
                                peer_addr,
                                listener_token: token,
                            }));
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}, // Spurious wakeup, ignore.
                        Err(e) => eprintln!("[Reactor] Accept error on listener {}: {}", token, e),
                    }
                    
                    let _ = self.poller.rearm_for_read(context.fd, token);
                    std::mem::forget(listener);
                }
                // Event for a connecting TCP socket: check if the connection is established.
                IoState::TcpConnecting => {
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
                        // Connection successful!
                        let _ = context
                            .result_tx
                            .send(Ok(IoOutput::TcpConnectionEstablished {
                                connection_token: token,
                                peer_addr: context.peer_address.unwrap(),
                            }));
                        // CRITICAL FIX: The connection is now idle, NOT finished.
                        context.state = IoState::TcpIdle;
                    } else {
                        // Connection failed.
                        let err = io::Error::from_raw_os_error(error);
                        let _ = context
                            .result_tx
                            .send(Err(TaskError::ExecutionFailed(Box::new(err))));
                        // On failure, the operation IS finished.
                        context_is_finished = true;
                    }
                }
                // Event for a readable TCP socket.
                IoState::TcpReading => {
                    let mut read_buf = vec![0; 2048]; // Use a temporary buffer for the read.
                    match unsafe { libc::read(context.fd, read_buf.as_mut_ptr() as *mut _, read_buf.len()) } {
                        -1 => { // Error
                            let err = io::Error::last_os_error();
                            if err.kind() != io::ErrorKind::WouldBlock {
                                let _ = context.result_tx.send(Err(TaskError::ExecutionFailed(Box::new(err))));
                                context_is_finished = true;
                            } else {
                                let _ = self.poller.rearm_for_read(context.fd, context.token);
                            }
                        }
                        0 => { // Connection closed by peer
                            let _ = context.result_tx.send(Ok(IoOutput::TcpDataReceived { data: vec![] }));
                            context_is_finished = true;
                        }
                        n => { // Data received
                            read_buf.truncate(n as usize);
                            let _ = context.result_tx.send(Ok(IoOutput::TcpDataReceived { data: read_buf }));
                            context.state = IoState::TcpIdle;
                        }
                    }
                }
                // Event for a writable TCP socket.
                IoState::TcpWriting => {
                    let data_to_write = context.write_buffer.make_contiguous();
                    match unsafe { libc::write(context.fd, data_to_write.as_ptr() as *const _, data_to_write.len()) } {
                        -1 => { // Error
                            let err = io::Error::last_os_error();
                            if err.kind() != io::ErrorKind::WouldBlock {
                                let _ = context.result_tx.send(Err(TaskError::ExecutionFailed(Box::new(err))));
                                context_is_finished = true;
                            } else {
                                let _ = self.poller.rearm_for_write(context.fd, context.token);
                            }
                        }
                        n if n > 0 => { // Partially or fully wrote data
                            let bytes_written = n as usize;
                            let total_sent_previously = data_to_write.len() - context.write_buffer.len() + bytes_written;
                            context.write_buffer.drain(..bytes_written);
                            if context.write_buffer.is_empty() {
                                context.state = IoState::TcpIdle;
                                let _ = context.result_tx.send(Ok(IoOutput::TcpDataSent { bytes_sent: total_sent_previously }));
                            } else {
                                let _ = self.poller.rearm_for_write(context.fd, context.token);
                            }
                        }
                        _ => {} // Wrote 0 bytes, just wait for next event.
                    }
                }
                // Event for a readable UDP socket.
                IoState::UdpWaitingForResponse => {
                    let mut read_buf = vec![0; 4096];
                    let mut peer_addr_storage: libc::sockaddr_in = unsafe { std::mem::zeroed() };
                    let mut peer_addr_len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;

                    match unsafe { libc::recvfrom(
                        context.fd,
                        read_buf.as_mut_ptr() as *mut _,
                        read_buf.len(),
                        0,
                        &mut peer_addr_storage as *mut _ as *mut libc::sockaddr,
                        &mut peer_addr_len,
                    ) } {
                        -1 => {
                            let err = io::Error::last_os_error();
                            if err.kind() != io::ErrorKind::WouldBlock {
                                let _ = context.result_tx.send(Err(TaskError::ExecutionFailed(Box::new(err))));
                                context_is_finished = true;
                            }
                        }
                        n => {
                            read_buf.truncate(n as usize);
                            // THIS IS THE CORRECTED FUNCTION CALL
                            let from_addr = sockaddr_in_to_socket_addr(&peer_addr_storage);
                            let _ = context.result_tx.send(Ok(IoOutput::UdpResponse {
                                data: read_buf,
                                from_addr,
                            }));
                            context_is_finished = true;
                        }
                    }
                }
                IoState::TcpIdle => {
                    let mut buf = [0u8; 0];
                    if unsafe { libc::read(context.fd, buf.as_mut_ptr() as *mut _, 0) } == 0 {
                        context_is_finished = true;
                    }
                }
            }
        }

        if let Some((token_to_add, context_to_add)) = new_connection_to_add {
            if let Err(e) = self.poller.add_fd_for_read(context_to_add.fd, token_to_add) {
                 let _ = context_to_add.result_tx.send(Err(TaskError::ExecutionFailed(Box::new(e))));
                 unsafe { libc::close(context_to_add.fd); }
            } else {
                self.connections.insert(token_to_add, context_to_add);
            }
        }
        
        if context_is_finished {
            if let Some(context) = self.connections.remove(&token) {
                let _ = self.poller.remove_fd(context.fd);
                unsafe { libc::close(context.fd); }
            }
        }
    }
    /// Graceful shutdown logic: close all active connections and notify clients.
    fn shutdown(&mut self) {
        // OMEGA OPTIMIZATION: Manual drain - no iterator, no allocation
        loop {
            // Find first available key by checking buckets directly
            let mut found_token = None;

            // Fast scan for any available key
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

            // If no more keys, we're done
            let Some(token) = found_token else {
                break;
            };

            // Remove and process
            if let Some(context) = self.connections.remove(&token) {
                let _ = context
                    .result_tx
                    .send(Err(TaskError::ExecutionFailed(Box::new(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "Reactor is shutting down",
                    )))));
                let _ = self.poller.remove_fd(context.fd);
                unsafe {
                    libc::close(context.fd);
                }
            }
        }

        unsafe {
            libc::close(self.shutdown_event_fd);
        }
    }
}

/// Helper to convert std::net::SocketAddr to libc::sockaddr_in for OS calls.
fn socket_addr_to_sockaddr_in(addr: &SocketAddr) -> libc::sockaddr_in {
    match addr {
        SocketAddr::V4(a) => libc::sockaddr_in {
            sin_family: libc::AF_INET as u16,
            sin_port: a.port().to_be(),
            sin_addr: libc::in_addr {
                s_addr: u32::from_be(u32::from(*a.ip())),
            },
            sin_zero: [0; 8],
        },
        SocketAddr::V6(_) => panic!("IPv6 not yet supported for this reactor implementation"),
    }
}
