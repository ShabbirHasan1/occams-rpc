use std::{
    cell::UnsafeCell,
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll},
};

use crate::error::*;
use crate::runtime::{AsyncIO, TimeInterval};
use crate::stream::{client::*, proto, throttler::*};
use crate::*;
use crossfire::*;
use futures::pin_mut;
use std::time::Duration;
use sync_utils::{time::DelayedTime, waitgroup::WaitGroupGuard};
use zerocopy::AsBytes;

/// RpcClient represents a client-side connection. connection will close on dropped
pub struct RpcClient<F: ClientFactory> {
    close_tx: Option<MTx<()>>,
    inner: Arc<RpcClientInner<F>>,
}

impl<F: ClientFactory> RpcClient<F> {
    /// timeout_setting: only use read_timeout/write_timeout
    pub fn new(
        factory: Arc<F>, conn: F::Transport, client_id: u64, server_id: u64,
        last_resp_ts: Option<Arc<AtomicU64>>,
    ) -> Self {
        let (_close_tx, _close_rx) = mpmc::unbounded_async::<()>();
        let inner = Arc::new(RpcClientInner::new(
            factory,
            conn,
            client_id,
            server_id,
            _close_rx,
            last_resp_ts,
        ));
        logger_debug!(inner.logger(), "{:?} connected", inner);
        let _inner = inner.clone();
        inner.factory.spawn_detach(async move {
            _inner.receive_loop().await;
        });
        Self { close_tx: Some(_close_tx), inner }
    }

    #[inline]
    pub fn get_codec(&self) -> &F::Codec {
        &self.inner.codec
    }

    /// Should be call in sender thread
    #[inline(always)]
    pub async fn ping(&self) -> Result<(), RpcError> {
        self.inner.send_ping_req().await
    }

    #[inline(always)]
    pub fn get_last_resp_ts(&self) -> u64 {
        if let Some(ts) = self.inner.last_resp_ts.as_ref() { ts.load(Ordering::Relaxed) } else { 0 }
    }

    /// Since sender and receiver are two threads, might be close on either side
    #[inline(always)]
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }

    /// Force the receiver to exit
    pub async fn set_error_and_exit(&self) {
        self.inner.has_err.store(true, Ordering::SeqCst);
        self.inner.conn.close().await;
        if let Some(close_tx) = self.close_tx.as_ref() {
            let _ = close_tx.send(()); // This equals to RpcClient::drop
        }
    }

    #[inline(always)]
    pub async fn send_task(&self, task: F::Task, need_flush: bool) -> Result<(), RpcError> {
        self.inner.send_task(task, need_flush).await
    }

    #[inline(always)]
    pub async fn flush_req(&self) -> Result<(), RpcError> {
        self.inner.flush_req().await
    }

    #[inline]
    pub fn will_block(&self) -> bool {
        if let Some(t) = self.inner.throttler.as_ref() { t.nearly_full() } else { false }
    }

    #[inline(always)]
    pub async fn throttle(&self) -> bool {
        if self.inner.closed.load(Ordering::SeqCst) {
            return false;
        }
        if let Some(t) = self.inner.throttler.as_ref() {
            return t.throttle().await;
        } else {
            false
        }
    }
}

impl<F: ClientFactory> Drop for RpcClient<F> {
    fn drop(&mut self) {
        self.close_tx.take();
        let timer = self.inner.get_timer_mut();
        timer.stop_reg_task();
        self.inner.closed.store(true, Ordering::SeqCst);
    }
}

impl<F: ClientFactory> fmt::Debug for RpcClient<F> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.inner.fmt(f)
    }
}

struct RpcClientInner<F: ClientFactory> {
    client_id: u64,
    conn: F::Transport,
    seq: AtomicU64,
    close_rx: MAsyncRx<()>, // When RpcClient(sender) dropped, receiver will be timer
    closed: AtomicBool,     // flag set by either sender or receive on there exit
    timer: UnsafeCell<RpcClientTaskTimer<F>>,
    has_err: AtomicBool,
    throttler: Option<Throttler>,
    last_resp_ts: Option<Arc<AtomicU64>>,
    codec: F::Codec,
    factory: Arc<F>,
}

