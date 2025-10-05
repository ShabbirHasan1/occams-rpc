use std::{fmt, future::Future, io, sync::Arc};

use super::*;
use crate::codec::Codec;
use crate::config::RpcConfig;
use crate::error::*;
use crate::io::*;
use crate::runtime::*;
use captains_log::filter::Filter;
use crossfire::*;
use io_buffer::Buffer;

pub trait ServerFactory: Clone + Sync + Send + 'static + Sized {
    /// Define the codec to serialization and deserialization
    ///
    /// Refers to [crate::codec]
    type Codec: Codec;

    /// A [captains-log::filter::Filter](https://docs.rs/captains-log/latest/captains_log/filter/index.html) implementation
    type Logger: Filter + Send;

    /// Define the transport layer protocol
    ///
    /// Refers to [ServerTransport]
    type Transport: ServerTransport<Self>;

    /// Define the adaptor of async runtime
    ///
    /// Refers to [crate::runtime]
    type IO: AsyncIO;

    fn get_config(&self) -> &RpcConfig;

    /// Construct a logger filter to oganize log of a client
    fn new_logger(&self) -> Self::Logger;

    /// Define how the server connection is served
    type ConnHandle: ServerHandle<Self>;

    fn get_dispatcher<R: RpcServerTaskResp>(&self) -> impl TaskDispatcher<R>;

    /// Define how the async runtime spawn a task
    ///
    /// You may spawn globally, or to a specified runtime executor
    fn spawn_detach<F, R>(&self, f: F)
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static;
}

pub trait ServerTransport<F: ServerFactory>: Send + Sync + Sized + 'static + fmt::Debug {
    type Listener: AsyncListener;

    fn get_logger(&self) -> &F::Logger;

    /// The implementation is expected to store the conn_count until dropped
    fn new_conn(
        stream: <Self::Listener as AsyncListener>::Conn, f: &F, conn_count: Arc<()>,
    ) -> Self;

    fn recv_req<'a>(
        &'a self, close_ch: &crossfire::MAsyncRx<()>,
    ) -> impl Future<Output = Result<RpcSvrReq<'a>, RpcError>> + Send;

    fn send_resp<'a>(
        &self, seq: u64, res: Result<(Vec<u8>, Option<&'a Buffer>), &'a RpcError>,
    ) -> impl Future<Output = io::Result<()>> + Send;

    fn flush_resp(&self) -> impl Future<Output = io::Result<()>> + Send;

    fn close_conn(&self) -> impl Future<Output = ()> + Send;
}

/// A temporary struct to hold data buffer return by ServerTransport
///
/// NOTE: `RpcAction` and `msg` contains slice that reference to ServerTransport's internal buffer,
/// you should parse and clone them.
#[derive(Debug)]
pub struct RpcSvrReq<'a> {
    pub seq: u64,
    pub action: RpcAction<'a>,
    pub msg: &'a [u8],
    pub blob: Option<Buffer>, // for write, this contains data
}

pub trait ServerHandle<F: ServerFactory> {
    /// It's expect to spawn coroutine to process
    fn run(conn: F::Transport, factory: &F, server_close_rx: MAsyncRx<()>);
}

pub trait TaskDispatcher<R: RpcServerTaskResp>: Send + Sync + Sized + 'static {
    /// Define the RPC task from server-side
    ///
    /// Either one or an enum of multiple RpcServerTask.
    /// If you have multiple task type, recommend to use the `enum_dispatch` crate.
    ///
    /// You can use [crate::macros] on task type
    type Task: RpcServerTaskReq<R>;

    /// Define the task handler, called from connection reader coroutine.
    ///
    /// You might dispatch them to a worker pool.
    /// If you processing them directly in the connection coroutine, should make sure not
    /// blocking the thread for long.
    fn dispatch_task(&self, task: Self::Task) -> impl Future<Output = ()> + Send;
}

/// A writer channel to send reponse. Can be clone anywhere.
#[derive(Clone)]
pub struct RpcRespNoti<T>(pub(crate) crossfire::MTx<Result<T, (u64, Option<RpcError>)>>);

impl<T: RpcServerTaskResp> RpcRespNoti<T> {
    #[inline]
    pub fn done(self, task: T) {
        let _ = self.0.send(Ok(task));
    }
}

/// For enum_dispatch
pub trait RpcServerTaskReq<R: RpcServerTaskResp>:
    for<'a> ServerTaskDecode<'a, R> + Send + Sized + fmt::Debug + Unpin + 'static
{
}

/// For enum_dispatch
pub trait RpcServerTaskResp:
    ServerTaskEncode + Send + Sized + fmt::Debug + Unpin + 'static
{
}

pub trait ServerTaskDecode<'a, T>: Sized + 'static {
    fn decode_req<C: Codec>(
        codec: &'a C, action: RpcAction<'a>, seq: u64, req: &'a [u8], blob: Option<Buffer>,
        noti: RpcRespNoti<T>,
    ) -> Result<Self, ()>;
}

pub trait ServerTaskEncode {
    fn encode_resp<'a, C: Codec>(
        &'a self, codec: &'a C,
    ) -> Result<(u64, Result<(Vec<u8>, Option<&'a Buffer>), &'a RpcError>), u64>;
}

pub trait ServerTaskDone<T: RpcServerTaskResp> {
    fn set_result(self, res: Result<(), RpcError>) -> RpcRespNoti<T>;
}
