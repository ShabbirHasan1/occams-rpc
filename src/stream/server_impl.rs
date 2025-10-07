use super::server::*;
use super::*;
use crate::codec::Codec;
use crate::error::*;
use crate::io::AsyncListener;
use crate::runtime::AsyncIO;
use futures::{
    FutureExt,
    future::{AbortHandle, Abortable},
};
use io_buffer::Buffer;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct RpcServer<F>
where
    F: ServerFactory,
{
    listeners_abort: Vec<(AbortHandle, String)>,
    logger: F::Logger,
    factory: Arc<F>,
    conn_ref_count: Arc<()>,
    server_close_tx: Mutex<Option<crossfire::MTx<()>>>,
    server_close_rx: crossfire::MAsyncRx<()>,
}

impl<F> RpcServer<F>
where
    F: ServerFactory,
{
    pub fn new(factory: Arc<F>) -> Self {
        let (tx, rx) = crossfire::mpmc::unbounded_async();
        Self {
            listeners_abort: Vec::new(),
            logger: factory.new_logger(),
            factory,
            conn_ref_count: Arc::new(()),
            server_close_tx: Mutex::new(Some(tx)),
            server_close_rx: rx,
        }
    }

    pub fn listen(&mut self, addr: &str) -> io::Result<()> {
        match <<F::Transport as ServerTransport<F>>::Listener as AsyncListener>::bind(addr) {
            Err(e) => {
                error!("bind addr {:?} err: {:?}", addr, e);
                return Err(e);
            }
            Ok(mut listener) => {
                let (abort_handle, abort_registration) = AbortHandle::new_pair();
                let factory = self.factory.clone();
                let conn_ref_count = self.conn_ref_count.clone();
                let listener_info = format!("listener {:?}", addr);
                let server_close_rx = self.server_close_rx.clone();
                let abrt = Abortable::new(
                    async move {
                        debug!("listening on {:?}", listener);
                        loop {
                            match listener.accept().await {
                                Err(e) => {
                                    warn!("{:?} accept error: {}", listener, e);
                                    return;
                                }
                                Ok(stream) => {
                                    let conn = F::Transport::new_conn(
                                        stream,
                                        &factory,
                                        conn_ref_count.clone(),
                                    );
                                    <F::ConnHandle as ServerHandle<F>>::run(
                                        conn,
                                        &factory,
                                        server_close_rx.clone(),
                                    )
                                }
                            }
                        }
                    },
                    abort_registration,
                )
                .map(|x| match x {
                    Ok(_) => {}
                    Err(e) => {
                        warn!("rpc server exit listening: {:?}", e);
                    }
                });
                self.factory.spawn_detach(abrt);
                self.listeners_abort.push((abort_handle, listener_info));
                return Ok(());
            }
        }
    }

    #[inline]
    fn get_alive_conn(&self) -> usize {
        Arc::strong_count(&self.conn_ref_count) - 1
    }

    pub async fn close(&mut self) {
        // close listeners
        for h in &self.listeners_abort {
            h.0.abort();
            logger_info!(self.logger, "{} has closed", h.1);
        }
        // Notify all reader connection exit
        let _ = self.server_close_tx.lock().unwrap().take();

        let mut exists_count = self.get_alive_conn();
        // wait client close all connections
        let mut close_timeout = 0;
        while exists_count > 0 {
            close_timeout += 1;
            <F::IO as AsyncIO>::sleep(Duration::from_secs(1)).await;
            exists_count = self.get_alive_conn();
            if close_timeout > 90 {
                logger_warn!(
                    self.logger,
                    "closed as wait too long for all conn closed voluntarily({} conn left)",
                    exists_count,
                );
                break;
            }
        }
        logger_info!(self.logger, "server closed with alive conn {}", exists_count);
    }
}

/// This ServerHandle impl two coroutines, one to read task, and one to write task.
///
/// When task is done, it will dispatched back to writer through a channel. The task may be an enum that impl RpcServerTaskResp.
pub struct ServerHandleTaskStream();