unsafe impl<F: ClientFactory> Send for RpcClientInner<F> {}

unsafe impl<F: ClientFactory> Sync for RpcClientInner<F> {}

impl<F: ClientFactory> fmt::Debug for RpcClientInner<F> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.conn.fmt(f)
    }
}

impl<F: ClientFactory> RpcClientInner<F> {
    pub fn new(
        factory: Arc<F>, conn: F::Transport, client_id: u64, server_id: u64,
        close_rx: MAsyncRx<()>, last_resp_ts: Option<Arc<AtomicU64>>,
    ) -> Self {
        let config = factory.get_config();
        let thresholds = config.thresholds;
        let mut client_inner = Self {
            client_id,
            conn,
            close_rx,
            closed: AtomicBool::new(false),
            seq: AtomicU64::new(1),
            timer: UnsafeCell::new(RpcClientTaskTimer::new(
                server_id,
                client_id,
                config.timeout.task_timeout,
                thresholds,
            )),
            throttler: None,
            last_resp_ts,
            has_err: AtomicBool::new(false),
            codec: F::Codec::default(),
            factory,
        };
        if thresholds > 0 {
            logger_trace!(
                client_inner.logger(),
                "{:?} throttler is set to {}",
                client_inner,
                thresholds,
            );
            client_inner.throttler = Some(Throttler::new(thresholds));
        } else {
            logger_trace!(client_inner.logger(), "{:?} throttler is disabled", client_inner);
        }

        client_inner
    }

    #[inline(always)]
    fn logger(&self) -> &F::Logger {
        self.conn.get_logger()
    }

    #[inline(always)]
    fn get_timer_mut(&self) -> &mut RpcClientTaskTimer<F> {
        unsafe { std::mem::transmute(self.timer.get()) }
    }

    //    #[inline(always)]
    //    fn should_close(&self, e: Errno) -> bool {
    //          TODO replace this
    //        e == Errno::EAGAIN || e == Errno::EHOSTDOWN
    //    }

    /// Directly work on the socket steam, when failed
    async fn send_task(&self, mut task: F::Task, need_flush: bool) -> Result<(), RpcError> {
        let timer = self.get_timer_mut();
        timer.pending_task_count_ref().fetch_add(1, Ordering::SeqCst);
        if self.closed.load(Ordering::Acquire) {
            logger_warn!(
                self.logger(),
                "{:?} sending task {} failed: {}",
                self,
                task,
                RPC_ERR_CLOSED,
            );
            self.factory.error_handle(task, RPC_ERR_CLOSED);
            timer.pending_task_count_ref().fetch_sub(1, Ordering::SeqCst); // rollback
            return Err(RPC_ERR_COMM);
        }

        match self.send_request(&mut task, need_flush).await {
            Err(e) => {
                logger_warn!(self.logger(), "{:?} sending task {} failed: {:?}", self, task, e);
                timer.pending_task_count_ref().fetch_sub(1, Ordering::SeqCst); // rollback
                self.factory.error_handle(task, e.clone());
                self.closed.store(true, Ordering::SeqCst);
                self.has_err.store(true, Ordering::SeqCst);
                timer.stop_reg_task();
                return Err(e);
            }
            Ok(_) => {
                logger_trace!(self.logger(), "{:?} send task {} success", self, task);
                // register task to norifier
                let mut wg: Option<WaitGroupGuard> = None;
                if let Some(throttler) = self.throttler.as_ref() {
                    wg = Some(throttler.add_task());
                }
                timer.reg_task(task, wg).await;
                return Ok(());
            }
        }
    }

