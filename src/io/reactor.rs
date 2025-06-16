use crate::io::poller::Poller;
use crate::io::{IoOp, IoOutput, Token};
use crate::task::{TaskError, TaskId};
use omega::OmegaHashSet;
use omega::ohs::OmegaBucket;
use omega::omega_timer::{TimeoutManager, TimerConfig, ms_to_ticks};
use std::collections::VecDeque;
use std::io::{self};
use std::net::SocketAddr;
use std::net::TcpListener;
use std::net::UdpSocket;
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
enum ReactorAction {
    IoTimeout(Token),
}

#[derive(Debug)]
pub(crate) enum ReactorCommand {
    SubmitIoOp {
        op: IoOp,
        task_id: TaskId,
        result_tx: mpsc::Sender<Result<IoOutput, TaskError>>,
        timeout: Option<Duration>,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
pub(crate) enum IoState {
    TcpListening,
    TcpAccepting,
    UdpWaitingForResponse,
    TcpConnecting,
    TcpWriting,
    TcpReading,
    TcpIdle,
}

#[derive(Clone)]
pub(crate) struct IoOperationContext {
    pub task_id: TaskId,
    pub token: Token,
    pub fd: RawFd,
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
            fd: -1,
            state: IoState::TcpIdle,
            result_tx: tx,
            read_buffer: Vec::new(),
            write_buffer: VecDeque::new(),
            peer_address: None,
        }
    }
}

const EVENT_BUFFER_CAPACITY: usize = 1024;
const REACTOR_LOOP_TIMEOUT_MS: i32 = 10;

pub(crate) struct GlobalReactor {
    command_rx: mpsc::Receiver<ReactorCommand>,
    poller: Poller,
    shutdown_event_fd: RawFd,
    next_token: AtomicU64,

    connections: OmegaHashSet<u64, IoOperationContext>,
    timeout_manager: TimeoutManager<ReactorAction>,
}

fn sockaddr_in_to_socket_addr(addr: &libc::sockaddr_in) -> SocketAddr {
    let ip = std::net::Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));
    let port = u16::from_be(addr.sin_port);
    SocketAddr::new(std::net::IpAddr::V4(ip), port)
}
impl GlobalReactor {
    pub fn new(command_rx: mpsc::Receiver<ReactorCommand>) -> io::Result<Self> {
        let poller = Poller::new()?;
        let shutdown_event_fd = crate::io::poller::create_shutdown_eventfd()?;

        poller.add_fd_for_read(shutdown_event_fd, 0)?;

        let timer_config = TimerConfig::default();
        let timeout_manager = TimeoutManager::with_config(timer_config);

        Ok(Self {
            command_rx,
            poller,
            shutdown_event_fd,
            next_token: AtomicU64::new(1),
            connections: OmegaHashSet::new_u64_map(1024),
            timeout_manager,
        })
    }

    fn handle_io_timeout(&mut self, token: Token) {
        if let Some(context) = self.connections.remove(&token) {
            let _ = context.result_tx.send(Err(TaskError::TimedOut));
            let _ = self.poller.remove_fd(context.fd);
            unsafe {
                libc::close(context.fd);
            }
        }
    }

    pub fn run(mut self) {
        let mut events: [libc::epoll_event; EVENT_BUFFER_CAPACITY] =
            [libc::epoll_event { events: 0, u64: 0 }; EVENT_BUFFER_CAPACITY];

        'main_loop: loop {
            let ready_actions = self.timeout_manager.poll_ready();

            for action in ready_actions {
                match action {
                    ReactorAction::IoTimeout(token) => self.handle_io_timeout(token),
                }
            }

            let num_events = match self.poller.wait(&mut events, REACTOR_LOOP_TIMEOUT_MS) {
                Ok(num) => num,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    eprintln!("[Reactor] epoll_wait error: {}", e);
                    continue;
                }
            };

            for event in events.iter().take(num_events) {
                let token = event.u64;

                if token == 0 {
                    break 'main_loop;
                }
                self.handle_event(token, event.events);
            }

