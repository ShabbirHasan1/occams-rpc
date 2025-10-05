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
pub struct ServerHandleTaskStream<R: RpcServerTaskResp>(std::marker::PhantomData<fn(&R)>);

impl<F: ServerFactory, R: RpcServerTaskResp> ServerHandle<F> for ServerHandleTaskStream<R> {
    fn run(conn: F::Transport, factory: &F, server_close_rx: crossfire::MAsyncRx<()>) {
        let conn = Arc::new(conn);

        let (done_tx, done_rx) =
            crossfire::mpsc::unbounded_async::<Result<R, (u64, Option<RpcError>)>>();
        let dispatch = factory.get_dispatcher::<R>();
        struct Reader<F: ServerFactory, R: RpcServerTaskResp, D: TaskDispatcher<R>> {
            done_tx: crossfire::MTx<Result<R, (u64, Option<RpcError>)>>,
            conn: Arc<F::Transport>,
            server_close_rx: crossfire::MAsyncRx<()>,
            dispatch: D,
            codec: F::Codec,
        }

        impl<F: ServerFactory, R: RpcServerTaskResp, D: TaskDispatcher<R>> Reader<F, R, D> {
            async fn run(self) -> Result<(), ()> {
                loop {
                    match self.conn.recv_req(&self.server_close_rx).await {
                        Ok(req) => {
                            if req.action == RpcAction::Num(0) && req.msg.len() == 0 {
                                // ping request
                                self.send_quick_resp(req.seq, None)?;
                            } else {
                                match <D::Task as ServerTaskDecode<'_, R>>::decode_req(
                                    &self.codec,
                                    req.action,
                                    req.seq,
                                    req.msg,
                                    req.blob,
                                    RpcRespNoti(self.done_tx.clone()),
                                ) {
                                    Err(_) => {
                                        error!(
                                            "{:?} reader: action {:?} seq={} decode err",
                                            self.conn, req.action, req.seq
                                        );
                                        self.send_quick_resp(req.seq, Some(RPC_ERR_DECODE))?;
                                    }
                                    Ok(t) => {
                                        self.dispatch.dispatch_task(t).await;
                                    }
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
                if self.done_tx.send(Err((seq, err))).is_err() {
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
        let reader = Reader::<F, R, _> {
            done_tx,
            conn: conn.clone(),
            server_close_rx,
            codec: F::Codec::default(),
            dispatch,
        };
        factory.spawn_detach(async move { reader.run().await });

        struct Writer<F: ServerFactory, R: RpcServerTaskResp> {
            codec: F::Codec,
            done_rx: crossfire::AsyncRx<Result<R, (u64, Option<RpcError>)>>,
            conn: Arc<F::Transport>,
        }

        impl<F: ServerFactory, R: RpcServerTaskResp> Writer<F, R> {
            async fn run(self) -> Result<(), io::Error> {
                macro_rules! process {
                    ($task: expr) => {{
                        match $task {
                            Ok(_task) => match _task.encode_resp(&self.codec) {
                                Err(seq) => {
                                    self.conn.send_resp(seq, Err(&RPC_ERR_ENCODE)).await?;
                                }
                                Ok((seq, res)) => {
                                    self.conn.send_resp(seq, res).await?;
                                }
                            },
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
        let writer = Writer::<F, R> { done_rx, conn, codec: F::Codec::default() };
        factory.spawn_detach(async move { writer.run().await });
    }
}

/// A container that impl RpcServerTaskResp to show an example,
/// pressuming you have a different types to represent Request and Response.
/// You can write your customize version.
pub struct ServerTaskVariant<T, M: fmt::Debug> {
    seq: u64,
    msg: M,
    blob: Option<Buffer>,
    res: Option<Result<(), RpcError>>,
    done_tx: Option<RpcRespNoti<T>>,
}

impl<T, M: fmt::Debug> fmt::Debug for ServerTaskVariant<T, M> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "task seq={} {:?}", self.seq, self.msg)
    }
}

impl<T: RpcServerTaskResp, M: 'static + fmt::Debug> ServerTaskDone<T> for ServerTaskVariant<T, M> {
    fn set_result(mut self, res: Result<(), RpcError>) -> RpcRespNoti<T> {
        self.res.replace(res);
        return self.done_tx.take().unwrap();
    }
}

impl<'a, T: RpcServerTaskResp, M: Deserialize<'a> + 'static + fmt::Debug> ServerTaskDecode<'a, T>
    for ServerTaskVariant<T, M>
{
    fn decode_req<C: Codec>(
        codec: &'a C, _action: RpcAction<'a>, seq: u64, msg: &'a [u8], blob: Option<Buffer>,
        noti: RpcRespNoti<T>,
    ) -> Result<Self, ()> {
        let req = codec.decode(msg)?;
        Ok(Self { seq, msg: req, blob, res: None, done_tx: Some(noti) })
    }
}

impl<T, M: Serialize + 'static + fmt::Debug> ServerTaskEncode for ServerTaskVariant<T, M> {
    fn encode_resp<'a, C: Codec>(
        &'a self, codec: &'a C,
    ) -> Result<(u64, Result<(Vec<u8>, Option<&'a Buffer>), &'a RpcError>), u64> {
        if let Some(res) = self.res.as_ref() {
            match codec.encode(&self.msg) {
                Err(_) => {
                    return Err(self.seq);
                }
                Ok(resp) => match res {
                    Ok(_) => {
                        return Ok((self.seq, Ok((resp, self.blob.as_ref()))));
                    }
                    Err(e) => {
                        return Ok((self.seq, Err(e)));
                    }
                },
            }
        } else {
            error!("task {:?} has no result", self);
            return Err(self.seq);
        }
    }
}
