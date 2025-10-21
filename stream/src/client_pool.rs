use crate::client::{ClientCaller, ClientCallerBlocking, ClientFactory, ClientTaskDone};
use crate::client_stream::ClientStream;
use crossfire::{MAsyncRx, MAsyncTx, MTx, mpmc};
use occams_rpc_core::{error::RpcIntErr, runtime::AsyncIO};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool, AtomicUsize,
    Ordering::{Acquire, Relaxed, Release, SeqCst},
};
use std::time::Duration;

/// The ClientPool is a connection pool to the same server address
///
/// There should be a connection:
/// - serves as ping monitoring
/// - cleanup the task in channel with error_handle when the address is unhealthy,
/// because:
/// - The task incoming might never stop until faulty pool remove from pools collection
/// - If ping mixed with task with real business, might blocked due to throttler of in-flight
/// message in the stream.
pub struct ClientPool<F: ClientFactory> {
    tx_async: MAsyncTx<F::Task>,
    tx: MTx<F::Task>,
    inner: Arc<ClientPoolInner<F>>,
}

struct ClientPoolInner<F: ClientFactory> {
    factory: Arc<F>,
    logger: F::Logger,
    rx: MAsyncRx<F::Task>,
    addr: String,
    conn_id: String,
    /// whether connection is healthy?
    is_ok: AtomicBool,
    /// dynamic worker count (not the monitor)
    worker_count: AtomicUsize,
    /// dynamic worker count (not the monitor)
    connected_worker_count: AtomicUsize,
    ///// Set by user
    //limit: AtomicUsize, // TODO
}

const ONE_SEC: Duration = Duration::from_secs(1);

impl<F: ClientFactory> ClientPool<F> {
    pub fn new(factory: Arc<F>, addr: &str) -> Self {
        let config = factory.get_config();
        let (tx_async, rx) = mpmc::bounded_async(config.thresholds);
        let tx = tx_async.clone().into();
        let conn_id = format!("to {}", addr);
        let inner = Arc::new(ClientPoolInner {
            logger: factory.new_logger(&conn_id),
            factory: factory.clone(),
            rx,
            addr: addr.to_string(),
            conn_id,
            is_ok: AtomicBool::new(true),
            worker_count: AtomicUsize::new(0),
            connected_worker_count: AtomicUsize::new(0),
        });
        let s = Self { tx_async, tx, inner };
        s.spawn();
        s
    }

    #[inline]
    pub async fn send_req(&self, task: F::Task) {
        ClientCaller::send_req(self, task).await;
    }

    #[inline]
    pub fn send_req_blocking(&self, task: F::Task) {
        ClientCallerBlocking::send_req_blocking(self, task);
    }

    #[inline]
    pub fn spawn(&self) {
        let worker_id = self.inner.worker_count.fetch_add(1, Acquire);
        self.inner.clone().spawn_worker(worker_id);
    }
}

impl<F: ClientFactory> Drop for ClientPool<F> {
    fn drop(&mut self) {
        self.inner.cleanup();
    }
}

impl<F: ClientFactory> ClientCaller<F> for ClientPool<F> {
    #[inline]
    async fn send_req(&self, task: F::Task) {
        self.tx_async.send(task).await.expect("submit");
    }
}

impl<F: ClientFactory> ClientCallerBlocking<F> for ClientPool<F> {
    #[inline]
    fn send_req_blocking(&self, task: F::Task) {
        self.tx.send(task).expect("submit");
    }
}

impl<F: ClientFactory> fmt::Display for ClientPoolInner<F> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ConnPool {:?}", self.conn_id)
    }
}

impl<F: ClientFactory> ClientPoolInner<F> {
    fn spawn_worker(self: Arc<Self>, worker_id: usize) {
        let factory = self.factory.clone();
        factory.spawn_detach(async move {
            logger_trace!(&self.logger, "{} worker_id={} running", self, worker_id);
            self.run(worker_id).await;
            self.worker_count.fetch_sub(1, SeqCst);
            logger_trace!(&self.logger, "{} worker_id={} exit", self, worker_id);
        });
    }