            while let Ok(command) = self.command_rx.try_recv() {
                match command {
                    ReactorCommand::SubmitIoOp { .. } => {
                        self.handle_command(command);
                    }
                    ReactorCommand::Shutdown => {
                        break 'main_loop;
                    }
                }
            }
        }
        self.shutdown();
    }

    fn handle_command(&mut self, command: ReactorCommand) {
        let ReactorCommand::SubmitIoOp {
            op,
            task_id,
            result_tx,
            timeout,
        } = command
        else {
            return;
        };

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
                std::mem::forget(socket);

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
                        libc::close(fd);
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
                        let actual_addr = listener.local_addr().unwrap(); // Get the actual bound address
                        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
                        std::mem::forget(listener);

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
                                libc::close(fd);
                            }
                            return;
                        }

                        // Send the success result immediately
                        let _ = context.result_tx.send(Ok(IoOutput::TcpListenerReady {
                            listener_token: token,
                            local_addr: actual_addr,
                        }));

                        self.connections.insert(token, context);
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

                unsafe {
                    libc::connect(
                        socket_fd,
                        &addr as *const _ as *const libc::sockaddr,
                        std::mem::size_of::<libc::sockaddr_in>() as u32,
                    );
                };

                let connect_err = io::Error::last_os_error();

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
                    context.result_tx = result_tx;
                    context.state = IoState::TcpWriting;
                    context.write_buffer.extend(data);
                    let _ = self.poller.rearm_for_write(context.fd, context.token);
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
                    context.result_tx = result_tx;
                    context.state = IoState::TcpReading;
                    let _ = self.poller.rearm_for_read(context.fd, context.token);
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
                    let _ = self.poller.remove_fd(context.fd);
                    unsafe {
                        libc::close(context.fd);
                    }
                    let _ = result_tx.send(Ok(IoOutput::ConnectionClosed));
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

    fn handle_event(&mut self, token: u64, _event_flags: u32) {
        let mut context_is_finished = false;
        let mut new_connection_to_add: Option<(Token, IoOperationContext)> = None;

        if let Some(context) = self.connections.get_mut(&token) {
            match context.state {
                IoState::TcpAccepting => {
                    // We only accept one connection per `TcpAccept` operation.
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
                        unsafe {
                            let flags = libc::fcntl(new_fd, libc::F_GETFL, 0);
                            libc::fcntl(new_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                        }

                        let peer_addr = sockaddr_in_to_socket_addr(&peer_addr_storage);
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

                        let _ = context.result_tx.send(Ok(IoOutput::NewConnectionAccepted {
                            connection_token: new_token,
                            peer_addr,
                            listener_token: token,
                        }));
                        // IMPORTANT: Set the listener's state back to idle listening.
                        context.state = IoState::TcpListening;
                    } else {
                        let err = io::Error::last_os_error();
                        if err.kind() != io::ErrorKind::WouldBlock {
                            // A real error occurred. Fail the TcpAccept task.
                            let _ = context
                                .result_tx
                                .send(Err(TaskError::ExecutionFailed(Box::new(err))));
                            context.state = IoState::TcpListening; // Revert state
                        } else {
                            // Spurious wakeup, re-arm and wait again.
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
                        let _ = context
                            .result_tx
                            .send(Ok(IoOutput::TcpConnectionEstablished {
                                connection_token: token,
                                peer_addr: context.peer_address.unwrap(),
                            }));

                        context.state = IoState::TcpIdle;
                    } else {
                        let err = io::Error::from_raw_os_error(error);
                        let _ = context
                            .result_tx
                            .send(Err(TaskError::ExecutionFailed(Box::new(err))));

                        context_is_finished = true;
                    }
                }

                IoState::TcpReading => {
                    let mut read_buf = vec![0; 2048];
                    match unsafe {
                        libc::read(context.fd, read_buf.as_mut_ptr() as *mut _, read_buf.len())
                    } {
                        -1 => {
                            let err = io::Error::last_os_error();
                            if err.kind() != io::ErrorKind::WouldBlock {
                                let _ = context
                                    .result_tx
                                    .send(Err(TaskError::ExecutionFailed(Box::new(err))));
                                context_is_finished = true;
                            } else {
                                let _ = self.poller.rearm_for_read(context.fd, context.token);
                            }
                        }
                        0 => {
                            let _ = context
                                .result_tx
                                .send(Ok(IoOutput::TcpDataReceived { data: vec![] }));
                            context_is_finished = true;
                        }
                        n => {
                            read_buf.truncate(n as usize);
                            let _ = context
                                .result_tx
                                .send(Ok(IoOutput::TcpDataReceived { data: read_buf }));
                            context.state = IoState::TcpIdle;
                        }
                    }
                }

                IoState::TcpWriting => {
                    let data_to_write = context.write_buffer.make_contiguous();
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
                                let _ = context
                                    .result_tx
                                    .send(Err(TaskError::ExecutionFailed(Box::new(err))));
                                context_is_finished = true;
                            } else {
                                let _ = self.poller.rearm_for_write(context.fd, context.token);
                            }
                        }
                        n if n > 0 => {
                            let bytes_written = n as usize;
                            let total_sent_previously =
                                data_to_write.len() - context.write_buffer.len() + bytes_written;
                            context.write_buffer.drain(..bytes_written);
                            if context.write_buffer.is_empty() {
                                context.state = IoState::TcpIdle;
                                let _ = context.result_tx.send(Ok(IoOutput::TcpDataSent {
                                    bytes_sent: total_sent_previously,
                                }));
                            } else {
                                let _ = self.poller.rearm_for_write(context.fd, context.token);
                            }
                        }
                        _ => {}
                    }
                }

                IoState::UdpWaitingForResponse => {
                    let mut read_buf = vec![0; 4096];
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
                                let _ = context
                                    .result_tx
                                    .send(Err(TaskError::ExecutionFailed(Box::new(err))));
                                context_is_finished = true;
                            }
                        }
                        n => {
                            read_buf.truncate(n as usize);

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

        if context_is_finished {
            if let Some(context) = self.connections.remove(&token) {
                let _ = self.poller.remove_fd(context.fd);
                unsafe {
                    libc::close(context.fd);
                }
            }
        }
    }

    fn shutdown(&mut self) {
        loop {
            let mut found_token = None;

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
                break;
            };

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
