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
pub trait AsyncRead: Send + 'static {
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
pub trait AsyncWrite: Send + 'static {
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

/// Interface for transport server listener
pub trait AsyncListener: Send + Sized + 'static + fmt::Debug {
    type Conn: Send + 'static + Sized;

    fn bind(addr: &str) -> io::Result<Self>;

    fn accept(&mut self) -> impl Future<Output = io::Result<Self::Conn>> + Send;

    fn local_addr(&self) -> io::Result<std::net::SocketAddr>;
}

///// Buffered IO for UnifyStream
//pub struct UnifyBufStream {
//    reader_buf_size: usize,
//    writer_buf_size: usize,
//    buf_stream: BufStream<UnifyStream>,
//}
//
//impl UnifyBufStream {
//    #[inline(always)]
//    pub fn new(unify_stream: UnifyStream) -> Self {
//        Self {
//            reader_buf_size: 8 * 1024,
//            writer_buf_size: 8 * 1024,
//            buf_stream: BufStream::with_capacity(8 * 1024, 8 * 1024, unify_stream),
//        }
//    }
//
//    #[inline(always)]
//    pub fn with_capacity(
//        reader_buf_size: usize, writer_buf_size: usize, unify_stream: UnifyStream,
//    ) -> Self {
//        Self {
//            reader_buf_size,
//            writer_buf_size,
//            buf_stream: BufStream::with_capacity(reader_buf_size, writer_buf_size, unify_stream),
//        }
//    }
//
//    #[inline(always)]
//    pub async fn close(&mut self) -> io::Result<()> {
//        self.buf_stream.shutdown().await
//    }
//
//    #[inline(always)]
//    pub async fn read_exact(&mut self, dst: &mut [u8]) -> Result<usize, io::Error> {
//        self.buf_stream.read_exact(dst).await
//    }
//
//    #[inline(always)]
//    pub async fn read_exact_timeout(
//        &mut self, dst: &mut [u8], read_timeout: Duration,
//    ) -> Result<usize, io::Error> {
//        if read_timeout == ZERO_TIME {
//            return self.buf_stream.read_exact(dst).await;
//        } else {
//            match timeout(read_timeout, self.buf_stream.read_exact(dst)).await {
//                Ok(o) => match o {
//                    Ok(l) => return Ok(l),
//                    Err(e2) => return Err(e2),
//                },
//                Err(e) => {
//                    return Err(e.into());
//                }
//            }
//        }
//    }
//
//    #[inline(always)]
//    pub async fn write_all(&mut self, dst: &[u8]) -> Result<(), io::Error> {
//        self.buf_stream.write_all(dst).await
//    }
//
//    #[inline(always)]
//    pub async fn write_timeout(
//        &mut self, dst: &[u8], write_timeout: Duration,
//    ) -> Result<(), io::Error> {
//        if write_timeout == ZERO_TIME {
//            return self.buf_stream.write_all(dst).await;
//        } else {
//            match timeout(write_timeout, self.buf_stream.write_all(dst)).await {
//                Ok(o) => match o {
//                    Ok(_) => return Ok(()),
//                    Err(e2) => return Err(e2),
//                },
//                Err(e) => {
//                    return Err(e.into());
//                }
//            }
//        }
//    }
//
//    #[inline(always)]
//    pub async fn flush_timeout(&mut self, write_timeout: Duration) -> Result<(), io::Error> {
//        if write_timeout == ZERO_TIME {
//            return self.buf_stream.flush().await;
//        } else {
//            match timeout(write_timeout, self.buf_stream.flush()).await {
//                Ok(o) => match o {
//                    Ok(_) => return Ok(()),
//                    Err(e2) => return Err(e2),
//                },
//                Err(e) => {
//                    return Err(e.into());
//                }
//            }
//        }
//    }
//}
//
//impl std::fmt::Display for UnifyBufStream {
//    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//        return write!(
//            f,
//            "UnifyBufStream reader_buf_size:{} writer_buf_size:{}, stream:{:#}",
//            self.reader_buf_size,
//            self.writer_buf_size,
//            self.buf_stream.get_ref()
//        );
//    }
//}
//

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
