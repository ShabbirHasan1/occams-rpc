//! The RpcServer implementation, and some structs that impl the traits in crate::server
//!
//! There are pre-defined structs that impl [ServerTaskDecode] and [ServerTaskResp]:
//! - [ServerTaskVariant] is for the situation to map a request struct to a response struct
//! - [ServerTaskVariantFull] is for the situation of holding the request and response msg in the same struct

use crate::proto::{RpcAction, RpcActionOwned};
use crate::server::*;
use futures::future::{AbortHandle, Abortable};
use io_buffer::Buffer;
use occams_rpc_core::{Codec, error::*, io::AsyncListener, runtime::AsyncIO};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// An RpcServer that listen, accept, and server connections, according to ServerFactory interface.
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

    pub fn listen(&mut self, addr: &str) -> io::Result<String> {
        match <<F::Transport as ServerTransport<F>>::Listener as AsyncListener>::bind(addr) {
            Err(e) => {
                error!("bind addr {:?} err: {}", addr, e);
                return Err(e);
            }
            Ok(mut listener) => {
                let local_addr = match listener.local_addr() {
                    Ok(addr) => addr,
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::AddrNotAvailable {
                            // For Unix sockets, return a dummy address
                            "0.0.0.0:0".parse().unwrap()
                        } else {
                            return Err(e);
                        }
                    }
                };
                let (abort_handle, abort_registration) = AbortHandle::new_pair();
                let factory = self.factory.clone();
                let conn_ref_count = self.conn_ref_count.clone();
                let listener_info = format!("listener {:?}", addr);
                let server_close_rx = self.server_close_rx.clone();
                debug!("listening on {:?}", listener);
                let abrt = Abortable::new(
                    async move {
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
                                    Self::server_conn(conn, &factory, server_close_rx.clone())
                                }
                            }
                        }
                    },
                    abort_registration,
                );

                self.factory.spawn_detach(abrt);
                self.listeners_abort.push((abort_handle, listener_info));
                return Ok(local_addr);
            }
        }
    }

    fn server_conn(conn: F::Transport, factory: &F, server_close_rx: crossfire::MAsyncRx<()>) {
        let conn = Arc::new(conn);

        let dispatch = Arc::new(factory.new_dispatcher());
        let (done_tx, done_rx) = crossfire::mpsc::unbounded_async();

        let noti = RespNoti(done_tx);
        struct Reader<F: ServerFactory, D: ReqDispatch<R>, R: ServerTaskResp> {
            noti: RespNoti<R>,
            conn: Arc<F::Transport>,
            server_close_rx: crossfire::MAsyncRx<()>,
            dispatch: Arc<D>,
        }

        impl<F: ServerFactory, D: ReqDispatch<R>, R: ServerTaskResp> Reader<F, D, R> {
            async fn run(self) -> Result<(), ()> {
                loop {
                    match self.conn.read_req(&self.server_close_rx).await {
                        Ok(req) => {
                            if req.action == RpcAction::Num(0) && req.msg.len() == 0 {
                                // ping request
                                self.send_quick_resp(req.seq, None)?;
                            } else {
                                let seq = req.seq;
                                if self.dispatch.dispatch_req(req, self.noti.clone()).await.is_err()
                                {
                                    self.send_quick_resp(seq, Some(RpcIntErr::Decode.into()))?;
                                }
                            }
                        }
                        Err(_e) => {
                            // XXX read_req return error not used
                            return Err(());
                        }
                    }
                }
            }

            #[inline]
            fn send_quick_resp(&self, seq: u64, err: Option<RpcIntErr>) -> Result<(), ()> {
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

        struct Writer<F: ServerFactory, D: ReqDispatch<R>, R: ServerTaskResp> {
            dispatch: Arc<D>,
            done_rx: crossfire::AsyncRx<Result<R, (u64, Option<RpcIntErr>)>>,
            conn: Arc<F::Transport>,
        }

        impl<F: ServerFactory, D: ReqDispatch<R>, R: ServerTaskResp> Writer<F, D, R> {
            async fn run(self) -> Result<(), io::Error> {
                macro_rules! process {
                    ($task: expr) => {{
                        match $task {
                            Ok(_task) => {
                                logger_trace!(self.conn.get_logger(), "write_resp {:?}", _task);
                                self.conn.write_resp(self.dispatch.get_codec(), _task).await?;
                            }
                            Err((seq, err)) => {
                                self.conn.write_resp_internal(seq, err).await?;
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

    #[inline]
    fn get_alive_conn(&self) -> usize {
        Arc::strong_count(&self.conn_ref_count) - 1
    }

    /// Gracefully close the server
    ///
    /// Steps:
    /// - listeners coroutine is abort
    /// - drop the close channel to notify connection read coroutines.
    /// - the writer coroutines will exit after all the reference of RespNoti channel drop to 0
    /// - wait for connection coroutines to exit with a timeout defined by
    /// ServerConfig.server_close_wait
    pub async fn close(&mut self) {
        // close listeners
        for h in &self.listeners_abort {
            h.0.abort();
            logger_info!(self.logger, "{} has closed", h.1);
        }
        // Notify all reader connection exit, then the reader will notify writer
        let _ = self.server_close_tx.lock().unwrap().take();

        let mut exists_count = self.get_alive_conn();
        // wait client close all connections
        let start_ts = Instant::now();
        let config = self.factory.get_config();
        while exists_count > 0 {
            <F::IO as AsyncIO>::sleep(Duration::from_secs(1)).await;
            exists_count = self.get_alive_conn();
            if Instant::now().duration_since(start_ts) > config.server_close_wait {
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

/// A ReqDispatch trait impl with a closure, only useful for writing tests.
///
/// NOTE: The closure requires Clone.
///
/// # Example
///
/// ```no_compile,ignore
/// use occams_rpc_stream::server::{ServerFactory, ReqDispatch};
/// impl ServerFactory for YourServer {
///
///     ...
///
///     #[inline]
///     fn new_dispatcher(&self) -> impl ReqDispatch<Self::RespTask> {
///         let dispatch_f = move |task: FileServerTask| {
///             async move {
///                 todo!();
///             }
///         }
///         return ReqDispatchClosure::<MsgpCodec, YourServerTask, Self::RespTask, _, _>::new(
///             dispatch_f,
///         );
///     }
/// }
/// ```
pub struct ReqDispatchClosure<C, T, R, H, F>
where
    C: Codec,
    T: ServerTaskDecode<R>,
    R: ServerTaskResp,
    H: FnOnce(T) -> F + Send + Sync + 'static + Clone,
    F: Future<Output = Result<(), ()>> + Send + 'static,
{
    codec: C,
    task_handle: H,
    _phan: PhantomData<fn(&R, &T)>,
}

impl<C, T, R, H, F> ReqDispatchClosure<C, T, R, H, F>
where
    C: Codec,
    T: ServerTaskDecode<R>,
    R: ServerTaskResp,
    H: FnOnce(T) -> F + Send + Sync + 'static + Clone,
    F: Future<Output = Result<(), ()>> + Send + 'static,
{
    #[inline]
    pub fn new(task_handle: H) -> Self {
        Self { codec: C::default(), task_handle, _phan: Default::default() }
    }
}

impl<C, T, R, H, F> ReqDispatch<R> for ReqDispatchClosure<C, T, R, H, F>
where
    C: Codec,
    T: ServerTaskDecode<R>,
    R: ServerTaskResp,
    H: FnOnce(T) -> F + Send + Sync + 'static + Clone,
    F: Future<Output = Result<(), ()>> + Send + 'static,
{
    #[inline]
    async fn dispatch_req<'a>(&'a self, req: RpcSvrReq<'a>, noti: RespNoti<R>) -> Result<(), ()> {
        match <T as ServerTaskDecode<R>>::decode_req(
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
                let handle = self.task_handle.clone();
                if let Err(_) = (handle)(task).await {
                    error!("action {:?} seq={} dispatch err", req.action, req.seq);
                    return Err(());
                }
                Ok(())
            }
        }
    }

    #[inline(always)]
    fn get_codec(&self) -> &impl Codec {
        &self.codec
    }
}

/// A container that impl ServerTaskResp to show an example,
/// presuming you have a different types to represent Request and Response.
/// You can write your customize version.
#[allow(dead_code)]
pub struct ServerTaskVariant<T, M, E>
where
    T: Send + Unpin + 'static,
    M: Send + Unpin + 'static,
    E: RpcErrCodec,
{
    pub seq: u64,
    pub action: RpcActionOwned,
    pub msg: M,
    pub blob: Option<Buffer>,
    pub res: Option<Result<(), E>>,
    pub noti: Option<RespNoti<T>>,
}

impl<T, M, E> fmt::Debug for ServerTaskVariant<T, M, E>
where
    T: Send + Unpin + 'static,
    M: fmt::Debug + Send + Unpin + 'static,
    E: RpcErrCodec + fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "task seq={} action={:?} {:?}", self.seq, self.action, self.msg)?;
        match self.res.as_ref() {
            Some(Ok(())) => {
                write!(f, " ok")
            }
            Some(Err(e)) => {
                write!(f, " err: {}", e)
            }
            _ => Ok(()),
        }
    }
}

impl<T, M, E> ServerTaskDone<T, E> for ServerTaskVariant<T, M, E>
where
    T: Send + Unpin + 'static,
    M: Send + Unpin + 'static,
    E: RpcErrCodec,
{
    fn _set_result(&mut self, res: Result<(), E>) -> RespNoti<T> {
        self.res.replace(res);
        return self.noti.take().unwrap();
    }
}

impl<T, M, E> ServerTaskDecode<T> for ServerTaskVariant<T, M, E>
where
    T: Send + Unpin + 'static,
    M: for<'b> Deserialize<'b> + Send + Unpin + 'static,
    E: RpcErrCodec,
{
    fn decode_req<'a, C: Codec>(
        codec: &'a C, action: RpcAction<'a>, seq: u64, msg: &'a [u8], blob: Option<Buffer>,
        noti: RespNoti<T>,
    ) -> Result<Self, ()> {
        let req = codec.decode(msg)?;
        Ok(Self { seq, action: action.into(), msg: req, blob, res: None, noti: Some(noti) })
    }
}

impl<T, M, E> ServerTaskAction for ServerTaskVariant<T, M, E>
where
    T: Send + Unpin + 'static,
    M: Send + Unpin + 'static,
    E: RpcErrCodec,
{
    fn get_action<'a>(&'a self) -> RpcAction<'a> {
        self.action.to_action()
    }
}

impl<T, M, E> ServerTaskEncode for ServerTaskVariant<T, M, E>
where
    T: Send + Unpin + 'static,
    M: Serialize + Send + Unpin + 'static,
    E: RpcErrCodec,
{
    #[inline]
    fn encode_resp<'a, 'b, C: Codec>(
        &'a mut self, codec: &'b C, buf: &'b mut Vec<u8>,
    ) -> (u64, Result<(usize, Option<&'a [u8]>), EncodedErr>) {
        if let Some(res) = self.res.as_ref() {
            match res {
                Ok(_) => match codec.encode_into(&self.msg, buf) {
                    Err(_) => {
                        return (self.seq, Err(RpcIntErr::Encode.into()));
                    }
                    Ok(msg_len) => {
                        return (self.seq, Ok((msg_len, self.blob.as_deref())));
                    }
                },
                Err(e) => {
                    return (self.seq, Err(e.encode::<C>(codec)));
                }
            }
        } else {
            panic!("no result when encode_resp");
        }
    }
}

/// A container that impl ServerTaskResp to show an example,
/// presuming you have a type to carry both Request and Response.
/// You can write your customize version.
#[allow(dead_code)]
pub struct ServerTaskVariantFull<T, R, P, E>
where
    T: Send + Unpin + 'static,
    R: Send + Unpin + 'static,
    P: Send + Unpin + 'static,
    E: RpcErrCodec,
{
    pub seq: u64,
    pub action: RpcActionOwned,
    pub req: R,
    pub req_blob: Option<Buffer>,
    pub resp: Option<P>,
    pub resp_blob: Option<Buffer>,
    pub res: Option<Result<(), E>>,
    noti: Option<RespNoti<T>>,
}

impl<T, R, P, E> fmt::Debug for ServerTaskVariantFull<T, R, P, E>
where
    T: Send + Unpin + 'static,
    R: Send + Unpin + 'static + fmt::Debug,
    P: Send + Unpin + 'static,
    E: RpcErrCodec + fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "task seq={} action={:?} {:?}", self.seq, self.action, self.req)?;
        match self.res.as_ref() {
            Some(Ok(())) => write!(f, " ok"),
            Some(Err(e)) => write!(f, " err: {}", e),
            _ => Ok(()),
        }
    }
}

impl<T, R, P, E> ServerTaskDone<T, E> for ServerTaskVariantFull<T, R, P, E>
where
    T: Send + Unpin + 'static,
    R: Send + Unpin + 'static,
    P: Send + Unpin + 'static,
    E: RpcErrCodec,
{
    fn _set_result(&mut self, res: Result<(), E>) -> RespNoti<T> {
        self.res.replace(res);
        return self.noti.take().unwrap();
    }
}

impl<T, R, P, E> ServerTaskDecode<T> for ServerTaskVariantFull<T, R, P, E>
where
    T: Send + Unpin + 'static,
    R: for<'b> Deserialize<'b> + Send + Unpin + 'static,
    P: Send + Unpin + 'static,
    E: RpcErrCodec,
{
    fn decode_req<'a, C: Codec>(
        codec: &'a C, action: RpcAction<'a>, seq: u64, msg: &'a [u8], blob: Option<Buffer>,
        noti: RespNoti<T>,
    ) -> Result<Self, ()> {
        let req = codec.decode(msg)?;
        Ok(Self {
            seq,
            action: action.into(),
            req,
            req_blob: blob,
            res: None,
            resp: None,
            resp_blob: None,
            noti: Some(noti),
        })
    }
}

impl<T, R, P, E> ServerTaskAction for ServerTaskVariantFull<T, R, P, E>
where
    T: Send + Unpin + 'static,
    R: Send + Unpin + 'static,
    P: Send + Unpin + 'static,
    E: RpcErrCodec,
{
    fn get_action<'a>(&'a self) -> RpcAction<'a> {
        self.action.to_action()
    }
}

impl<T, R, P, E> ServerTaskEncode for ServerTaskVariantFull<T, R, P, E>
where
    T: Send + Unpin + 'static,
    R: Send + Unpin + 'static,
    P: Send + Unpin + 'static + Serialize,
    E: RpcErrCodec,
{
    #[inline]
    fn encode_resp<'a, 'b, C: Codec>(
        &'a mut self, codec: &'b C, buf: &'b mut Vec<u8>,
    ) -> (u64, Result<(usize, Option<&'a [u8]>), EncodedErr>) {
        if let Some(res) = self.res.as_ref() {
            match res {
                Ok(_) => {
                    if let Some(resp) = self.resp.as_ref() {
                        match codec.encode_into(resp, buf) {
                            Err(_) => {
                                return (self.seq, Err(RpcIntErr::Encode.into()));
                            }
                            Ok(msg_len) => {
                                return (self.seq, Ok((msg_len, self.resp_blob.as_deref())));
                            }
                        }
                    } else {
                        // empty response
                        return (self.seq, Ok((0, self.resp_blob.as_deref())));
                    }
                }
                Err(e) => {
                    return (self.seq, Err(e.encode::<C>(codec)));
                }
            }
        } else {
            panic!("no result when encode_resp");
        }
    }
}
