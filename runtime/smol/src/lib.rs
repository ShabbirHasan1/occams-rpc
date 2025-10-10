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
use std::task::*;
use std::time::{Duration, Instant};

use async_io::{Async, Timer};

pub struct SmolRT();

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
}

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
