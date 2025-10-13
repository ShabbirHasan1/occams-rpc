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

impl<'a, R: ?Sized + AsyncRead> AsyncRead for &'a mut R {
    fn read(&mut self, buf: &mut [u8]) -> impl Future<Output = io::Result<usize>> + Send {
        (**self).read(buf)
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

impl<'a, W: ?Sized + AsyncWrite> AsyncWrite for &'a mut W {
    fn write(&mut self, buf: &[u8]) -> impl Future<Output = io::Result<usize>> + Send {
        (**self).write(buf)
    }
}

/// A buffered reader that wraps an `AsyncRead` trait object and a buffer.
pub struct AsyncBufRead<'a, T: ?Sized> {
    inner: &'a mut T,
    buf: Vec<u8>,
    pos: usize,
    cap: usize,
}

impl<'a, T: ?Sized + AsyncRead> AsyncBufRead<'a, T> {
    /// Creates a new `AsyncBufRead` with the given reader and buffer capacity.
    pub fn new(inner: &'a mut T, capacity: usize) -> Self {
        AsyncBufRead { inner, buf: vec![0; capacity], pos: 0, cap: 0 }
    }
}

impl<'a, T: ?Sized + AsyncRead> AsyncRead for AsyncBufRead<'a, T> {
    fn read(&mut self, buf: &mut [u8]) -> impl Future<Output = io::Result<usize>> + Send {
        async move {
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
                return self.inner.read(buf).await;
            }

            // Otherwise, fill our buffer and then copy to `buf`.
            self.cap = self.inner.read(&mut self.buf).await?;
            self.pos = 0;

            let n = std::cmp::min(buf.len(), self.cap);
            buf[..n].copy_from_slice(&self.buf[..n]);
            self.pos += n;
            Ok(n)
        }
    }
}

/// A buffered writer that wraps an `AsyncWrite` trait object and a buffer.
pub struct AsyncBufWrite<'a, W: ?Sized> {
    inner: &'a mut W,
    buf: Vec<u8>,
    pos: usize,
}

impl<'a, W: ?Sized + AsyncWrite> AsyncBufWrite<'a, W> {
    /// Creates a new `AsyncBufWrite` with the given writer and buffer capacity.
    pub fn new(inner: &'a mut W, capacity: usize) -> Self {
        AsyncBufWrite { inner, buf: vec![0; capacity], pos: 0 }
    }

    /// Flushes the buffered data to the underlying writer.
    pub async fn flush(&mut self) -> io::Result<()> {
        if self.pos > 0 {
            self.inner.write_all(&self.buf[..self.pos]).await?;
            self.pos = 0;
        }
        Ok(())
    }

    /// Flushes the buffered data and consumes the writer.
    pub async fn close(mut self) -> io::Result<()> {
        self.flush().await?;
        Ok(())
    }
}