impl<F: ServerFactory> ServerHandle<F> for ServerHandleTaskStream {
    fn run(conn: F::Transport, factory: &F, server_close_rx: crossfire::MAsyncRx<()>) {
        let conn = Arc::new(conn);

        let dispatch = Arc::new(factory.new_dispatcher());
        let (done_tx, done_rx) = crossfire::mpsc::unbounded_async();

        let noti = RpcRespNoti(done_tx);
        struct Reader<F: ServerFactory, D: ReqDispatch<R>, R: RespReceiver> {
            noti: RpcRespNoti<R::ChannelItem>,
            conn: Arc<F::Transport>,
            server_close_rx: crossfire::MAsyncRx<()>,
            dispatch: Arc<D>,
        }

        impl<F: ServerFactory, D: ReqDispatch<R>, R: RespReceiver> Reader<F, D, R> {
            async fn run(self) -> Result<(), ()> {
                loop {
                    match self.conn.recv_req(&self.server_close_rx).await {
                        Ok(req) => {
                            if req.action == RpcAction::Num(0) && req.msg.len() == 0 {
                                // ping request
                                self.send_quick_resp(req.seq, None)?;
                            } else {
                                let seq = req.seq;
                                if self.dispatch.dispatch_req(req, self.noti.clone()).await.is_err()
                                {
                                    self.send_quick_resp(seq, Some(RPC_ERR_DECODE))?;
                                }
                            }
                        }
                        Err(_e) => {
                            return Err(());
                        }
                    }
                }
            }

            #[inline]
            fn send_quick_resp(&self, seq: u64, err: Option<RpcError>) -> Result<(), ()> {
                if self.noti.send_err(seq, err).is_err() {
                    logger_warn!(
                        self.conn.get_logger(),
                        "{:?} reader abort due to writer has err",
                        self.conn
                    );
                    return Err(());
                }
                Ok(())
            }
        }
        let reader = Reader::<F, _, _> {
            noti,
            conn: conn.clone(),
            server_close_rx,
            dispatch: dispatch.clone(),
        };
        factory.spawn_detach(async move { reader.run().await });

        struct Writer<F: ServerFactory, D: ReqDispatch<R>, R: RespReceiver> {
            dispatch: Arc<D>,
            done_rx: crossfire::AsyncRx<Result<R::ChannelItem, (u64, Option<RpcError>)>>,
            conn: Arc<F::Transport>,
        }

        impl<F: ServerFactory, D: ReqDispatch<R>, R: RespReceiver> Writer<F, D, R> {
            async fn run(self) -> Result<(), io::Error> {
                macro_rules! process {
                    ($task: expr) => {{
                        match $task {
                            Ok(mut _task) => {
                                let (seq, res) = self.dispatch.encode_resp(&mut _task);
                                self.conn.send_resp(seq, res).await?;
                            }
                            Err((seq, None)) => {
                                self.conn.send_resp(seq, Ok((vec![], None))).await?;
                            }
                            Err((seq, Some(err))) => {
                                self.conn.send_resp(seq, Err(&err)).await?;
                            }
                        }
                    }};
                }
                while let Ok(task) = self.done_rx.recv().await {
                    process!(task);
                    while let Ok(task) = self.done_rx.try_recv() {
                        process!(task);
                    }
                    self.conn.flush_resp().await?;
                }
                logger_trace!(self.conn.get_logger(), "{:?} writer exits", self.conn);
                self.conn.close_conn().await;
                Ok(())
            }
        }
        let writer = Writer::<F, _, _> { done_rx, conn, dispatch };
        factory.spawn_detach(async move { writer.run().await });
    }
}

pub struct TaskReqDispatch<C, T, R, H, F>
where
    C: Codec,
    T: RpcServerTaskReq<R::ChannelItem>,
    R: RespReceiver,
    H: Fn(T) -> F + Send + Sync + 'static,
    F: Future<Output = Result<(), ()>> + Send + 'static,
{
    codec: C,
    task_handle: H,
    _phan: PhantomData<fn(&R, &T)>,
}

