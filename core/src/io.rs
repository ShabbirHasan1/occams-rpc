//! I/O utilities

use pin_project_lite::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::*;
use std::{fmt, io};

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

/// AsyncRead trait for runtime adapter
pub trait AsyncRead: Send {
    /// Async version of read function
    ///
    /// On ok, return the bytes read
    fn read(&mut self, buf: &mut [u8]) -> impl Future<Output = io::Result<usize>> + Send;

    /// Read the exact number of bytes required to fill `buf`.
    ///
    /// This function repeatedly calls `read` until the buffer is completely filled.
    ///
    /// # Errors
    ///
    /// This function will return an error if the stream is closed before the
    /// buffer is filled.
    fn read_exact<'a>(
        &'a mut self, mut buf: &'a mut [u8],
    ) -> impl Future<Output = io::Result<()>> + Send + 'a {
        async move {
            while !buf.is_empty() {
                match self.read(buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let tmp = buf;
                        buf = &mut tmp[n..];
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(e),
                }
            }
            if !buf.is_empty() {
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "failed to fill whole buffer"))
            } else {
                Ok(())
            }
        }
    }

    /// Reads at least `min_len` bytes into `buf`.
    ///
    /// This function repeatedly calls `read` until at least `min_len` bytes have been
    /// read. It is allowed to read more than `min_len` bytes, but not more than
    /// the length of `buf`.
    ///
    /// # Returns
    ///
    /// On success, returns the total number of bytes read. This will be at least
    /// `min_len`, and could be more, up to the length of `buf`.
    ///
    /// # Errors
    ///
    /// It will return an `UnexpectedEof` error if the stream is closed before at least `min_len` bytes have been read.
    fn read_at_least<'a>(
        &'a mut self, buf: &'a mut [u8], min_len: usize,
    ) -> impl Future<Output = io::Result<usize>> + Send + 'a {
        async move {
            let mut total_read = 0;
            while total_read < min_len && total_read < buf.len() {
                match self.read(&mut buf[total_read..]).await {
                    Ok(0) => {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "failed to read minimum number of bytes",
                        ));
                    }
                    Ok(n) => total_read += n,
                    Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                };
            }
            Ok(total_read)
        }
    }
}

/// AsyncWrite trait for runtime adapter
pub trait AsyncWrite: Send {
    /// Async version of write function
    ///
    /// On ok, return the bytes written
    fn write(&mut self, buf: &[u8]) -> impl Future<Output = io::Result<usize>> + Send;

    /// Write the entire buffer `buf`.
    ///
    /// This function repeatedly calls `write` until the entire buffer is written.
    ///
    /// # Errors
    ///
    /// This function will return an error if the stream is closed before the
    /// entire buffer is written.
    fn write_all<'a>(
        &'a mut self, mut buf: &'a [u8],
    ) -> impl Future<Output = io::Result<()>> + Send + 'a {
        async move {
            while !buf.is_empty() {
                match self.write(buf).await {
                    Ok(0) => {
                        return Err(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "failed to write whole buffer",
                        ));
                    }
                    Ok(n) => {
                        buf = &buf[n..];
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        }
    }
}

/// A buffered reader that wraps an `AsyncRead` trait object and a buffer.
pub struct AsyncBufRead {
    buf: Vec<u8>,
    pos: usize,
    cap: usize,
}

impl AsyncBufRead {
    /// Creates a new `AsyncBufRead` with the given reader and buffer capacity.
    #[inline]
    pub fn new(capacity: usize) -> Self {
        AsyncBufRead { buf: vec![0; capacity], pos: 0, cap: 0 }
    }

    #[inline]
    pub async fn read_buffered<T: AsyncRead>(
        &mut self, reader: &mut T, buf: &mut [u8],
    ) -> io::Result<usize> {
        // If we have bytes in our buffer, copy them to `buf`.
        if self.pos < self.cap {
            let n = std::cmp::min(buf.len(), self.cap - self.pos);
            buf[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
            self.pos += n;
            return Ok(n);
        }

        // If the request is larger than our buffer, read directly into `buf`.
        // This avoids extra copying.
        if buf.len() >= self.buf.len() {
            return reader.read(buf).await;
        }

        // Otherwise, fill our buffer and then copy to `buf`.
        self.cap = reader.read(&mut self.buf).await?;
        self.pos = 0;
        let n = std::cmp::min(buf.len(), self.cap);
        buf[..n].copy_from_slice(&self.buf[..n]);
        self.pos += n;
        Ok(n)
    }
}

/// A buffered writer that wraps an `AsyncWrite` trait object and a buffer.
pub struct AsyncBufWrite {
    buf: Vec<u8>,
    pos: usize,
}

impl AsyncBufWrite {
    /// Creates a new `AsyncBufWrite` with the given writer and buffer capacity.
    #[inline]
    pub fn new(capacity: usize) -> Self {
        AsyncBufWrite { buf: vec![0; capacity], pos: 0 }
    }

    /// Flushes the buffered data to the underlying writer.
    #[inline]
    pub async fn flush<W: AsyncWrite>(&mut self, writer: &mut W) -> io::Result<()> {
        if self.pos > 0 {
            writer.write_all(&self.buf[..self.pos]).await?;
            self.pos = 0;
        }
        Ok(())
    }

