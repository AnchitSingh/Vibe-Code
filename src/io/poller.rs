//! Provides a low-level, unsafe wrapper around Linux `epoll` and `eventfd` APIs.
//!
//! This module is specifically designed for Linux-based systems and handles
//! direct operating system calls for efficient I/O event notification.
//! It is an internal component of the `GlobalReactor`.

use std::io;
use std::os::unix::io::RawFd;

/// A thin, unsafe wrapper around the Linux `epoll` API.
///
/// This struct is responsible for managing file descriptors (FDs) within an
/// `epoll` instance, allowing the `GlobalReactor` to efficiently wait for
/// I/O events. It directly interacts with `libc` functions.
pub(crate) struct Poller {
    /// The file descriptor for the `epoll` instance.
    epoll_fd: RawFd,
}

impl Poller {
    /// Creates a new `epoll` instance.
    ///
    /// Uses `epoll_create1` with `EPOLL_CLOEXEC` to ensure the epoll FD is
    /// closed on `execve`.
    ///
    /// # Returns
    ///
    /// A `Result` containing a new `Poller` instance on success, or an `io::Error`
    /// if the `epoll` instance could not be created.
    pub fn new() -> io::Result<Self> {
        let epoll_fd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        if epoll_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { epoll_fd })
    }

    /// Adds a file descriptor to the epoll set with read interest and `EPOLLONESHOT`.
    ///
    /// `EPOLLONESHOT` ensures that the event is reported only once, requiring
    /// explicit re-arming after handling.
    ///
    /// # Arguments
    ///
    /// * `fd` - The raw file descriptor to add.
    /// * `token` - A unique `u64` identifier associated with this FD.
    ///
    /// # Returns
    ///
    /// An `io::Result` indicating success or failure.
    pub fn add_fd_for_read(&self, fd: RawFd, token: u64) -> io::Result<()> {
        self.add(fd, token, libc::EPOLLIN | libc::EPOLLONESHOT)
    }

    /// Adds a file descriptor to the epoll set with write interest and `EPOLLONESHOT`.
    ///
    /// # Arguments
    ///
    /// * `fd` - The raw file descriptor to add.
    /// * `token` - A unique `u64` identifier associated with this FD.
    ///
    /// # Returns
    ///
    /// An `io::Result` indicating success or failure.
    pub fn add_fd_for_write(&self, fd: RawFd, token: u64) -> io::Result<()> {
        self.add(fd, token, libc::EPOLLOUT | libc::EPOLLONESHOT)
    }

    /// Re-arms an existing file descriptor in the epoll set for a new read interest.
    ///
    /// This is used after an `EPOLLONESHOT` event has been received and processed,
    /// to continue monitoring the FD for read events.
    ///
    /// # Arguments
    ///
    /// * `fd` - The raw file descriptor to re-arm.
    /// * `token` - The unique `u64` identifier associated with this FD.
    ///
    /// # Returns
    ///
    /// An `io::Result` indicating success or failure.
    pub fn rearm_for_read(&self, fd: RawFd, token: u64) -> io::Result<()> {
        self.modify(fd, token, libc::EPOLLIN | libc::EPOLLONESHOT)
    }

    /// Re-arms an existing file descriptor in the epoll set for a new write interest.
    ///
    /// This is used after an `EPOLLONESHOT` event has been received and processed,
    /// to continue monitoring the FD for write events.
    ///
    /// # Arguments
    ///
    /// * `fd` - The raw file descriptor to re-arm.
    /// * `token` - The unique `u64` identifier associated with this FD.
    ///
    /// # Returns
    ///
    /// An `io::Result` indicating success or failure.
    pub fn rearm_for_write(&self, fd: RawFd, token: u64) -> io::Result<()> {
        self.modify(fd, token, libc::EPOLLOUT | libc::EPOLLONESHOT)
    }

    /// Removes a file descriptor from the epoll set.
    ///
    /// # Arguments
    ///
    /// * `fd` - The raw file descriptor to remove.
    ///
    /// # Returns
    ///
    /// An `io::Result` indicating success or failure.
    pub fn remove_fd(&self, fd: RawFd) -> io::Result<()> {
        let mut event = libc::epoll_event { events: 0, u64: 0 }; // Event is ignored for DEL operation
        let res = unsafe { libc::epoll_ctl(self.epoll_fd, libc::EPOLL_CTL_DEL, fd, &mut event) };
        if res < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Waits for I/O events on the epoll file descriptor.
    ///
    /// This function blocks until events are available or the `timeout_ms` expires.
    /// It fills the provided `events` buffer with the triggered events.
    ///
    /// # Arguments
    ///
    /// * `events` - A mutable slice to store the `epoll_event`s.
    /// * `timeout_ms` - The maximum time to wait for events, in milliseconds.
    ///                  A value of -1 means infinite timeout.
    ///
    /// # Returns
    ///
    /// A `Result` containing the number of events received on success, or an `io::Error`.
    /// Returns `Ok(0)` if the wait was interrupted by a signal (`EINTR`).
    pub fn wait(&self, events: &mut [libc::epoll_event], timeout_ms: i32) -> io::Result<usize> {
        let num_events = unsafe {
            libc::epoll_wait(self.epoll_fd, events.as_mut_ptr(), events.len() as i32, timeout_ms)
        };

        if num_events < 0 {
            let err = io::Error::last_os_error();
            // EINTR is a common case where the syscall was interrupted by a signal.
            // It's not a true error, and we can just retry by returning 0 events.
            if err.kind() == io::ErrorKind::Interrupted {
                return Ok(0);
            }
            Err(err)
        } else {
            Ok(num_events as usize)
        }
    }

    /// Helper function to add a new file descriptor to the epoll set.
    ///
    /// # Arguments
    ///
    /// * `fd` - The file descriptor to add.
    /// * `token` - The user-defined data associated with the file descriptor.
    /// * `interests` - The event interests (e.g., `EPOLLIN`, `EPOLLOUT`).
    ///
    /// # Returns
    ///
    /// An `io::Result` indicating success or failure.
    fn add(&self, fd: RawFd, token: u64, interests: i32) -> io::Result<()> {
        let mut event = libc::epoll_event {
            events: interests as u32,
            u64: token,
        };
        let res = unsafe { libc::epoll_ctl(self.epoll_fd, libc::EPOLL_CTL_ADD, fd, &mut event) };
        if res < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Helper function to modify an existing file descriptor's interests in the epoll set.
    ///
    /// # Arguments
    ///
    /// * `fd` - The file descriptor to modify.
    /// * `token` - The user-defined data associated with the file descriptor.
    /// * `interests` - The new event interests.
    ///
    /// # Returns
    ///
    /// An `io::Result` indicating success or failure.
    fn modify(&self, fd: RawFd, token: u64, interests: i32) -> io::Result<()> {
        let mut event = libc::epoll_event {
            events: interests as u32,
            u64: token,
        };
        let res = unsafe { libc::epoll_ctl(self.epoll_fd, libc::EPOLL_CTL_MOD, fd, &mut event) };
        if res < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

impl Drop for Poller {
    /// Closes the epoll file descriptor when the `Poller` instance is dropped.
    fn drop(&mut self) {
        unsafe { libc::close(self.epoll_fd) };
    }
}

/// Creates a new `eventfd` for inter-thread signaling, typically for shutdown.
///
/// The `eventfd` is created with `EFD_CLOEXEC` and `EFD_NONBLOCK` flags.
///
/// # Returns
///
/// A `Result` containing the raw file descriptor of the `eventfd` on success,
/// or an `io::Error` if creation fails.
pub(crate) fn create_shutdown_eventfd() -> io::Result<RawFd> {
    let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

/// Writes to the shutdown `eventfd` to signal the reactor to stop.
///
/// This function writes a `u64` value of 1 to the `eventfd`, which will
/// trigger an event in the `epoll` loop, signaling the reactor to shut down.
///
/// # Arguments
///
/// * `fd` - The raw file descriptor of the `eventfd`.
pub(crate) fn signal_shutdown(fd: RawFd) {
    let value: u64 = 1;
    let _ = unsafe {
        libc::write(
            fd,
            &value as *const u64 as *const std::ffi::c_void,
            std::mem::size_of::<u64>(),
        )
    };
}