impl<C, T, R, H, F> TaskReqDispatch<C, T, R, H, F>
where
    C: Codec,
    T: RpcServerTaskReq<R::ChannelItem>,
    R: RespReceiver,
    H: Fn(T) -> F + Send + Sync + 'static,
    F: Future<Output = Result<(), ()>> + Send + 'static,
{
    #[inline]
    pub fn new(task_handle: H) -> Self {
        Self { codec: C::default(), task_handle, _phan: Default::default() }
    }
}

impl<C, T, R, H, F> ReqDispatch<R> for TaskReqDispatch<C, T, R, H, F>
where
    C: Codec,
    T: RpcServerTaskReq<R::ChannelItem>,
    R: RespReceiver,
    H: Fn(T) -> F + Send + Sync + 'static,
    F: Future<Output = Result<(), ()>> + Send + 'static,
{
    #[inline]
    async fn dispatch_req<'a>(
        &'a self, req: RpcSvrReq<'a>, noti: RpcRespNoti<R::ChannelItem>,
    ) -> Result<(), ()> {
        match <T as ServerTaskDecode<R::ChannelItem>>::decode_req(
            &self.codec,
            req.action,
            req.seq,
            req.msg,
            req.blob,
            noti,
        ) {
            Err(_) => {
                error!("action {:?} seq={} decode err", req.action, req.seq);
                return Err(());
            }
            Ok(task) => {
                if let Err(_) = (self.task_handle)(task).await {
                    error!("action {:?} seq={} dispatch err", req.action, req.seq);
                    return Err(());
                }
                Ok(())
            }
        }
    }

    #[inline]
    fn encode_resp<'a>(
        &'a self, task: &'a mut R::ChannelItem,
    ) -> (u64, Result<(Vec<u8>, Option<&'a Buffer>), &'a RpcError>) {
        R::encode_resp::<C>(&self.codec, task)
    }
}

pub struct RespReceiverTask<T: RpcServerTaskResp>(PhantomData<fn(&T)>);

impl<T: RpcServerTaskResp> RespReceiver for RespReceiverTask<T> {
    type ChannelItem = T;

    #[inline]
    fn encode_resp<'a, C: Codec>(
        codec: &'a C, task: &'a mut Self::ChannelItem,
    ) -> (u64, Result<(Vec<u8>, Option<&'a Buffer>), &'a RpcError>) {
        task.encode_resp(codec)
    }
}

pub struct RespReceiverBuf();
impl RespReceiver for RespReceiverBuf {
    type ChannelItem = RpcSvrResp;

    #[inline]
    fn encode_resp<'a, C: Codec>(
        _codec: &'a C, item: &'a mut Self::ChannelItem,
    ) -> (u64, Result<(Vec<u8>, Option<&'a Buffer>), &'a RpcError>) {
        match &mut item.res {
            Ok(()) => {
                let msg = item.msg.take().unwrap();
                (item.seq, Ok((msg, item.blob.as_ref())))
            }
            Err(e) => (item.seq, Err(e)),
        }
    }
}

/// A container that impl RpcServerTaskResp to show an example,
/// pressuming you have a different types to represent Request and Response.
/// You can write your customize version.
#[allow(dead_code)]
pub struct ServerTaskVariant<T: Send + Unpin + 'static, M> {
    pub seq: u64,
    pub msg: M,
    pub blob: Option<Buffer>,
    pub res: Option<Result<(), RpcError>>,
    pub noti: Option<RpcRespNoti<T>>,
}

impl<T: Send + Unpin + 'static, M: fmt::Debug> fmt::Debug for ServerTaskVariant<T, M> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "task seq={} {:?}", self.seq, self.msg)
    }
}

impl<T: Send + Unpin + 'static, M: 'static> ServerTaskDone<T> for ServerTaskVariant<T, M> {
    fn set_result(&mut self, res: Result<(), RpcError>) -> RpcRespNoti<T> {
        self.res.replace(res);
        return self.noti.take().unwrap();
    }
}

