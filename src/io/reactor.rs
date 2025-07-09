use crate::io::poller::Poller;
use crate::io::{IoOp, IoOutput, Token};
use crate::task::{TaskError, TaskId};
use omega::OmegaHashSet;
use omega::ohs::OmegaBucket;
use omega::omega_timer::{TimeoutManager, TimerConfig, ms_to_ticks};
use ovp::{DroneId, OmegaSocket, parse_ovp_frame_fast};
use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr};
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum IoState {
    OvpListening,
    OvpReceiving,
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
    pub ovp_socket: Option<OmegaSocket>,
    pub my_drone_id: Option<DroneId>,
}

impl Default for IoOperationContext {
    fn default() -> Self {
        let (tx, _) = mpsc::channel();
        Self {
            task_id: TaskId::new(),
            token: 0,
            fd: -1,
            state: IoState::OvpListening,
            result_tx: tx,
            read_buffer: Vec::new(),
            write_buffer: VecDeque::new(),
            peer_address: None,
            ovp_socket: None,
            my_drone_id: None,
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
            connections: OmegaHashSet::new(1024),
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
                Err(_e) => {
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
            IoOp::OvpInit {
                interface,
                my_drone_id,
            } => match OmegaSocket::new(&interface) {
                Ok(socket) => {
                    let fd = socket.raw_fd;
                    let token = self.next_token.fetch_add(1, Ordering::Relaxed);

                    let context = IoOperationContext {
                        task_id,
                        token,
                        fd,
                        state: IoState::OvpListening,
                        result_tx,
                        ovp_socket: Some(socket),
                        my_drone_id: Some(my_drone_id),

                        read_buffer: Vec::new(),
                        write_buffer: VecDeque::new(),
                        peer_address: None,
                    };

                    if let Err(e) = self.poller.add_fd_for_read(fd, token) {
                        let _ = context
                            .result_tx
                            .send(Err(TaskError::ExecutionFailed(Box::new(e))));

                        return;
                    }

                    let _ = context.result_tx.send(Ok(IoOutput::OvpSocketReady {
                        socket_token: token,
                    }));
                    self.connections.insert(token, context);
                }
                Err(e) => {
                    let safe_error = io::Error::new(io::ErrorKind::Other, e.to_string());
                    let _ = result_tx.send(Err(TaskError::ExecutionFailed(Box::new(safe_error))));
                }
            },
            IoOp::OvpEmit {
                socket_token,
                targets,
                payload,
            } => {
                if let Some(context) = self.connections.get_mut(&socket_token) {
                    if let Some(socket) = context.ovp_socket.as_mut() {
                        match socket.build_and_emit(targets.as_deref().unwrap_or(&[]), &payload) {
                            Ok(_) => {
                                let _ = result_tx.send(Ok(IoOutput::OvpEmitSuccess));
                            }
                            Err(e) => {
                                let safe_error =
                                    io::Error::new(io::ErrorKind::Other, e.to_string());
                                let _ = result_tx
                                    .send(Err(TaskError::ExecutionFailed(Box::new(safe_error))));
                            }
                        }
                    } else {
                        let _ = result_tx.send(Err(TaskError::ExecutionFailed(
                            "Token does not correspond to an OVP socket".into(),
                        )));
                    }
                } else {
                    let _ = result_tx.send(Err(TaskError::ExecutionFailed(
                        "OVP socket token not found".into(),
                    )));
                }
            }
            IoOp::OvpReceive { socket_token } => {
                if let Some(context) = self.connections.get_mut(&socket_token) {
                    context.state = IoState::OvpReceiving;
                    context.result_tx = result_tx;
                    context.task_id = task_id;

                    schedule_timeout(&mut self.timeout_manager, socket_token);

                    let _ = self.poller.rearm_for_read(context.fd, context.token);
                } else {
                    let _ = result_tx.send(Err(TaskError::ExecutionFailed(
                        "OVP socket token not found for Receive".into(),
                    )));
                }
            }
        }
    }

    fn handle_event(&mut self, token: u64, _event_flags: u32) {
        let context_is_finished = false;
        let new_connection_to_add: Option<(Token, IoOperationContext)> = None;

        if let Some(context) = self.connections.get_mut(&token) {
            match context.state {
                IoState::OvpReceiving => {
                    if let (Some(socket), Some(my_id)) =
                        (context.ovp_socket.as_mut(), context.my_drone_id)
                    {
                        // We only try to receive once per OvpReceive command, enforcing the one-shot model.
                        // The loop is for draining the socket after we find a match, not for finding multiple matches.
                        match socket.receive_frame() {
                            Ok(frame) => {
                                if let Some(payload) = parse_ovp_frame_fast(frame, my_id) {
                                    // We found a valid frame for us. Send it and we are done.
                                    let _ =
                                        context.result_tx.send(Ok(IoOutput::OvpFrameReceived {
                                            payload: payload.to_vec(),
                                        }));
                                    // Set state back to idle listening. The task is complete.
                                    context.state = IoState::OvpListening;
                                } else {
                                    // The frame was not for us. Re-arm the poller and wait for the next one.
                                    let _ = self.poller.rearm_for_read(context.fd, token);
                                }
                            }
                            Err(_) => {
                                // An error (like EAGAIN) means no more packets right now.
                                // Re-arm and wait for the next event.
                                let _ = self.poller.rearm_for_read(context.fd, token);
                            }
                        }
                    }
                }
                IoState::OvpListening => {
                    let _ = self.poller.rearm_for_read(context.fd, token);
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
