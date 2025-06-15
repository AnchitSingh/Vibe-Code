// src/io/poller.rs

// This module is Linux-only, as per our plan.

use std::os::unix::io::{AsRawFd, RawFd};
use std::io;

/// A thin, unsafe wrapper around the Linux `epoll` and `eventfd` APIs.
/// This component is responsible for all direct OS-level event notification calls.
pub(crate) struct Poller {
    epoll_fd: RawFd,
}

impl Poller {
    /// Creates a new `epoll` instance.
    pub fn new() -> io::Result<Self> {
        let epoll_fd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        if epoll_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { epoll_fd })
    }

    /// Adds a file descriptor to the epoll set with read and one-shot interests.
    /// The `token` is a unique u64 identifier for the connection instance.
    pub fn add_fd_for_read(&self, fd: RawFd, token: u64) -> io::Result<()> {
        self.add(fd, token, libc::EPOLLIN | libc::EPOLLONESHOT)
    }
    
    /// Adds a file descriptor to the epoll set with write and one-shot interests.
    pub fn add_fd_for_write(&self, fd: RawFd, token: u64) -> io::Result<()> {
        self.add(fd, token, libc::EPOLLOUT | libc::EPOLLONESHOT)
    }

    /// Re-arms a file descriptor for a new read interest.
    pub fn rearm_for_read(&self, fd: RawFd, token: u64) -> io::Result<()> {
        self.modify(fd, token, libc::EPOLLIN | libc::EPOLLONESHOT)
    }

    /// Re-arms a file descriptor for a new write interest.
    pub fn rearm_for_write(&self, fd: RawFd, token: u64) -> io::Result<()> {
        self.modify(fd, token, libc::EPOLLOUT | libc::EPOLLONESHOT)
    }

    /// Removes a file descriptor from the epoll set.
    pub fn remove_fd(&self, fd: RawFd) -> io::Result<()> {
        let mut event = libc::epoll_event { events: 0, u64: 0 }; // The event is ignored for remove
        let res = unsafe { libc::epoll_ctl(self.epoll_fd, libc::EPOLL_CTL_DEL, fd, &mut event) };
        if res < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Waits for events on the epoll file descriptor, filling the provided `events` buffer.
    pub fn wait(&self, events: &mut [libc::epoll_event], timeout_ms: i32) -> io::Result<usize> {
        let num_events = unsafe {
            libc::epoll_wait(self.epoll_fd, events.as_mut_ptr(), events.len() as i32, timeout_ms)
        };

        if num_events < 0 {
            let err = io::Error::last_os_error();
            // EINTR is a common case where the syscall was interrupted by a signal.
            // It's not a true error, and we can just retry.
            if err.kind() == io::ErrorKind::Interrupted {
                return Ok(0);
            }
            Err(err)
        } else {
            Ok(num_events as usize)
        }
    }

    /// Helper function for adding a new FD to epoll.
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

    /// Helper function for modifying an existing FD in epoll.
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
    fn drop(&mut self) {
        unsafe { libc::close(self.epoll_fd) };
    }
}

/// Creates a new `eventfd` for shutdown signaling.
pub(crate) fn create_shutdown_eventfd() -> io::Result<RawFd> {
    let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

/// Writes to the shutdown `eventfd` to signal the reactor to stop.
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