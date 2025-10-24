use occams_rpc_core::io::{AsyncBufStream, AsyncRead, AsyncWrite, Cancellable, io_with_timeout};
use occams_rpc_core::runtime::AsyncIO;
use occams_rpc_core::{Codec, ServerConfig, error::*};
use occams_rpc_stream::server::{RpcSvrReq, ServerFacts, ServerTransport, task::ServerTaskEncode};
use occams_rpc_stream::{proto, proto::RpcAction};

use crate::net::{UnifyListener, UnifyStream};
use io_buffer::Buffer;
use std::cell::UnsafeCell;
use std::mem::transmute;
use std::sync::Arc;
use std::time::Duration;
use std::{fmt, io};

pub const SERVER_DEFAULT_BUF_SIZE: usize = 8 * 1024;

pub struct TcpServer<IO: AsyncIO> {
    stream: UnsafeCell<AsyncBufStream<UnifyStream<IO>>>,
    _conn_count: Arc<()>,
    config: ServerConfig,
    /// for read
    action_buf: UnsafeCell<Vec<u8>>,
    /// for read
    msg_buf: UnsafeCell<Vec<u8>>,
    /// for write
    encode_buf: UnsafeCell<Vec<u8>>,
}

unsafe impl<IO: AsyncIO> Send for TcpServer<IO> {}

unsafe impl<IO: AsyncIO> Sync for TcpServer<IO> {}

impl<IO: AsyncIO> TcpServer<IO> {
    // Because async runtimes does not support splitting read and write to static handler,
    // we use unsafe to achieve such goal,
    #[inline(always)]
    fn get_stream_mut(&self) -> &mut AsyncBufStream<UnifyStream<IO>> {
        unsafe { transmute(self.stream.get()) }
    }

    #[inline(always)]
    fn get_msg_buf(&self) -> &mut Vec<u8> {
        unsafe { transmute(self.msg_buf.get()) }
    }

    #[inline(always)]
    fn get_action_buf(&self) -> &mut Vec<u8> {
        unsafe { transmute(self.action_buf.get()) }
    }

    #[inline(always)]
    fn get_encode_buf(&self) -> &mut Vec<u8> {
        unsafe { transmute(self.encode_buf.get()) }
    }
}

impl<IO: AsyncIO> fmt::Debug for TcpServer<IO> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.get_stream_mut().fmt(f)
    }
}

impl<IO: AsyncIO> ServerTransport<IO> for TcpServer<IO> {
    type Listener = UnifyListener<IO>;

    fn new_conn(stream: UnifyStream<IO>, config: &ServerConfig, conn_count: Arc<()>) -> Self {
        let mut buf_size = config.stream_buf_size;
        if buf_size == 0 {
            buf_size = SERVER_DEFAULT_BUF_SIZE;
        }
        Self {
            stream: UnsafeCell::new(AsyncBufStream::new(stream, buf_size)),
            config: config.clone(),
            action_buf: UnsafeCell::new(Vec::with_capacity(128)),
            msg_buf: UnsafeCell::new(Vec::with_capacity(512)),
            // TODO add const assert with RPC_RESP_HEADER_LEN
            encode_buf: UnsafeCell::new(Vec::with_capacity(512)),
            _conn_count: conn_count,
        }
    }

