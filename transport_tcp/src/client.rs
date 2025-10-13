use crate::net::{UnifyAddr, UnifyStream};
use bytes::BytesMut;
use crossfire::MAsyncRx;
use io_buffer::Buffer;
use occams_rpc_core::io::{AsyncRead, AsyncWrite, Cancellable, io_with_timeout};
use occams_rpc_core::runtime::AsyncIO;
use occams_rpc_core::{ClientConfig, error::*};
use occams_rpc_stream::client::{ClientFactory, ClientTaskDecode, ClientTaskDone, ClientTransport};
use occams_rpc_stream::client_timer::ClientTaskTimer;
use occams_rpc_stream::proto;
use std::cell::UnsafeCell;
use std::mem::transmute;
use std::str::FromStr;
use std::time::Duration;
use std::{fmt, io};

pub struct TcpClient<F: ClientFactory> {
    stream: UnsafeCell<UnifyStream<F::IO>>,
    resp_buf: UnsafeCell<BytesMut>,
    logger: F::Logger,
    server_id: u64,
    client_id: u64,
    read_timeout: Duration,
    write_timeout: Duration,
}

unsafe impl<F: ClientFactory> Send for TcpClient<F> {}
unsafe impl<F: ClientFactory> Sync for TcpClient<F> {}

impl<F: ClientFactory> fmt::Debug for TcpClient<F> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rpc client {}:{}", self.server_id, self.client_id)
    }
}

impl<F: ClientFactory> TcpClient<F> {
    // Because async runtimes does not support splitting read and write to static handler,
    // we use unsafe to achieve such goal,
    #[inline(always)]
    fn get_stream_mut(&self) -> &mut UnifyStream<F::IO> {
        unsafe { std::mem::transmute(self.stream.get()) }
    }

    #[inline(always)]
    fn get_resp_buf(&self, len: usize) -> &mut BytesMut {
        let buf: &mut BytesMut = unsafe { transmute(self.resp_buf.get()) };
        buf.resize(len as usize, 0);
        buf
    }

    async fn _recv_and_dump(&self, l: usize) -> Result<(), RpcError> {
        let reader = self.get_stream_mut();
        // TODO is there dump ?
        match Buffer::alloc(l as i32) {
            Err(_) => {
                logger_warn!(self.logger, "{:?} alloc buf failed", self);
                return Err(RPC_ERR_COMM);
            }
            Ok(mut buf) => {
                if let Err(e) =
                    io_with_timeout!(F::IO, self.read_timeout, reader.read_exact(&mut buf))
                {
                    logger_warn!(self.logger, "{:?} recv task failed: {:?}", self, e);
                    return Err(RPC_ERR_COMM);
                }
                return Ok(());
            }
        }
    }

