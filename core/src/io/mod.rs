//! I/O utilities

use pin_project_lite::pin_project;
use std::future::Future;
use std::os::fd::{AsRawFd, RawFd};
use std::pin::Pin;
use std::task::*;
use std::{fmt, io};

mod buffer;
pub use buffer::AllocateBuf;
mod buf_io;
pub use buf_io::{AsyncBufRead, AsyncBufStream, AsyncBufWrite, AsyncRead, AsyncWrite};

pin_project! {
    /// Cancellable accepts a param `future` for I/O,
    /// abort the I/O waiting when `cancel_future` returns.
    ///
    /// The `cancel_future` can be timer or notification channel recv()
    pub struct Cancellable<F, C> {
        #[pin]
        future: F,
        #[pin]
        cancel_future: C,
    }
}

impl<F: Future + Send, C: Future + Send> Cancellable<F, C> {
    pub fn new(future: F, cancel_future: C) -> Self {
        Self { future, cancel_future }
    }
}

impl<F: Future + Send, C: Future + Send> Future for Cancellable<F, C> {
    type Output = Result<F::Output, ()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut _self = self.project();
        let future = unsafe { Pin::new_unchecked(&mut _self.future) };
        if let Poll::Ready(output) = future.poll(cx) {
            return Poll::Ready(Ok(output));
        }
        let cancel_future = unsafe { Pin::new_unchecked(&mut _self.cancel_future) };
        if let Poll::Ready(_) = cancel_future.poll(cx) {
            return Poll::Ready(Err(()));
        }
        return Poll::Pending;
    }
}

/// Because timeout function return () as error, this macro convert to io::Error
#[macro_export(local_inner_macros)]
macro_rules! io_with_timeout {
    ($IO: path, $timeout: expr, $f: expr) => {{
        if $timeout == Duration::from_secs(0) {
            $f.await
        } else {
            match <$IO as AsyncIO>::timeout($timeout, $f).await {
                Ok(Ok(r)) => Ok(r),
                Ok(Err(e)) => Err(e),
                Err(_) => Err(io::ErrorKind::TimedOut.into()),
            }
        }
    }};
}
pub use io_with_timeout;

/// Interface for transport server listener
pub trait AsyncListener: Send + Sized + 'static + fmt::Debug {
    type Conn: Send + 'static + Sized;

    fn bind(addr: &str) -> io::Result<Self>;

    fn accept(&mut self) -> impl Future<Output = io::Result<Self::Conn>> + Send;

    fn local_addr(&self) -> io::Result<String>;

    /// Try to recover a listener from RawFd
    ///
    /// Will set listener to non_blocking to validate the fd
    ///
    /// # Arguments
    ///
    /// * addr: the addr is for determine address type
    unsafe fn try_from_raw_fd(addr: &str, raw_fd: RawFd) -> io::Result<Self>
    where
        Self: AsRawFd;
}