    /// recv_req and return a temporary structure.
    ///
    /// NOTE: you should consume the buffer ref before recv another request.
    async fn read_req<'a, F: ServerFacts>(
        &'a self, logger: &F::Logger, close_ch: &crossfire::MAsyncRx<()>,
    ) -> Result<RpcSvrReq<'a>, RpcIntErr> {
        let reader = self.get_stream_mut();
        let read_timeout = self.config.read_timeout;
        let idle_timeout = self.config.idle_timeout;
        let mut req_header_buf = [0u8; proto::RPC_REQ_HEADER_LEN];

        let cancel_f = close_ch.recv_with_timer(IO::sleep(idle_timeout));
        match Cancellable::new(reader.read_exact(&mut req_header_buf), cancel_f).await {
            Ok(Err(e)) => {
                logger_debug!(logger, "{:?}: recv_req: err {}", self, e);
                return Err(RpcIntErr::IO);
            }
            Err(()) => {
                logger_trace!(logger, "{:?}: read timeout", self);
                return Err(RpcIntErr::Timeout);
            }
            _ => {}
        }
        let rpc_head: &proto::ReqHead;
        match proto::ReqHead::decode_head(&req_header_buf) {
            Err(e) => {
                logger_warn!(logger, "{:?}: decode_head error, {}", self, e);
                return Err(RpcIntErr::Decode);
            }
            Ok(head) => {
                rpc_head = head;
            }
        }
        logger_trace!(logger, "{:?}: recv req: {}", self, rpc_head);
        // XXX: we do return ping
        let action = match rpc_head.get_action() {
            Ok(num) => RpcAction::Num(num),
            Err(action_len) => {
                let action_buf = self.get_action_buf();
                action_buf.resize(action_len as usize, 0);
                match io_with_timeout!(IO, read_timeout, reader.read_exact(action_buf)) {
                    Err(e) => {
                        logger_trace!(logger, "{:?}: read_exact error {}", self, e);
                        return Err(RpcIntErr::IO);
                    }
                    Ok(_) => {
                        match std::str::from_utf8(action_buf) {
                            Ok(s) => RpcAction::Str(s),
                            Err(_) => {
                                error!("{:?}: read action string decode error", self);
                                return Err(RpcIntErr::Decode);
                                // XXX stop reading or consume junk data?
                            }
                        }
                    }
                }
            }
        };

        let msg_buf = self.get_msg_buf();
        msg_buf.resize(rpc_head.msg_len.get() as usize, 0);
        if rpc_head.msg_len > 0 {
            if let Err(e) = io_with_timeout!(IO, read_timeout, reader.read_exact(msg_buf)) {
                logger_trace!(logger, "{:?}: read req msg error: {:?}", self, e);
                return Err(RpcIntErr::IO);
            }
        }
        let mut blob: Option<Buffer> = None;
        let blob_len = rpc_head.blob_len.get() as i32;
        if blob_len > 0 {
            match Buffer::alloc(blob_len) {
                Err(_) => return Err(RpcIntErr::Decode),
                Ok(mut ext_buf) => {
                    match io_with_timeout!(IO, read_timeout, reader.read_exact(&mut ext_buf)) {
                        Err(e) => {
                            logger_trace!(logger, "{:?}: read_exact_buffer error: {}", self, e);
                            return Err(RpcIntErr::IO);
                        }
                        Ok(_) => {
                            blob = Some(ext_buf);
                        }
                    }
                }
            }
        }
        return Ok(RpcSvrReq::<'a> { seq: rpc_head.seq.get(), action, msg: msg_buf, blob });
    }

    #[inline]
    async fn write_resp<F: ServerFacts, T: ServerTaskEncode>(
        &self, logger: &F::Logger, codec: &impl Codec, mut task: T,
    ) -> io::Result<()> {
        let writer = self.get_stream_mut();
        let write_timeout = self.config.write_timeout;
        let buf = self.get_encode_buf();
        let (seq, blob_buf) = proto::RespHead::encode(&logger, codec, buf, &mut task);
        if let Err(e) = io_with_timeout!(IO, write_timeout, writer.write_all(buf)) {
            logger_warn!(logger, "{:?}: send_resp write resp seq={} msg err: {}", self, seq, e);
            return Err(e);
        }
        if let Some(blob) = blob_buf {
            if let Err(e) = io_with_timeout!(IO, write_timeout, writer.write_all(blob.as_ref())) {
                logger_debug!(
                    logger,
                    "{:?}: send_resp write resp seq={} blob err: {}",
                    self,
                    seq,
                    e
                );
                return Err(e);
            }
        }
        logger_trace!(logger, "{:?}: send resp seq={}", self, seq);
        return Ok(());
    }

    #[inline(always)]
    async fn write_resp_internal<F: ServerFacts>(
        &self, logger: &F::Logger, seq: u64, err: Option<RpcIntErr>,
    ) -> io::Result<()> {
        let writer = self.get_stream_mut();
        let write_timeout = self.config.write_timeout;
        let buf = self.get_encode_buf();
        let seq = proto::RespHead::encode_internal(&logger, buf, seq, err);
        if let Err(e) = io_with_timeout!(IO, write_timeout, writer.write_all(buf)) {
            logger_warn!(logger, "{:?}: send_resp write resp seq={} msg err: {}", self, seq, e);
            return Err(e);
        }
        logger_trace!(logger, "{:?}: send resp seq={}", self, seq);
        return Ok(());
    }

    #[inline(always)]
    async fn flush_resp<F: ServerFacts>(&self, logger: &F::Logger) -> io::Result<()> {
        let writer = self.get_stream_mut();
        if let Err(e) = io_with_timeout!(IO, self.config.write_timeout, writer.flush()) {
            logger_warn!(logger, "{:?}: flush err: {}", self, e);
            return Err(e);
        }
        logger_trace!(logger, "{:?}: flush_resp ok", self);
        return Ok(());
    }

    #[inline]
    async fn close_conn<F: ServerFacts>(&self, logger: &F::Logger) {
        if self.flush_resp::<F>(logger).await.is_ok() {
            let writer = self.get_stream_mut();
            let _ = writer.get_inner().shutdown_write().await;
        }
    }
}
