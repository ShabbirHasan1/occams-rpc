#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(docsrs, allow(unused_attributes))]

//! # occams-rpc-tokio
//!
//! This crate provides a runtime adapter for [`occams-rpc`](https://docs.rs/occams-rpc) to work with the `tokio` runtime.
//! It implements the [`AsyncIO`](https://docs.rs/occams-rpc-core/latest/occams_rpc_core/runtime/index.html) trait to support `tokio`.
//!
use occams_rpc_core::io::io_with_timeout;
use occams_rpc_core::runtime::{AsyncFdTrait, AsyncIO, TimeInterval};
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
use tokio::runtime::Handle;

/// The main struct for tokio runtime IO, assign this type to AsyncIO trait when used:
///
/// - [ClientFactory::IO](https://occams-rpc-stream/latest/occams-rpc-stream/client/trait.ClientFactory.html)
///
/// - [ServerFactory::IO](https://occams-rpc-stream/latest/occams-rpc-stream/server/trait.ServerFactory.html)
pub struct TokioRT(Handle);

impl TokioRT {
    /// Capture a tokio runtime handle to ensure background task can spawn
    #[inline]
    pub fn new(handle: Handle) -> Self {
        Self(handle)
    }
}

impl AsyncIO for TokioRT {
    type Interval = TokioInterval;

    type AsyncFd<T: AsRawFd + AsFd + Send + Sync + 'static> = TokioFD<T>;

    #[inline(always)]
    fn sleep(d: Duration) -> impl Future + Send {
        tokio::time::sleep(d)
    }

    #[inline(always)]
    fn tick(d: Duration) -> Self::Interval {
        let later = tokio::time::Instant::now() + d;
        TokioInterval(tokio::time::interval_at(later, d))
    }

    #[inline(always)]
    async fn connect_tcp(
        addr: &SocketAddr, timeout: Duration,
    ) -> io::Result<Self::AsyncFd<TcpStream>> {
        let stream = io_with_timeout!(Self, timeout, tokio::net::TcpStream::connect(addr))?;
        // into_std will not change back to blocking
        Self::to_async_fd_rw(stream.into_std()?)
    }

    #[inline(always)]
    async fn connect_unix(
        addr: &PathBuf, timeout: Duration,
    ) -> io::Result<Self::AsyncFd<UnixStream>> {
        let stream = io_with_timeout!(Self, timeout, tokio::net::UnixStream::connect(addr))?;
        // into_std will not change back to blocking
        Self::to_async_fd_rw(stream.into_std()?)
    }

    #[inline(always)]
    fn to_async_fd_rd<T: AsRawFd + AsFd + Send + Sync + 'static>(
        fd: T,
    ) -> io::Result<Self::AsyncFd<T>> {
        use tokio::io;
        Ok(TokioFD(io::unix::AsyncFd::with_interest(fd, io::Interest::READABLE)?))
    }

    #[inline(always)]
    fn to_async_fd_rw<T: AsRawFd + AsFd + Send + Sync + 'static>(
        fd: T,
    ) -> io::Result<Self::AsyncFd<T>> {
        use tokio::io;
        use tokio::io::Interest;
        Ok(TokioFD(io::unix::AsyncFd::with_interest(fd, Interest::READABLE | Interest::WRITABLE)?))
    }

    /// spawn background coroutine with captured runtime handle
    #[inline]
    fn spawn_detach<F, R>(&self, f: F)
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static,
    {
        self.0.spawn(f);
    }
}

/// Associate type for TokioRT
pub struct TokioInterval(tokio::time::Interval);

impl TimeInterval for TokioInterval {
    #[inline]
    fn poll_tick(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Instant> {
        let _self = self.get_mut();
        if let Poll::Ready(i) = _self.0.poll_tick(ctx) {
            Poll::Ready(i.into_std())
        } else {
            Poll::Pending
        }
    }
}

/// Associate type for TokioRT
pub struct TokioFD<T: AsRawFd + AsFd + Send + Sync + 'static>(tokio::io::unix::AsyncFd<T>);

impl<T: AsRawFd + AsFd + Send + Sync + 'static> AsyncFdTrait<T> for TokioFD<T> {
    #[inline(always)]
    async fn async_read<R>(&self, f: impl FnMut(&T) -> io::Result<R> + Send) -> io::Result<R> {
        self.0.async_io(tokio::io::Interest::READABLE, f).await
    }

    #[inline(always)]
    async fn async_write<R>(&self, f: impl FnMut(&T) -> io::Result<R> + Send) -> io::Result<R> {
        self.0.async_io(tokio::io::Interest::WRITABLE, f).await
    }
}

impl<T: AsRawFd + AsFd + Send + Sync + 'static> Deref for TokioFD<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.0.get_ref()
    }
}

pub type ClientDefault<T, C> = occams_rpc_stream::client::ClientDefault<T, TokioRT, C>;
pub type ServerDefault = occams_rpc_stream::server::ServerDefault<TokioRT>;
