//! The vibe interface - for coders who just want things to work
//! No channels, no Results, no complexity. Just run stuff and get stuff back.
//! RESPECTS THE MONA LISA - no manual thread management!

use crate::ultra_omega::UltraOmegaSystem;
use crate::task::{Priority, TaskError, TaskHandle, TaskId};
use crate::io::{IoOp, IoOutput};
use crate::types::NodeError;
use omega::borrg::OmegaRng;
use omega::omega_timer::elapsed_ns;
use ovp::DroneId;
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// Thread-local RNG to avoid mutex contention (only performance optimization)
thread_local! {
    static LOCAL_RNG: std::cell::RefCell<OmegaRng> = std::cell::RefCell::new(OmegaRng::new(elapsed_ns()));
}

/// A job running in the background. Call `.get()` when you want the result.
pub struct Job<T> {
    handle: TaskHandle<T>,
}

impl<T> Job<T> {
    /// Get your result. Blocks until done. 
    /// If it breaks, your program crashes with a helpful message.
    pub fn get(self) -> T {
        match self.handle.recv_result() {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => panic!("❌ Your job failed! Check your function for bugs."),
            Err(_) => panic!("❌ Job was cancelled - did you shut down the system?"),
        }
    }
    
    /// Check if your job is done without waiting.
    /// Returns Some(result) if ready, None if still working.
    pub fn peek(self) -> Option<T> {
        match self.handle.try_recv_result() {
            Ok(Ok(result)) => Some(result),
            Ok(Err(_)) => panic!("❌ Your job failed! Check your function for bugs."),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => panic!("❌ Job was cancelled"),
        }
    }
    
    /// Is the job finished? (doesn't consume the job)
    pub fn is_done(&self) -> bool {
        matches!(self.handle.try_recv_result(), Ok(_) | Err(mpsc::TryRecvError::Disconnected))
    }
}

/// A simple OVP socket. Send and receive without thinking about it.
pub struct OvpSocket {
    token: u64,
    system: Arc<UltraOmegaSystem>,
}

impl OvpSocket {
    /// Send a message to specific drones. Fire and forget.
    /// 
    /// Example: `socket.send_to(vec![drone1, drone2], b"hello")`
    pub fn send_to(&self, targets: Vec<DroneId>, data: &[u8]) {
        LOCAL_RNG.with(|rng| {
            let io_op = IoOp::OvpEmit {
                socket_token: self.token,
                targets: Some(targets),
                payload: data.to_vec(),
            };
            
            // Trust the Mona Lisa! Just submit and drop the handle.
            // The system will manage all threads internally.
            match self.system.submit_io_task(io_op, Some(Duration::from_secs(5)), &mut *rng.borrow_mut()) {
                Ok(_handle) => {
                    // Fire and forget - just drop the handle!
                    // No manual threads, let the system handle everything.
                }
                Err(_) => panic!("💥 Failed to send OVP message - system overloaded!"),
            }
        })
    }
    
    /// Broadcast to everyone on the network. Fire and forget.
    /// 
    /// Example: `socket.broadcast(b"hello everyone")`
    pub fn broadcast(&self, data: &[u8]) {
        LOCAL_RNG.with(|rng| {
            let io_op = IoOp::OvpEmit {
                socket_token: self.token,
                targets: None, // None means broadcast
                payload: data.to_vec(),
            };
            
            // Trust the Mona Lisa! No manual thread management.
            match self.system.submit_io_task(io_op, Some(Duration::from_secs(5)), &mut *rng.borrow_mut()) {
                Ok(_handle) => {
                    // Fire and forget - system handles everything internally
                }
                Err(_) => panic!("💥 Failed to broadcast OVP message - system overloaded!"),
            }
        })
    }
    
    /// Get the next message. Blocks until one arrives.
    /// 
    /// Example: `let data = socket.next_message();`
    pub fn next_message(&self) -> Vec<u8> {
        LOCAL_RNG.with(|rng| {
            let io_op = IoOp::OvpReceive {
                socket_token: self.token,
            };
            
            match self.system.submit_io_task(io_op, Some(Duration::from_secs(30)), &mut *rng.borrow_mut()) {
                Ok(handle) => {
                    match handle.recv_result() {
                        Ok(Ok(IoOutput::OvpFrameReceived { payload })) => payload,
                        Ok(Ok(_)) => panic!("🤔 Got weird response from OVP socket"),
                        Ok(Err(_)) => panic!("📡 Network error while receiving OVP message"),
                        Err(_) => panic!("📡 OVP receive was cancelled"),
                    }
                }
                Err(_) => panic!("💥 Failed to listen for OVP messages - system overloaded!"),
            }
        })
    }
    