    #[inline(always)]
    async fn flush_req(&self) -> Result<(), RpcError> {
        if let Err(e) = self.conn.flush_req().await {
            logger_warn!(self.logger(), "{:?} flush_req flush err: {:?}", self, e);
            self.closed.store(true, Ordering::SeqCst);
            self.has_err.store(true, Ordering::SeqCst);
            let timer = self.get_timer_mut();
            timer.stop_reg_task();
            return Err(e);
        }
        Ok(())
    }

    #[inline(always)]
    async fn send_request(&self, task: &mut F::Task, need_flush: bool) -> Result<(), RpcError> {
        let seq = self.seq_update();
        task.set_seq(seq);
        match proto::ReqHead::encode(&self.codec, self.client_id, task) {
            Err(_) => {
                logger_warn!(self.logger(), "{:?} send_req encode req {} err", self, task);
                return Err(RPC_ERR_ENCODE);
            }
            Ok((header, action_str, msg_buf, blob_buf)) => {
                let header_bytes = header.as_bytes();
                if let Err(e) = self
                    .conn
                    .write_task(need_flush, header_bytes, action_str, msg_buf.as_bytes(), blob_buf)
                    .await
                {
                    logger_warn!(
                        self.logger(),
                        "{:?} send_req write req {} err: {:?}",
                        self,
                        task,
                        e
                    );
                    self.closed.store(true, Ordering::SeqCst);
                    self.has_err.store(true, Ordering::SeqCst);
                    let timer = self.get_timer_mut();
                    // TODO check stop_reg_task
                    timer.stop_reg_task();
                    return Err(RPC_ERR_COMM);
                }
                return Ok(());
            }
        }
    }

    #[inline(always)]
    async fn send_ping_req(&self) -> Result<(), RpcError> {
        if self.closed.load(Ordering::Acquire) {
            logger_warn!(self.logger(), "{:?} send_ping_req skip as conn closed", self);
            return Err(RPC_ERR_CLOSED);
        }
        // encode response header
        let header = proto::ReqHead {
            magic: proto::RPC_MAGIC,
            seq: self.seq_update(),
            client_id: self.client_id,
            ver: 1,
            format: 0,
            action: proto::PING_ACTION,
            msg_len: 0 as u32,
            blob_len: 0 as u32,
        };
        // Ping does not need to reg_task, and have no error_handle, just to keep the connection
        // alive. Connection Prober can monitor the liveness of ClientConn
        if let Err(e) = self.conn.write_task(true, header.as_bytes(), None, b"", None).await {
            logger_warn!(self.logger(), "{:?} send ping err: {:?}", self, e);
            self.closed.store(true, Ordering::SeqCst);
            return Err(RPC_ERR_COMM);
        }
        Ok(())
    }

    // return Ok(false) when close_rx has close and nothing more pending resp to receive
    async fn recv_some(&self) -> Result<(), RpcError> {
        for _ in 0i32..20 {
            // Underlayer rpc socket is buffered, might not yeal to runtime
            // return if recv_one_resp runs too long, allow timer to be fire at each second
            match self.recv_one_resp().await {
                Err(e) => {
                    //if e == RPC_ERR_CLOSED {
                    //    return Ok(false);
                    //} else {
                    return Err(e);
                    //}
                }
                Ok(_) => {
                    if let Some(last_resp_ts) = self.last_resp_ts.as_ref() {
                        last_resp_ts.store(DelayedTime::get(), Ordering::Relaxed);
                    }
                }
            }
        }
        Ok(())
    }

    async fn recv_one_resp(&self) -> Result<(), RpcError> {
        let timer = self.get_timer_mut();
        loop {
            if self.closed.load(Ordering::Acquire) {
                // ensure task receive on normal exit
                if timer.check_pending_tasks_empty() || self.has_err.load(Ordering::Relaxed) {
                    return Err(RPC_ERR_CLOSED);
                }
                if let Err(e) = self.conn.recv_task(&self.factory, &self.codec, None, timer).await {
                    self.closed.store(true, Ordering::SeqCst);
                    return Err(e);
                }
            } else {
                // Block here for new header without timeout
                if !self
                    .conn
                    .recv_task(&self.factory, &self.codec, Some(&self.close_rx), timer)
                    .await?
                {
                    self.closed.store(true, Ordering::SeqCst);
                    continue;
                }
            }
        }
    }

