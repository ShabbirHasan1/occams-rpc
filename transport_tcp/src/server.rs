use futures::future::FutureExt;
use occams_rpc::config::TimeoutSetting;
use occams_rpc::error::*;
use occams_rpc::io::{AsyncRead, AsyncWrite, Cancellable, io_with_timeout};
use occams_rpc::runtime::AsyncIO;
use occams_rpc::stream::RpcAction;
use occams_rpc::stream::proto;
use occams_rpc::stream::server::{RpcSvrReq, ServerFactory, ServerTransport};

use crate::net::{UnifyListener, UnifyStream};
use bytes::BytesMut;
use io_buffer::Buffer;
use std::cell::UnsafeCell;
use std::sync::Arc;
use std::time::Duration;
use std::{fmt, io};
use zerocopy::AsBytes;

pub struct TcpServer<F: ServerFactory> {
    stream: UnsafeCell<UnifyStream<F::IO>>,
    _conn_count: Arc<()>,
    timeout: TimeoutSetting,
    action_buf: UnsafeCell<BytesMut>,
    msg_buf: UnsafeCell<BytesMut>,
    logger: F::Logger,
}

unsafe impl<F: ServerFactory> Send for TcpServer<F> {}

unsafe impl<F: ServerFactory> Sync for TcpServer<F> {}

impl<F: ServerFactory> TcpServer<F> {
    // Because async runtimes does not support spliting read and write to static handler,
    // we use unsafe to achieve such goal,
    #[inline(always)]
    fn get_stream_mut(&self) -> &mut UnifyStream<F::IO> {
        unsafe { std::mem::transmute(self.stream.get()) }
    }

    #[inline(always)]
    fn get_msg_buf(&self) -> &mut BytesMut {
        unsafe { std::mem::transmute(self.msg_buf.get()) }
    }

    #[inline(always)]
    fn get_action_buf(&self) -> &mut BytesMut {
        unsafe { std::mem::transmute(self.action_buf.get()) }
    }
}

impl<F: ServerFactory> fmt::Debug for TcpServer<F> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.stream.fmt(f)
    }
}

impl<F: ServerFactory> ServerTransport<F> for TcpServer<F> {
    type Listener = UnifyListener<F::IO>;

    fn new_conn(stream: UnifyStream<F::IO>, factory: &F, conn_count: Arc<()>) -> Self {
        let config = factory.get_config();
        let logger = factory.new_logger();
        Self {
            stream: UnsafeCell::new(stream),
            timeout: config.timeout.clone(),
            action_buf: UnsafeCell::new(BytesMut::with_capacity(128)),
            msg_buf: UnsafeCell::new(BytesMut::with_capacity(512)),
            _conn_count: conn_count,
            logger,
        }
    }

    #[inline(always)]
    fn get_logger(&self) -> &F::Logger {
        &self.logger
    }

