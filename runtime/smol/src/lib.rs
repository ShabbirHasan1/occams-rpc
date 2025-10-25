#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(docsrs, allow(unused_attributes))]

//! # occams-rpc-smol
//!
//! This crate provides a runtime adapter for [`occams-rpc`](https://docs.rs/occams-rpc) to work with the `smol` runtime.
//! It implements the [`AsyncIO`](https://docs.rs/occams-rpc-core/latest/occams_rpc_core/runtime/index.html) trait to support `async-io` of `smol`.

use async_executor::Executor;
use async_io::{Async, Timer};
use occams_rpc_core::io::*;
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
use std::sync::Arc;
use std::task::*;
use std::time::{Duration, Instant};

/// The main struct for async-io, assign this type to AsyncIO trait when used:
///
/// - [ClientFactory::IO](https://occams-rpc-stream/latest/occams-rpc-stream/client/trait.ClientFactory.html)
///
/// - [ServerFactory::IO](https://occams-rpc-stream/latest/occams-rpc-stream/server/trait.ServerFactory.html)
pub struct SmolRT(Option<Arc<Executor<'static>>>);

impl SmolRT {
    #[cfg(feature = "global")]
    #[inline]
    pub fn new_global() -> Self {
        Self(None)
    }

    #[inline]
    pub fn new(executor: Arc<Executor<'static>>) -> Self {
        Self(Some(executor))
    }
}

impl AsyncIO for SmolRT {
    type Interval = SmolInterval;

    type AsyncFd<T: AsRawFd + AsFd + Send + Sync + 'static> = SmolFD<T>;

    #[inline(always)]
    fn sleep(d: Duration) -> impl Future + Send {
        Timer::after(d)
    }

    #[inline(always)]
    fn tick(d: Duration) -> Self::Interval {
        let later = std::time::Instant::now() + d;
        SmolInterval(Timer::interval_at(later, d))
    }

    #[inline(always)]
    async fn connect_tcp(
        addr: &SocketAddr, timeout: Duration,
    ) -> io::Result<Self::AsyncFd<TcpStream>> {
        let _addr = addr.clone();
        let stream = io_with_timeout!(Self, timeout, Async::<TcpStream>::connect(_addr))?;
        // into_inner will not change back to blocking
        Self::to_async_fd_rw(stream.into_inner()?)
    }

    #[inline(always)]
    async fn connect_unix(
        addr: &PathBuf, timeout: Duration,
    ) -> io::Result<Self::AsyncFd<UnixStream>> {
        let _addr = addr.clone();
        let stream = io_with_timeout!(Self, timeout, Async::<UnixStream>::connect(_addr))?;
        // into_inner will not change back to blocking
        Self::to_async_fd_rw(stream.into_inner()?)
    }

    #[inline(always)]
    fn to_async_fd_rd<T: AsRawFd + AsFd + Send + Sync + 'static>(
        fd: T,
    ) -> io::Result<Self::AsyncFd<T>> {
        Ok(SmolFD(Async::new(fd)?))
    }

    #[inline(always)]
    fn to_async_fd_rw<T: AsRawFd + AsFd + Send + Sync + 'static>(
        fd: T,
    ) -> io::Result<Self::AsyncFd<T>> {
        Ok(SmolFD(Async::new(fd)?))
    }

    #[inline]
    fn spawn_detach<F, R>(&self, f: F)
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static,
    {
        if let Some(executor) = self.0.as_ref() {
            executor.spawn(f).detach();
        } else {
            #[cfg(feature = "global")]
            {
                smol::spawn(f).detach();
                return;
            }
            #[cfg(not(feature = "global"))]
            unreachable!();
        }
    }
}

/// Associate type for SmolRT
pub struct SmolInterval(Timer);

impl TimeInterval for SmolInterval {
    #[inline]
    fn poll_tick(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Instant> {
        let _self = self.get_mut();
        use futures::stream::StreamExt;
        match _self.0.poll_next_unpin(ctx) {
            Poll::Ready(Some(i)) => Poll::Ready(i),
            Poll::Ready(None) => unreachable!(),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Associate type for SmolRT
pub struct SmolFD<T: AsRawFd + AsFd + Send + Sync + 'static>(Async<T>);

impl<T: AsRawFd + AsFd + Send + Sync + 'static> AsyncFdTrait<T> for SmolFD<T> {
    #[inline(always)]
    async fn async_read<R>(&self, f: impl FnMut(&T) -> io::Result<R> + Send) -> io::Result<R> {
        self.0.read_with(f).await
    }

    #[inline(always)]
    async fn async_write<R>(&self, f: impl FnMut(&T) -> io::Result<R> + Send) -> io::Result<R> {
        self.0.write_with(f).await
    }
}

impl<T: AsRawFd + AsFd + Send + Sync + 'static> Deref for SmolFD<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.0.get_ref()
    }
}

pub type ClientDefault<T, C> = occams_rpc_stream::client::ClientDefault<T, SmolRT, C>;
pub type ServerDefault = occams_rpc_stream::server::ServerDefault<SmolRT>;