    #[inline]
    async fn _recv_error(
        &self, factory: &F, resp_head: &proto::RespHead, task: F::Task,
    ) -> Result<(), RpcError> {
        log_debug_assert!(resp_head.flag > 0);
        let reader = self.get_stream_mut();
        match resp_head.flag {
            1 => {
                let rpc_err = RpcError::Num(resp_head.msg_len as u32);
                //if self.should_close(err_no) {
                //    (self, task, rpc_err);
                //    return Err(RPC_ERR_COMM);
                //} else {
                factory.error_handle(task, rpc_err);
                return Ok(());
            }
            2 => {
                let buf = self.get_resp_buf(resp_head.blob_len as usize);
                match io_with_timeout!(F::IO, self.read_timeout, reader.read_exact(buf)) {
                    Err(e) => {
                        logger_warn!(self.logger, "{:?} recv buffer error: {:?}", self, e);
                        factory.error_handle(task, RPC_ERR_COMM);
                        return Err(RPC_ERR_COMM);
                    }
                    Ok(_) => {
                        match str::from_utf8(buf) {
                            Err(_) => {
                                logger_error!(
                                    self.logger,
                                    "{:?} recv task {:?} err string invalid",
                                    self,
                                    task
                                );
                                factory.error_handle(task, RPC_ERR_DECODE);
                            }
                            Ok(s) => {
                                let rpc_err = RpcError::Text(s.to_string());
                                factory.error_handle(task, rpc_err);
                            }
                        }
                        return Ok(());
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    #[inline]
    async fn _recv_resp_body(
        &self, factory: &F, codec: &F::Codec, task_reg: &mut ClientTaskTimer<F>,
        resp_head: &proto::RespHead,
    ) -> Result<(), RpcError> {
        let reader = self.get_stream_mut();
        let read_timeout = self.read_timeout;
        let blob_len = resp_head.blob_len;
        let read_buf = self.get_resp_buf(resp_head.msg_len as usize);
        if let Some(mut task_item) = task_reg.take_task(resp_head.seq).await {
            let mut task = task_item.task.take().unwrap();
            if resp_head.flag > 0 {
                return self._recv_error(factory, resp_head, task).await;
            }
            if resp_head.msg_len > 0 {
                if let Err(_) = io_with_timeout!(F::IO, read_timeout, reader.read_exact(read_buf)) {
                    factory.error_handle(task, RPC_ERR_COMM);
                    return Err(RPC_ERR_COMM);
                }
            } // When msg_len == 0, read_buf has 0 size

            if blob_len > 0 {
                match task.reserve_resp_blob(blob_len) {
                    None => {
                        logger_error!(
                            self.logger,
                            "{:?} rpc client task {:?} has no ext_buf",
                            self,
                            task,
                        );
                        task.set_result(Err(RPC_ERR_DECODE));
                        return self._recv_and_dump(blob_len as usize).await;
                    }
                    Some(buf) => {
                        // ensure buf can fit blob_len
                        if let Err(e) =
                            io_with_timeout!(F::IO, read_timeout, reader.read_exact(buf))
                        {
                            logger_warn!(
                                self.logger,
                                "{:?} rpc client reader read ext_buf err: {:?}",
                                self,
                                e
                            );
                            factory.error_handle(task, RPC_ERR_COMM);
                            return Err(RPC_ERR_COMM);
                        }
                    }
                }
            }
            logger_trace!(self.logger, "{:?} recv task {:?} ok", self, task);
            if resp_head.msg_len > 0 {
                // set result of task, and notify task completed
                if let Err(_) = task.decode_resp(codec, read_buf) {
                    logger_warn!(self.logger, "{:?} rpc client reader decode resp err", self,);
                    task.set_result(Err(RPC_ERR_DECODE));
                } else {
                    task.set_result(Ok(()));
                }
            } else {
                task.set_result(Ok(()));
            }
            return Ok(());
        } else {
            let seq = resp_head.seq;
            logger_trace!(self.logger, "{:?} timer take_task(seq={}) return None", self, seq);
            let mut data_len = 0;
            if resp_head.flag == 0 {
                data_len += resp_head.msg_len + resp_head.blob_len as u32;
            } else if resp_head.flag == proto::RESP_FLAG_HAS_ERR_STRING {
                data_len += resp_head.blob_len as u32;
            }
            if data_len > 0 {
                return self._recv_and_dump(data_len as usize).await;
            } else {
                return Ok(());
            }
        }
    }
}

impl<F: ClientFactory> ClientTransport<F> for TcpClient<F> {
    async fn connect(
        addr: &str, config: &ClientConfig, client_id: u64, server_id: u64, logger: F::Logger,
    ) -> Result<Self, RpcError> {
        let connect_timeout = config.connect_timeout;
        let stream: UnifyStream<F::IO> = {
            match UnifyAddr::from_str(addr) {
                Err(e) => {
                    error!("Cannot parsing addr {}: {:?}", addr, e);
                    return Err(RPC_ERR_CONNECT);
                }
                Ok(UnifyAddr::Socket(_addr)) => {
                    match F::IO::connect_tcp(&_addr, connect_timeout).await {
                        Ok(stream) => UnifyStream::Tcp(stream),
                        Err(e) => {
                            warn!("Cannot connect addr {}: {:?}", addr, e);
                            return Err(RPC_ERR_CONNECT);
                        }
                    }
                }
                Ok(UnifyAddr::Path(_addr)) => {
                    match F::IO::connect_unix(&_addr, connect_timeout).await {
                        Ok(stream) => UnifyStream::Unix(stream),
                        Err(e) => {
                            warn!("Cannot connect addr {}: {:?}", addr, e);
                            return Err(RPC_ERR_CONNECT);
                        }
                    }
                }
            }
        };
        Ok(Self {
            stream: UnsafeCell::new(stream),
            resp_buf: UnsafeCell::new(BytesMut::with_capacity(512)),
            server_id,
            client_id,
            write_timeout: config.write_timeout,
            read_timeout: config.read_timeout,
            logger,
        })
    }

    #[inline(always)]
    fn get_logger(&self) -> &F::Logger {
        &self.logger
    }

    #[inline(always)]
    async fn close_conn(&self) {
        let stream = self.get_stream_mut();
        // stream close is just shutdown on sending, receiver might not be notified on peer dead
        let _ = stream.shutdown_write().await;
    }

    #[inline(always)]
    async fn flush_req(&self) -> Result<(), RpcError> {
        return Ok(());
        //        let writer = self.get_stream_mut();
        //        let r = writer.flush_timeout(self.write_timeout).await;
        //        if r.is_err() {
        //            logger_warn!(self.logger, "{:?} flush_req flush err: {:?}", self, r);
        //            return Err(RPC_ERR_COMM);
        //        }
        //        Ok(())
    }

    #[inline(always)]
    async fn write_req<'a>(
        &'a self, need_flush: bool, header: &'a [u8], action_str: Option<&'a [u8]>,
        msg_buf: &'a [u8], blob: Option<&'a [u8]>,
    ) -> io::Result<()> {
        let writer = self.get_stream_mut();
        let mut data_len = header.len();
        let write_timeout = self.write_timeout;
        io_with_timeout!(F::IO, write_timeout, writer.write_all(header))?;
        if let Some(action_s) = action_str {
            data_len += action_s.len();
            io_with_timeout!(F::IO, write_timeout, writer.write_all(action_s))?;
        }
        if msg_buf.len() > 0 {
            data_len += msg_buf.len();
            io_with_timeout!(F::IO, write_timeout, writer.write_all(msg_buf))?;
        }
        if let Some(blob_buf) = blob {
            data_len += blob_buf.len();
            io_with_timeout!(F::IO, write_timeout, writer.write_all(blob_buf))?;
        }
        // TODO change to buffer writer
        //if need_flush || data_len >= 32 * 1024 {
        //    writer.flush_timeout(self.write_timeout).await?;
        //}
        return Ok(());
    }

    /// return false to indicate aborted by close_f
    #[inline]
    async fn read_resp(
        &self, factory: &F, codec: &F::Codec, close_ch: Option<&MAsyncRx<()>>,
        task_reg: &mut ClientTaskTimer<F>,
    ) -> Result<bool, RpcError> {
        let mut resp_head_buf = [0u8; proto::RPC_RESP_HEADER_LEN];
        let reader = self.get_stream_mut();
        if let Some(close_ch) = close_ch {
            let read_header_f = reader.read_exact(&mut resp_head_buf);
            let close_f = close_ch.recv();
            let res = Cancellable::new(read_header_f, close_f).await;
            match res {
                Ok(r) => {
                    if let Err(_e) = r {
                        logger_debug!(
                            self.logger,
                            "{:?} rpc client read resp head err: {:?}",
                            self,
                            _e
                        );
                        return Err(RPC_ERR_COMM);
                    }
                }
                Err(_) => {
                    return Ok(false);
                }
            }
        } else {
            if let Err(e) =
                io_with_timeout!(F::IO, self.read_timeout, reader.read_exact(&mut resp_head_buf))
            {
                logger_debug!(self.logger, "{:?} rpc client read resp head err: {:?}", self, e);
                return Err(RPC_ERR_COMM);
            }
        }
        match proto::RespHead::decode_head(&resp_head_buf) {
            Err(_e) => {
                logger_debug!(
                    self.logger,
                    "{:?} rpc client decode_response_header err: {:?}",
                    self,
                    _e
                );
                return Err(RPC_ERR_COMM);
            }
            Ok(head) => {
                logger_trace!(self.logger, "{:?} rpc client read head response {}", self, &head);
                self._recv_resp_body(factory, codec, task_reg, &head).await?;
                return Ok(true);
            }
        }
    }
}