impl<'a, W: ?Sized + AsyncWrite> AsyncWrite for AsyncBufWrite<'a, W> {
    fn write(&mut self, buf: &[u8]) -> impl Future<Output = io::Result<usize>> + Send {
        async move {
            // If the incoming buffer is larger than our internal buffer's capacity,
            // flush our buffer and write the incoming buffer directly.
            if buf.len() >= self.buf.len() {
                self.flush().await?;
                return self.inner.write(buf).await;
            }

            // If the incoming buffer doesn't fit in the remaining space in our buffer,
            // flush our buffer.
            if self.buf.len() - self.pos < buf.len() {
                self.flush().await?;
            }

            // Copy the incoming buffer into our internal buffer.
            let n = buf.len();
            self.buf[self.pos..self.pos + n].copy_from_slice(buf);
            self.pos += n;
            Ok(n)
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

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, RngCore};
    use std::future::Future;
    use std::sync::{Arc, Mutex};

    enum MockReadBehavior {
        Chunked(Vec<Vec<u8>>),
        Randomized { data: Vec<u8>, pos: usize },
    }

    // A mock reader that can return data in fixed chunks or in random sub-chunks.
    struct MockReader {
        behavior: MockReadBehavior,
    }

    impl MockReader {
        fn new_chunked(chunks: Vec<Vec<u8>>) -> Self {
            Self { behavior: MockReadBehavior::Chunked(chunks) }
        }

        fn new_randomized(data: Vec<u8>) -> Self {
            Self { behavior: MockReadBehavior::Randomized { data, pos: 0 } }
        }
    }

    impl AsyncRead for MockReader {
        fn read(&mut self, buf: &mut [u8]) -> impl Future<Output = io::Result<usize>> + Send {
            async move {
                match &mut self.behavior {
                    MockReadBehavior::Chunked(chunks) => {
                        if chunks.is_empty() {
                            return Ok(0);
                        }
                        let chunk = chunks.remove(0);
                        let n = std::cmp::min(buf.len(), chunk.len());
                        buf[..n].copy_from_slice(&chunk[..n]);
                        Ok(n)
                    }
                    MockReadBehavior::Randomized { data, pos } => {
                        let mut rng = rand::thread_rng();
                        let remaining = data.len() - *pos;
                        if remaining == 0 {
                            return Ok(0); // True EOF
                        }
                        let max_read = std::cmp::min(buf.len(), remaining);
                        let read_size = rng.gen_range(1..=max_read);

                        buf[..read_size].copy_from_slice(&data[*pos..*pos + read_size]);
                        *pos += read_size;
                        Ok(read_size)
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_async_buf_read_exact_random_chunks() {
        let mut rng = rand::thread_rng();
        let data_size = rng.gen_range(1024..4096);
        let mut source_data = vec![0u8; data_size];
        rng.fill_bytes(&mut source_data);

        let mut chunks = Vec::new();
        let mut remaining_data = &source_data[..];
        while !remaining_data.is_empty() {
            let chunk_size = rng.gen_range(1..128).min(remaining_data.len());
            chunks.push(remaining_data[..chunk_size].to_vec());
            remaining_data = &remaining_data[chunk_size..];
        }

        let mut mock_reader = MockReader::new_chunked(chunks);
        let mut reader = AsyncBufRead::new(&mut mock_reader, 256);

        let mut out = vec![0u8; data_size];
        reader.read_exact(&mut out).await.unwrap();
        assert_eq!(out, source_data);
    }

    #[tokio::test]
    async fn test_async_buf_read_bypass_random() {
        let mut rng = rand::thread_rng();
        let data_size = rng.gen_range(257..512);
        let mut source_data = vec![0u8; data_size];
        rng.fill_bytes(&mut source_data);

        let mut mock_reader = MockReader::new_chunked(vec![source_data.clone()]);
        let mut reader = AsyncBufRead::new(&mut mock_reader, 256);

        let mut out = vec![0u8; data_size];
        reader.read_exact(&mut out).await.unwrap();
        assert_eq!(out, source_data);
    }

    #[tokio::test]
    async fn test_async_buf_read_multiple_reads_random() {
        let mut rng = rand::thread_rng();
        let chunk1_size = rng.gen_range(64..128);
        let mut chunk1_data = vec![0u8; chunk1_size];
        rng.fill_bytes(&mut chunk1_data);

        let chunk2_size = rng.gen_range(64..128);
        let mut chunk2_data = vec![0u8; chunk2_size];
        rng.fill_bytes(&mut chunk2_data);

        let chunks = vec![chunk1_data.clone(), chunk2_data.clone()];
        let mut mock_reader = MockReader::new_chunked(chunks);
        let mut reader = AsyncBufRead::new(&mut mock_reader, 100);

        let mut out1 = vec![0u8; chunk1_size];
        reader.read_exact(&mut out1).await.unwrap();
        assert_eq!(out1, chunk1_data);

        let mut out2 = vec![0u8; chunk2_size];
        reader.read_exact(&mut out2).await.unwrap();
        assert_eq!(out2, chunk2_data);
    }

    #[tokio::test]
    async fn test_random_read_sizes_and_returns() {
        let mut rng = rand::thread_rng();
        let data_size = rng.gen_range(8192..16384);
        let mut source_data = vec![0u8; data_size];
        rng.fill_bytes(&mut source_data);

        let mut mock_reader = MockReader::new_randomized(source_data.clone());
        let mut reader = AsyncBufRead::new(&mut mock_reader, 1024);

        let mut result_data = Vec::with_capacity(data_size);
        while result_data.len() < data_size {
            let read_size = rng.gen_range(1..=512);
            let mut temp_buf = vec![0u8; read_size];
            match reader.read(&mut temp_buf).await {
                Ok(0) => break, // EOF
                Ok(n) => {
                    result_data.extend_from_slice(&temp_buf[..n]);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => panic!("Read failed: {}", e),
            }
        }

        assert_eq!(result_data.len(), data_size);
        assert_eq!(result_data, source_data);
    }

    struct MockWriter {
        data: Arc<Mutex<Vec<u8>>>,
    }

    impl MockWriter {
        fn new(data: Arc<Mutex<Vec<u8>>>) -> Self {
            Self { data }
        }
    }

    impl AsyncWrite for MockWriter {
        fn write(&mut self, buf: &[u8]) -> impl Future<Output = io::Result<usize>> + Send {
            self.data.lock().unwrap().extend_from_slice(buf);
            let n = buf.len();
            async move { Ok(n) }
        }
    }

    #[tokio::test]
    async fn test_async_buf_write_all_buffering() {
        let data_handle = Arc::new(Mutex::new(Vec::new()));
        let mut mock_writer = MockWriter::new(data_handle.clone());
        let mut writer = AsyncBufWrite::new(&mut mock_writer, 8);

        writer.write_all(b"hello").await.unwrap();
        assert!(data_handle.lock().unwrap().is_empty()); // buffered

        writer.write_all(b" wo").await.unwrap(); // total 5+3=8, should not flush yet
        assert!(data_handle.lock().unwrap().is_empty()); // still buffered, pos is 8

        writer.write_all(b"rld").await.unwrap(); // overflows buffer
        // "hello wo" should be flushed.
        assert_eq!(*data_handle.lock().unwrap(), b"hello wo");
        // "rld" is in the buffer
        assert_eq!(writer.pos, 3);

        writer.close().await.unwrap();
        assert_eq!(*data_handle.lock().unwrap(), b"hello world");
    }

    #[tokio::test]
    async fn test_async_buf_write_bypass() {
        let data_handle = Arc::new(Mutex::new(Vec::new()));
        let mut mock_writer = MockWriter::new(data_handle.clone());
        let mut writer = AsyncBufWrite::new(&mut mock_writer, 8);

        writer.write_all(b"abc").await.unwrap();
        assert!(data_handle.lock().unwrap().is_empty()); // buffered

        // This write is larger than the buffer, it should bypass it.
        writer.write_all(b"this is a long line").await.unwrap();
        // The buffer "abc" should be flushed first.
        // Then "this is a long line" is written directly.
        assert_eq!(*data_handle.lock().unwrap(), b"abcthis is a long line");

        writer.close().await.unwrap();
        assert_eq!(*data_handle.lock().unwrap(), b"abcthis is a long line");
    }
}