    #[inline]
    pub async fn write_buffered<W: AsyncWrite>(
        &mut self, writer: &mut W, buf: &[u8],
    ) -> io::Result<usize> {
        // If the incoming buffer is larger than our internal buffer's capacity,
        // flush our buffer and write the incoming buffer directly.
        if buf.len() >= self.buf.len() {
            self.flush(writer).await?;
            return writer.write(buf).await;
        }

        // If the incoming buffer doesn't fit in the remaining space in our buffer,
        // flush our buffer.
        if self.buf.len() - self.pos < buf.len() {
            self.flush(writer).await?;
        }
        // Copy the incoming buffer into our internal buffer.
        let n = buf.len();
        self.buf[self.pos..self.pos + n].copy_from_slice(buf);
        self.pos += n;
        Ok(n)
    }
}

pub struct AsyncBufStream<T: AsyncRead + AsyncWrite> {
    read_buf: AsyncBufRead,
    write_buf: AsyncBufWrite,
    inner: T,
}

impl<T: AsyncRead + AsyncWrite> AsyncBufStream<T> {
    #[inline]
    pub fn new(stream: T, buf_size: usize) -> Self {
        Self {
            read_buf: AsyncBufRead::new(buf_size),
            write_buf: AsyncBufWrite::new(buf_size),
            inner: stream,
        }
    }

    #[inline(always)]
    pub async fn flush(&mut self) -> io::Result<()> {
        self.write_buf.flush(&mut self.inner).await
    }

    #[inline(always)]
    pub fn get_inner(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: AsyncRead + AsyncWrite + fmt::Debug> fmt::Debug for AsyncBufStream<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T: AsyncRead + AsyncWrite + fmt::Display> fmt::Display for AsyncBufStream<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T: AsyncRead + AsyncWrite> AsyncRead for AsyncBufStream<T> {
    /// Async version of read function
    ///
    /// On ok, return the bytes read
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> impl Future<Output = io::Result<usize>> + Send {
        async move { self.read_buf.read_buffered(&mut self.inner, buf).await }
    }
}

impl<T: AsyncRead + AsyncWrite> AsyncWrite for AsyncBufStream<T> {
    /// Async version of write function
    ///
    /// On ok, return the bytes written
    #[inline(always)]
    fn write(&mut self, buf: &[u8]) -> impl Future<Output = io::Result<usize>> + Send {
        async move { self.write_buf.write_buffered(&mut self.inner, buf).await }
    }
}

/// Interface for transport server listener
pub trait AsyncListener: Send + Sized + 'static + fmt::Debug {
    type Conn: Send + 'static + Sized;

    fn bind(addr: &str) -> io::Result<Self>;

    fn accept(&mut self) -> impl Future<Output = io::Result<Self::Conn>> + Send;

    fn local_addr(&self) -> io::Result<String>;
}

/// A trait to adapt various type of buffer
pub trait AllocateBuf: 'static + Sized + Send {
    /// Alloc buffer or reserve space to fit blob_len inside the Buffer.
    ///
    /// When size is not enough, return None
    fn reserve<'a>(&'a mut self, _blob_len: i32) -> Option<&'a mut [u8]>;
}

/// If Option is None, create a new `Vec<u8>` on call, otherwise grow to fit the requirement
impl AllocateBuf for Option<Vec<u8>> {
    #[inline]
    fn reserve<'a>(&'a mut self, blob_len: i32) -> Option<&'a mut [u8]> {
        let blob_len = blob_len as usize;
        if let Some(buf) = self.as_mut() {
            if buf.len() != blob_len {
                if buf.capacity() < blob_len {
                    buf.reserve(blob_len - buf.capacity());
                }
                unsafe { buf.set_len(blob_len) };
            }
        } else {
            let mut v = Vec::with_capacity(blob_len);
            unsafe { v.set_len(blob_len) };
            self.replace(v);
        }
        return self.as_deref_mut();
    }
}

/// Grow to fit the requirement
impl AllocateBuf for Vec<u8> {
    #[inline]
    fn reserve<'a>(&'a mut self, blob_len: i32) -> Option<&'a mut [u8]> {
        let blob_len = blob_len as usize;
        if self.len() != blob_len {
            if self.capacity() < blob_len {
                self.reserve(blob_len - self.capacity());
            }
            unsafe { self.set_len(blob_len) };
        }
        return Some(self);
    }
}

/// If Option is None, create a new [io_buffer::Buffer](https://docs.rs/io_buffer) on call.
/// Otherwise will check the pre-allocated buffer.
///
/// RPC will return encode error or decode error when the size is not enough.
impl AllocateBuf for Option<io_buffer::Buffer> {
    #[inline]
    fn reserve<'a>(&'a mut self, blob_len: i32) -> Option<&'a mut [u8]> {
        if let Some(buf) = self.as_mut() {
            let blob_len = blob_len as usize;
            if buf.len() != blob_len {
                if buf.capacity() < blob_len {
                    return None;
                }
                buf.set_len(blob_len);
            }
        } else {
            if let Ok(v) = io_buffer::Buffer::alloc(blob_len) {
                self.replace(v);
            } else {
                // alloc failed
                return None;
            }
        }
        return self.as_deref_mut();
    }
}

/// Check an pre-allocated [io_buffer::Buffer](https://docs.rs/io_buffer).
///
/// RPC will return encode error or decode error when the size is not enough.
impl AllocateBuf for io_buffer::Buffer {
    #[inline]
    fn reserve<'a>(&'a mut self, blob_len: i32) -> Option<&'a mut [u8]> {
        let blob_len = blob_len as usize;
        if self.len() != blob_len {
            if self.capacity() < blob_len {
                return None;
            }
            self.set_len(blob_len);
        }
        Some(self)
    }
}
