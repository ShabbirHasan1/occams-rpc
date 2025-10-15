use crate::net::{QuicListener, QuicStream};
use io_buffer::Buffer;
use occams_rpc_core::io::{AsyncBufStream, AsyncRead, AsyncWrite, Cancellable};
use occams_rpc_core::runtime::AsyncIO;
use occams_rpc_core::{ServerConfig, error::*};
use occams_rpc_stream::server::{RpcSvrReq, ServerFactory, ServerTransport};
use occams_rpc_stream::{proto, proto::RpcAction};
use std::cell::UnsafeCell;
use std::sync::Arc;
use std::time::Duration;
use std::{fmt, io};
use zerocopy::AsBytes;

pub const SERVER_DEFAULT_BUF_SIZE: usize = 8 * 1024;

pub struct QuicServer<F: ServerFactory> {
    stream: UnsafeCell<AsyncBufStream<QuicStream>>,
    _conn_count: Arc<()>,
    config: ServerConfig,
    action_buf: UnsafeCell<Vec<u8>>,
    msg_buf: UnsafeCell<Vec<u8>>,
    logger: F::Logger,
    _io: std::marker::PhantomData<F::IO>,
}

unsafe impl<F: ServerFactory> Sync for QuicServer<F> {}

impl<F: ServerFactory> QuicServer<F> {
    #[inline(always)]
    fn get_stream_mut(&self) -> &mut AsyncBufStream<QuicStream> {
        unsafe { std::mem::transmute(self.stream.get()) }
    }

    #[inline(always)]
    fn get_msg_buf(&self) -> &mut Vec<u8> {
        unsafe { std::mem::transmute(self.msg_buf.get()) }
    }

    #[inline(always)]
    fn get_action_buf(&self) -> &mut Vec<u8> {
        unsafe { std::mem::transmute(self.action_buf.get()) }
    }
}

impl<F: ServerFactory> fmt::Debug for QuicServer<F> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "QuicServer")
    }
}

impl<F: ServerFactory> ServerTransport<F> for QuicServer<F>
where
    <F as ServerFactory>::IO: std::fmt::Debug,
{
    type Listener = QuicListener<F::IO>;

    fn new_conn(stream: QuicStream, factory: &F, conn_count: Arc<()>) -> Self {
        let config = factory.get_config();
        let logger = factory.new_logger();
        let mut buf_size = config.stream_buf_size;
        if buf_size == 0 {
            buf_size = SERVER_DEFAULT_BUF_SIZE;
        }
        Self {
            stream: UnsafeCell::new(AsyncBufStream::new(stream, buf_size)),
            config: config.clone(),
            action_buf: UnsafeCell::new(Vec::with_capacity(128)),
            msg_buf: UnsafeCell::new(Vec::with_capacity(512)),
            _conn_count: conn_count,
            logger,
            _io: std::marker::PhantomData,
        }
    }

    #[inline(always)]
    fn get_logger(&self) -> &F::Logger {
        &self.logger
    }

    async fn read_req<'a>(
        &'a self, close_ch: &crossfire::MAsyncRx<()>,
    ) -> Result<RpcSvrReq<'a>, RpcError> {
        let reader = self.get_stream_mut();
        let idle_timeout = self.config.idle_timeout;
        let mut req_header_buf = [0u8; proto::RPC_REQ_HEADER_LEN];

        let cancel_f = F::IO::sleep(idle_timeout);
        // TODO: Cancellable doesn't work well with the new crossfire receiver.
        // For now, we just read with a timeout.
        match F::IO::timeout(idle_timeout, reader.read_exact(&mut req_header_buf)).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                logger_debug!(self.logger, "{:?}: recv_req: err {}", self, e);
                return Err(RPC_ERR_COMM);
            }
            Err(_) => {
                logger_trace!(self.logger, "{:?}: read timeout", self);
                return Err(RPC_ERR_TIMEOUT);
            }
        }

        let rpc_head = proto::ReqHead::decode_head(&req_header_buf).map_err(|_| RPC_ERR_DECODE)?;
        logger_trace!(self.logger, "{:?}: recv req: {}", self, rpc_head);

        let action = match rpc_head.get_action() {
            Ok(num) => RpcAction::Num(num),
            Err(action_len) => {
                let action_buf = self.get_action_buf();
                action_buf.resize(action_len as usize, 0);
                reader.read_exact(action_buf).await.map_err(|_| RPC_ERR_COMM)?;
                match std::str::from_utf8(action_buf) {
                    Ok(s) => RpcAction::Str(s),
                    Err(_) => {
                        error!("{:?}: read action string decode error", self);
                        return Err(RPC_ERR_DECODE);
                    }
                }
            }
        };

        let msg_buf = self.get_msg_buf();
        msg_buf.resize(rpc_head.msg_len as usize, 0);
        if rpc_head.msg_len > 0 {
            reader.read_exact(msg_buf).await.map_err(|_| RPC_ERR_COMM)?;
        }

        let mut blob: Option<Buffer> = None;
        if rpc_head.blob_len > 0 {
            let mut ext_buf =
                Buffer::alloc(rpc_head.blob_len as i32).map_err(|_| RPC_ERR_ENCODE)?;
            reader.read_exact(&mut ext_buf).await.map_err(|_| RPC_ERR_COMM)?;
            blob = Some(ext_buf);
        }

        return Ok(RpcSvrReq::<'a> { seq: rpc_head.seq, action, msg: msg_buf, blob });
    }

    #[inline]
    async fn write_resp<'a>(
        &self, seq: u64, res: Result<(Vec<u8>, Option<&'a Buffer>), &'a RpcError>,
    ) -> io::Result<()> {
        let writer = self.get_stream_mut();
        match res {
            Err(e) => {
                let (header, err_str) = proto::RespHead::encode_err(seq, e);
                writer.write_all(header.as_bytes()).await?;
                if let Some(s) = err_str.as_ref() {
                    writer.write_all(s).await?;
                }
                logger_trace!(self.logger, "{:?}: send resp: {}", self, header);
            }
            Ok((msg, blob_buf)) => {
                let header = proto::RespHead::encode_msg(seq, &msg, blob_buf);
                writer.write_all(header.as_bytes()).await?;
                if msg.len() > 0 {
                    writer.write_all(msg.as_bytes()).await?;
                }
                if let Some(blob) = blob_buf.as_ref() {
                    writer.write_all(blob.as_bytes()).await?;
                }
                logger_trace!(self.logger, "{:?}: send resp: {}", self, header);
            }
        }
        return Ok(());
    }

    #[inline(always)]
    async fn flush_resp(&self) -> io::Result<()> {
        let writer = self.get_stream_mut();
        writer.flush().await
    }

    #[inline]
    async fn close_conn(&self) {
        // The connection will be closed when the handler is dropped.
    }
}