    #[inline(always)]
    fn get_workers(&self) -> usize {
        self.worker_count.load(SeqCst)
    }

    #[inline(always)]
    fn get_healthy_workers(&self) -> usize {
        self.connected_worker_count.load(SeqCst)
    }

    #[inline(always)]
    fn set_err(&self) {
        self.is_ok.store(false, SeqCst);
    }

    #[inline]
    async fn connect(&self) -> Result<ClientStream<F>, RpcIntErr> {
        ClientStream::connect(self.factory.clone(), &self.addr, &self.conn_id, None).await
    }

    #[inline(always)]
    async fn _run_worker(
        &self, _worker_id: usize, stream: &mut ClientStream<F>,
    ) -> Result<(), RpcIntErr> {
        loop {
            let task = self.rx.recv().await.unwrap();
            stream.send_task(task, false).await?;
            while let Ok(task) = self.rx.try_recv() {
                stream.send_task(task, false).await?;
            }
            stream.flush_req().await?;
        }
    }

    async fn run_worker(
        &self, worker_id: usize, stream: &mut ClientStream<F>,
    ) -> Result<(), RpcIntErr> {
        self.connected_worker_count.fetch_add(1, Acquire);
        let r = self._run_worker(worker_id, stream).await;
        self.connected_worker_count.fetch_add(1, Release);
        r
    }

    async fn run(self: &Arc<Self>, mut worker_id: usize) {
        'CONN_LOOP: loop {
            match self.connect().await {
                Ok(mut stream) => {
                    if worker_id == 0 {
                        // act as monitor
                        'MONITOR: loop {
                            if self.get_workers() > 1 {
                                F::IO::sleep(ONE_SEC).await;
                                if stream.ping().await.is_err() {
                                    self.set_err();
                                    // don't cleanup the channel unless only one worker left
                                    continue 'CONN_LOOP;
                                }
                            } else {
                                match self.rx.recv_with_timer(F::IO::sleep(ONE_SEC)).await {
                                    Err(_) => {
                                        // sleep passed
                                        if stream.ping().await.is_err() {
                                            self.set_err();
                                            self.cleanup();
                                            continue 'CONN_LOOP;
                                        }
                                    }
                                    Ok(task) => {
                                        if stream.get_inflight_count() > 0
                                            && self.get_workers() == 1
                                        {
                                            if self
                                                .worker_count
                                                .compare_exchange(1, 2, SeqCst, Relaxed)
                                                .is_ok()
                                            {
                                                // there's might be a lag to connect,
                                                // so we are spawning identity with new worker,
                                                worker_id = 1;
                                                self.clone().spawn_worker(0);
                                            }
                                        }
                                        if stream.send_task(task, true).await.is_err() {
                                            self.set_err();
                                            if worker_id == 0 {
                                                self.cleanup();
                                                F::IO::sleep(ONE_SEC).await;
                                                continue 'CONN_LOOP;
                                            } else {
                                                return;
                                            }
                                        } else if worker_id > 0 {
                                            // taken over as run_worker.
                                            break 'MONITOR;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if worker_id > 0 {
                        if self.run_worker(worker_id, &mut stream).await.is_err() {
                            self.set_err();
                            // don't cleanup the channel unless only one worker left
                        }
                        // TODO If worker will exit automiatically when idle_time passed
                        return;
                    }
                }
                Err(e) => {
                    self.set_err();
                    error!("connect failed to {}: {}", self.addr, e);
                    self.cleanup();
                    F::IO::sleep(ONE_SEC).await;
                }
            }
        }
    }

    fn cleanup(&self) {
        while let Ok(mut task) = self.rx.try_recv() {
            task.set_rpc_error(RpcIntErr::Unreachable);
            self.factory.error_handle(task);
        }
    }
}