impl<T: Send + Unpin + 'static, M: for<'b> Deserialize<'b> + 'static> ServerTaskDecode<T>
    for ServerTaskVariant<T, M>
{
    fn decode_req<'a, C: Codec>(
        codec: &'a C, _action: RpcAction<'a>, seq: u64, msg: &'a [u8], blob: Option<Buffer>,
        noti: RpcRespNoti<T>,
    ) -> Result<Self, ()> {
        let req = codec.decode(msg)?;
        Ok(Self { seq, msg: req, blob, res: None, noti: Some(noti) })
    }
}

impl<T: Send + Unpin + 'static, M: Serialize + 'static> ServerTaskEncode
    for ServerTaskVariant<T, M>
{
    fn encode_resp<'a, C: Codec>(
        &'a self, codec: &'a C,
    ) -> (u64, Result<(Vec<u8>, Option<&'a Buffer>), &'a RpcError>) {
        if let Some(res) = self.res.as_ref() {
            match codec.encode(&self.msg) {
                Err(_) => {
                    return (self.seq, Err(&RPC_ERR_ENCODE));
                }
                Ok(resp) => match res {
                    Ok(_) => {
                        return (self.seq, Ok((resp, self.blob.as_ref())));
                    }
                    Err(e) => {
                        return (self.seq, Err(e));
                    }
                },
            }
        } else {
            panic!("no result when encode_resp");
        }
    }
}

/// A container that impl RpcServerTaskResp to show an example,
/// pressuming you have a type to carry both Request and Response.
/// You can write your customize version.
#[allow(dead_code)]
pub struct ServerTaskVariantFull<T: Send + Unpin + 'static, R: 'static, P: 'static> {
    seq: u64,
    req: R,
    req_blob: Option<Buffer>,
    resp: Option<P>,
    resp_blob: Option<Buffer>,
    res: Option<Result<(), RpcError>>,
    done_tx: Option<RpcRespNoti<T>>,
}

impl<T: Send + Unpin + 'static, M: fmt::Debug, P> fmt::Debug for ServerTaskVariantFull<T, M, P> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "task seq={} {:?}", self.seq, self.req)
    }
}

impl<T: Send + Unpin + 'static, M: 'static, P> ServerTaskDone<T>
    for ServerTaskVariantFull<T, M, P>
{
    fn set_result(&mut self, res: Result<(), RpcError>) -> RpcRespNoti<T> {
        self.res.replace(res);
        return self.done_tx.take().unwrap();
    }
}

impl<T: Send + Unpin + 'static, M: for<'b> Deserialize<'b> + 'static, P> ServerTaskDecode<T>
    for ServerTaskVariantFull<T, M, P>
{
    fn decode_req<'a, C: Codec>(
        codec: &'a C, _action: RpcAction<'a>, seq: u64, msg: &'a [u8], blob: Option<Buffer>,
        noti: RpcRespNoti<T>,
    ) -> Result<Self, ()> {
        let req = codec.decode(msg)?;
        Ok(Self {
            seq,
            req,
            req_blob: blob,
            res: None,
            resp: None,
            resp_blob: None,
            done_tx: Some(noti),
        })
    }
}

impl<T: Send + Unpin + 'static, M, P: Serialize + 'static> ServerTaskEncode
    for ServerTaskVariantFull<T, M, P>
{
    fn encode_resp<'a, C: Codec>(
        &'a self, codec: &'a C,
    ) -> (u64, Result<(Vec<u8>, Option<&'a Buffer>), &'a RpcError>) {
        if let Some(res) = self.res.as_ref() {
            if let Some(resp) = self.resp.as_ref() {
                match codec.encode(resp) {
                    Err(_) => {
                        return (self.seq, Err(&RPC_ERR_ENCODE));
                    }
                    Ok(resp_buf) => match res {
                        Ok(_) => {
                            return (self.seq, Ok((resp_buf, self.resp_blob.as_ref())));
                        }
                        Err(e) => {
                            return (self.seq, Err(e));
                        }
                    },
                }
            } else {
                match res {
                    Ok(_) => {
                        return (self.seq, Ok((vec![], self.resp_blob.as_ref())));
                    }
                    Err(e) => {
                        return (self.seq, Err(e));
                    }
                }
            }
        } else {
            panic!("no result when encode_resp");
        }
    }
}