    /// recv_req and return a temporary structure.
    ///
    /// NOTE: you should consume the buffer ref before recv another request.
    async fn recv_req<'a>(
        &'a self, close_ch: &crossfire::MAsyncRx<()>,
    ) -> Result<RpcSvrReq<'a>, RpcError> {
        let reader = self.get_stream_mut();
        let read_timeout = self.timeout.read_timeout;
        let idle_timeout = self.timeout.idle_timeout;
        let mut req_header_buf = [0u8; proto::RPC_REQ_HEADER_LEN];

        let cancel_f = async {
            let sleep_f = F::IO::sleep(idle_timeout);
            let close_f = close_ch.recv();
            futures::select! {
                _=sleep_f.fuse()=>{},
                _=close_f.fuse()=>{}
            }
        };
        match Cancellable::new(reader.read_exact(&mut req_header_buf), cancel_f).await {
            Ok(Err(e)) => {
                logger_debug!(self.logger, "{:?}: recv_req: err {:?}", self, e);
                return Err(RPC_ERR_COMM);
            }
            Err(()) => {
                logger_trace!(self.logger, "{:?}: read timeout", self);
                return Err(RPC_ERR_TIMEOUT);
            }
            _ => {}
        }
        let rpc_head: &proto::ReqHead;
        match proto::ReqHead::decode_head(&req_header_buf) {
            Err(e) => {
                logger_warn!(self.logger, "{:?}: decode_head error, {:?}", self, e);
                return Err(RPC_ERR_COMM);
            }
            Ok(head) => {
                rpc_head = head;
            }
        }
        logger_trace!(self.logger, "{:?}: recv req: {}", self, rpc_head);
        // XXX: we do return ping
        let action = match rpc_head.get_action() {
            Ok(num) => RpcAction::Num(num),
            Err(action_len) => {
                let action_buf = self.get_action_buf();
                action_buf.resize(action_len as usize, 0);
                match io_with_timeout!(F::IO, read_timeout, reader.read_exact(action_buf)) {
                    Err(_) => {
                        logger_trace!(self.logger, "{:?}: read_exact error", self);
                        return Err(RPC_ERR_COMM);
                    }
                    Ok(_) => {
                        match std::str::from_utf8(action_buf) {
                            Ok(s) => RpcAction::Str(s),
                            Err(_) => {
                                error!("{:?}: read action string decode error", self);
                                return Err(RPC_ERR_DECODE);
                                // XXX stop reading or consume junk data?
                            }
                        }
                    }
                }
            }
        };

        let msg_buf = self.get_msg_buf();
        msg_buf.resize(rpc_head.msg_len as usize, 0);
        if rpc_head.msg_len > 0 {
            if let Err(e) = io_with_timeout!(F::IO, read_timeout, reader.read_exact(msg_buf)) {
                logger_trace!(self.logger, "{:?}: read req msg error: {:?}", self, e);
                return Err(RPC_ERR_COMM);
            }
        }
        let mut blob: Option<Buffer> = None;
        if rpc_head.blob_len > 0 {
            match Buffer::alloc(rpc_head.blob_len as i32) {
                Err(_) => return Err(RPC_ERR_ENCODE),
                Ok(mut ext_buf) => {
                    match io_with_timeout!(F::IO, read_timeout, reader.read_exact(&mut ext_buf)) {
                        Err(_) => {
                            logger_trace!(self.logger, "{:?}: read_exact_buffer error", self);
                            return Err(RPC_ERR_COMM);
                        }
                        Ok(_) => {
                            blob = Some(ext_buf);
                        }
                    }
                }
            }
        }
        return Ok(RpcSvrReq::<'a> { seq: rpc_head.seq, action, msg: msg_buf, blob });
    }

    #[inline]
    async fn send_resp<'a>(
        &self, seq: u64, res: Result<(Vec<u8>, Option<&'a Buffer>), &'a RpcError>,
    ) -> io::Result<()> {
        let writer = self.get_stream_mut();
        let write_timeout = self.timeout.write_timeout;
        match res {
            Err(e) => {
                let (header, err_str) = proto::RespHead::encode_err(seq, e);
                if let Err(e) =
                    io_with_timeout!(F::IO, write_timeout, writer.write_all(header.as_bytes()))
                {
                    logger_warn!(
                        self.logger,
                        "{:?}: send_resp write resp header err: {:?}",
                        self,
                        e
                    );
                    return Err(e);
                }
                if let Some(s) = err_str.as_ref() {
                    if let Err(e) = io_with_timeout!(F::IO, write_timeout, writer.write_all(s)) {
                        logger_debug!(
                            self.logger,
                            "{:?}: send_resp write resp blob err: {:?}",
                            self,
                            e
                        );
                        return Err(e);
                    }
                }
                logger_trace!(self.logger, "{:?}: send resp: {}", self, header);
            }
            Ok((msg, blob_buf)) => {
                let header = proto::RespHead::encode_msg(seq, &msg, blob_buf);
                if let Err(e) =
                    io_with_timeout!(F::IO, write_timeout, writer.write_all(header.as_bytes()))
                {
                    logger_warn!(
                        self.logger,
                        "{:?}: send_resp write resp header err: {:?}",
                        self,
                        e
                    );
                    return Err(e);
                }
                if msg.len() > 0 {
                    if let Err(e) =
                        io_with_timeout!(F::IO, write_timeout, writer.write_all(msg.as_bytes()))
                    {
                        logger_debug!(
                            self.logger,
                            "{:?}: send_resp write resp msg err: {:?}",
                            self,
                            e
                        );
                        return Err(e);
                    }
                }
                if let Some(blob) = blob_buf.as_ref() {
                    if let Err(e) =
                        io_with_timeout!(F::IO, write_timeout, writer.write_all(blob.as_bytes()))
                    {
                        logger_debug!(
                            self.logger,
                            "{:?}: send_resp write resp blob err: {:?}",
                            self,
                            e
                        );
                        return Err(e);
                    }
                }
                logger_trace!(self.logger, "{:?}: send resp: {}", self, header);
            }
        }
        return Ok(());
    }

    #[inline(always)]
    async fn flush_resp(&self) -> io::Result<()> {
        //let writer = self.get_stream_mut();
        //let r = writer.flush_timeout(write_timeout).await;
        //if r.is_err() {
        //    logger_debug!(inner.logger, "{}: flush err: {:?}", self, r);
        //    return Err(RPC_ERR_COMM);
        //}
        //logger_trace!(inner.logger, "{}: send_resp flushed", self);
        return Ok(());
    }

    #[inline]
    async fn close_conn(&self) {
        let writer = self.get_stream_mut();
        let _ = writer.shutdown_write().await;
    }
}
