use std::{fmt, future::Future, io, ops::Deref, sync::Arc};

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
        &self, seq: u64, res: Result<(&'a [u8], Option<Buffer>), RpcError>,
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
    pub msg: Option<&'a [u8]>,
    pub blob: Option<Buffer>, // for write, this contains data
}

pub trait ServerHandle<F: ServerFactory> {
    /// It's expect to spawn coroutine to process
    fn run(conn: F::Transport, factory: &F, server_close_rx: MAsyncRx<()>);
}

pub trait TaskDispatcher {
    /// Define the RPC task from server-side
    ///
    /// Either one RpcClientTask or an enum of multiple RpcClientTask.
    /// If you have multiple task type, recommend to use the `enum_dispatch` crate.
    ///
    /// You can use [crate::macros] on task type
    type Task: RpcServerTask;

    /// Define the task handler, called from connection reader coroutine.
    ///
    /// You might dispatch them to a worker pool.
    /// If you processing them directly in the connection coroutine, should make sure not
    /// blocking the thread for long.
    fn dispatch_task(&self, task: Self::Task);
}

/// A writer channel to send reponse. Can be clone anywhere.
#[derive(Clone)]
pub struct RpcRespNoti<T: RpcServerTask>(pub(crate) MTx<T>);

impl<T: RpcServerTask> RpcRespNoti<T> {
    #[inline]
    pub fn done(self, task: T) {
        let _ = self.0.send(task);
    }
}

pub trait RpcServerTask:
    ServerTaskDecode
    + ServerTaskEncode
    + Deref<Target = TaskCommon>
    + Send
    + Sized
    + fmt::Debug
    + Unpin
    + 'static
{
    fn set_result(self, res: Result<(), RpcError>);
}

pub trait ServerTaskDecode: Sized + 'static {
    fn decode_req<C: Codec, T: RpcServerTask>(
        codec: &C, seq: u64, req: &[u8], blob: Option<Buffer>, noti: RpcRespNoti<T>,
    ) -> Result<Self, ()>;
}

pub trait ServerTaskEncode {
    fn get_result(&self) -> Result<(), &RpcError>;

    fn encode_resp<C: Codec>(&self, codec: &C) -> Result<Vec<u8>, ()>;
}