    /// Try to get a message without blocking. Returns None if no message waiting.
    /// 
    /// Example: `if let Some(data) = socket.try_message() { ... }`
    pub fn try_message(&self) -> Option<Vec<u8>> {
        LOCAL_RNG.with(|rng| {
            let io_op = IoOp::OvpReceive {
                socket_token: self.token,
            };
            
            match self.system.submit_io_task(io_op, Some(Duration::from_millis(1)), &mut *rng.borrow_mut()) {
                Ok(handle) => {
                    match handle.try_recv_result() {
                        Ok(Ok(IoOutput::OvpFrameReceived { payload })) => Some(payload),
                        Ok(Ok(_)) => panic!("🤔 Got weird response from OVP socket"),
                        Ok(Err(TaskError::TimedOut)) => None, // No message available
                        Ok(Err(_)) => panic!("📡 Network error while checking for OVP message"),
                        Err(mpsc::TryRecvError::Empty) => None, // Still waiting
                        Err(mpsc::TryRecvError::Disconnected) => panic!("📡 OVP socket was closed"),
                    }
                }
                Err(_) => panic!("💥 Failed to check for OVP messages - system overloaded!"),
            }
        })
    }
}

/// The dead simple parallel system. Create one, run stuff, get results back.
pub struct VibeSystem {
    inner: Arc<UltraOmegaSystem>,
}

impl VibeSystem {
    /// Create a new system. Just works with good defaults.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(UltraOmegaSystem::builder().with_nodes(80).with_super_nodes(40).build()),
        }
    }
    
    /// Run a function with data in parallel. Returns a Job you can .get() later.
    /// 
    /// Example: `let job = system.run(compress_chunks, my_data);`
    pub fn run<F, T, R>(&self, func: F, data: T) -> Job<R>
    where
        F: FnOnce(T) -> R + Send + 'static,
        T: Send + 'static,
        R: Send + 'static,
    {
        LOCAL_RNG.with(|rng| {
            let work = move || Ok(func(data));
            
            match self.inner.submit_cpu_task(Priority::Normal, 10, work, &mut *rng.borrow_mut()) {
                Ok(handle) => Job { handle },
                Err(_) => panic!("🔥 System overloaded! Too many jobs running at once."),
            }
        })
    }
    
    /// Run a simple function in parallel (no input data needed).
    /// 
    /// Example: `let job = system.go(|| expensive_calculation());`
    pub fn go<F, R>(&self, func: F) -> Job<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.run(|_| func(), ())
    }
    
    /// Create an OVP socket for sending/receiving drone messages.
    /// 
    /// Example: `let socket = system.ovp_socket("eth0", my_drone_id);`
    pub fn ovp_socket(&self, interface: &str, my_drone_id: DroneId) -> OvpSocket {
        LOCAL_RNG.with(|rng| {
            let io_op = IoOp::OvpInit {
                interface: interface.to_string(),
                my_drone_id,
            };
            
            match self.inner.submit_io_task(io_op, Some(Duration::from_secs(10)), &mut *rng.borrow_mut()) {
                Ok(handle) => {
                    match handle.recv_result() {
                        Ok(Ok(IoOutput::OvpSocketReady { socket_token })) => OvpSocket {
                            token: socket_token,
                            system: Arc::clone(&self.inner),
                        },
                        Ok(Ok(_)) => panic!("🤔 Got weird response when creating OVP socket"),
                        Ok(Err(_)) => panic!("📡 Failed to create OVP socket - check your network interface!"),
                        Err(_) => panic!("📡 OVP socket creation was cancelled"),
                    }
                }
                Err(_) => panic!("💥 Failed to create OVP socket - system overloaded!"),
            }
        })
    }
}

impl Default for VibeSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Wait for all jobs to finish and collect results in order.
pub fn collect<T>(jobs: Vec<Job<T>>) -> Vec<T> {
    jobs.into_iter().map(|job| job.get()).collect()
}