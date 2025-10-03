use super::server::{RpcServerTask, ServerFactory, ServerHandle, ServerTransport};
use super::*;
use crate::io::AsyncListener;
use crate::runtime::AsyncIO;
use futures::{
    FutureExt,
    future::{AbortHandle, Abortable},
};
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
/// It will dispatch task through a channel. It's possible the task is an enum that impl RpcServerTask.
pub struct ServerHandleTaskStream<T: RpcServerTask>(std::marker::PhantomData<fn(&T)>);

impl<F: ServerFactory, T: RpcServerTask> ServerHandle<F> for ServerHandleTaskStream<T> {
    fn run(conn: F::Transport, factory: &F, server_close_rx: crossfire::MAsyncRx<()>) {
        let conn = Arc::new(conn);

        let (done_tx, done_rx) = crossfire::mpsc::unbounded_async::<Result<T, u64>>();
        struct Reader<F: ServerFactory, T: RpcServerTask> {
            done_tx: crossfire::MTx<Result<T, u64>>,
            conn: Arc<F::Transport>,
            server_close_rx: crossfire::MAsyncRx<()>,
        }

        impl<F: ServerFactory, T: RpcServerTask> Reader<F, T> {
            async fn run(self) -> Result<(), ()> {
                loop {
                    match self.conn.recv_req(&self.server_close_rx).await {
                        Ok(req) => {
                            if req.action == RpcAction::Num(0) && req.msg.is_none() {
                                self.send_ping_resp(req.seq).await?;
                            }
                            todo!(); // decode
                        }
                        Err(_e) => {
                            return Err(());
                        }
                    }
                }
            }

            #[inline]
            async fn send_ping_resp(&self, seq: u64) -> Result<(), ()> {
                if self.done_tx.send(Err(seq)).is_err() {
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
        let reader = Reader::<F, T> { done_tx, conn: conn.clone(), server_close_rx };
        factory.spawn_detach(async move { reader.run().await });

        struct Writer<F: ServerFactory, T: RpcServerTask> {
            done_rx: crossfire::AsyncRx<Result<T, u64>>,
            conn: Arc<F::Transport>,
        }

        impl<F: ServerFactory, T: RpcServerTask> Writer<F, T> {
            async fn run(self) -> Result<(), io::Error> {
                macro_rules! process {
                    ($task: expr) => {{
                        match $task {
                            Ok(_task) => {
                                todo!();
                            }
                            Err(seq) => {
                                self.conn.send_resp(seq, Ok((b"", None))).await?;
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
        let writer = Writer::<F, T> { done_rx, conn };
        factory.spawn_detach(async move { writer.run().await });
    }
}