    async fn receive_loop(&self) {
        let mut tick = <F::IO as AsyncIO>::tick(Duration::from_secs(1));
        loop {
            let f = self.recv_some();
            pin_mut!(f);
            let selector = ReciverTimerFuture::new(self, &mut tick, &mut f);
            match selector.await {
                Ok(_) => {}
                Err(e) => {
                    logger_debug!(self.logger(), "{:?} receive_loop error: {:?}", self, e);
                    self.closed.store(true, Ordering::SeqCst);
                    let timer = self.get_timer_mut();
                    timer.clean_pending_tasks(self.factory.as_ref());
                    // If pending_task_count > 0 means some tasks may still remain in the pending chan
                    while timer.pending_task_count_ref().load(Ordering::SeqCst) > 0 {
                        // After the 'closed' flag has taken effect,
                        // pending_task_count will not keep growing,
                        // so there is no need to sleep here.
                        timer.clean_pending_tasks(self.factory.as_ref());
                        <F::IO as AsyncIO>::sleep(Duration::from_secs(1)).await;
                    }
                    return;
                }
            }
        }
    }

    // Adjust the waiting queue
    fn time_reach(&self) {
        if let Some(throttler) = self.throttler.as_ref() {
            logger_trace!(
                self.logger(),
                "{:?} has {} pending_tasks",
                self,
                throttler.get_inflight_tasks_count()
            );
        }
        let timer = self.get_timer_mut();
        timer.adjust_task_queue(self.factory.as_ref());
        return;
    }

    #[inline(always)]
    fn seq_update(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst)
    }
}

impl<F: ClientFactory> Drop for RpcClientInner<F> {
    fn drop(&mut self) {
        let timer = self.get_timer_mut();
        timer.clean_pending_tasks(self.factory.as_ref());
    }
}

struct ReciverTimerFuture<'a, F, P>
where
    F: ClientFactory,
    P: Future<Output = Result<(), RpcError>> + Unpin,
{
    client: &'a RpcClientInner<F>,
    inv: Pin<&'a mut <F::IO as AsyncIO>::Interval>,
    recv_future: Pin<&'a mut P>,
}

impl<'a, F, P> ReciverTimerFuture<'a, F, P>
where
    F: ClientFactory,
    P: Future<Output = Result<(), RpcError>> + Unpin,
{
    fn new(
        client: &'a RpcClientInner<F>, inv: &'a mut <F::IO as AsyncIO>::Interval,
        recv_future: &'a mut P,
    ) -> Self {
        Self { inv: Pin::new(inv), client, recv_future: Pin::new(recv_future) }
    }
}

// Return Ok(true) to indicate Ok
// Return Ok(false) when client sender has close normally
// Err(e) when connection error
impl<'a, F, P> Future for ReciverTimerFuture<'a, F, P>
where
    F: ClientFactory,
    P: Future<Output = Result<(), RpcError>> + Unpin,
{
    type Output = Result<(), RpcError>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let mut _self = self.get_mut();
        //let _inv = Pin::new(<<_self.inv as F::IO as AsyncIO>::Interval as TimeInterval>);
        // In case ticker not fire, and ensure ticker schedule after ready
        while let Poll::Ready(_) = _self.inv.as_mut().poll_tick(ctx) {
            _self.client.time_reach();
        }
        if _self.client.has_err.load(Ordering::Relaxed) {
            // When sentinel detect peer unreachable, recv_some mighe blocked, at least inv will
            // wait us, just exit
            return Poll::Ready(Err(RPC_ERR_CLOSED));
        }
        _self.client.get_timer_mut().poll_sent_task(ctx);
        // Even if receive future has block, we should poll_sent_task in order to detect timeout event
        if let Poll::Ready(r) = _self.recv_future.as_mut().poll(ctx) {
            return Poll::Ready(r);
        }
        return Poll::Pending;
    }
}
