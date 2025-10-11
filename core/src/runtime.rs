//! The runtime model defines interface to adapt various async runtimes.
//!
//! The adaptor are provided as sub-crates:
//!
//! - [occams-rpc-tokio](https://docs.rs/occams-rpc-tokio)
//!
//! - [occams-rpc-smol](https://docs.rs/occams-rpc-smol)
//!
//! See the usage doc in [occams-rpc-stream](https://docs.rs/occams-rpc-stream)

use crate::io::Cancellable;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::ops::Deref;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::*;
use std::time::{Duration, Instant};

/// The trait of async fd to turn sync I/O to async
///
/// See module level doc: [crate::runtime]
pub trait AsyncFdTrait<T: AsRawFd + AsFd + Send + Sync + 'static>:
    Send + Sync + 'static + Deref<Target = T>
{
    fn async_read<R>(
        &self, f: impl FnMut(&T) -> io::Result<R> + Send,
    ) -> impl Future<Output = io::Result<R>> + Send;

    fn async_write<R>(
        &self, f: impl FnMut(&T) -> io::Result<R> + Send,
    ) -> impl Future<Output = io::Result<R>> + Send;
}

/// Defines the interface we used from async runtime
///
/// See module level doc: [crate::runtime]
pub trait AsyncIO: Send + 'static {
    type Interval: TimeInterval;

    type AsyncFd<T: AsRawFd + AsFd + Send + Sync + 'static>: AsyncFdTrait<T>;

    fn sleep(d: Duration) -> impl Future + Send;

    fn tick(d: Duration) -> Self::Interval;

    #[inline]
    fn timeout<F>(d: Duration, func: F) -> impl Future<Output = Result<F::Output, ()>> + Send
    where
        F: Future + Send,
    {
        Cancellable::new(func, Self::sleep(d))
    }

    fn connect_tcp(
        addr: &SocketAddr, timeout: Duration,
    ) -> impl Future<Output = io::Result<Self::AsyncFd<TcpStream>>> + Send;

    fn connect_unix(
        addr: &PathBuf, timeout: Duration,
    ) -> impl Future<Output = io::Result<Self::AsyncFd<UnixStream>>> + Send;

    /// Required to set_nonblocking first
    fn to_async_fd_rd<T: AsRawFd + AsFd + Send + Sync + 'static>(
        fd: T,
    ) -> io::Result<Self::AsyncFd<T>>;

    /// Required to set_nonblocking first
    fn to_async_fd_rw<T: AsRawFd + AsFd + Send + Sync + 'static>(
        fd: T,
    ) -> io::Result<Self::AsyncFd<T>>;
}

/// Defines the universal interval/ticker trait
pub trait TimeInterval: Unpin + Send {
    fn poll_tick(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Instant>;
}
