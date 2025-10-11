use std::{fmt, future::Future, io, sync::Arc};

use crate::proto::RpcAction;
use captains_log::filter::Filter;
use crossfire::*;
use io_buffer::Buffer;
pub use occams_rpc_core::ServerConfig;
use occams_rpc_core::{Codec, error::*, io::*, runtime::AsyncIO};

pub trait ServerFactory: Sync + Send + 'static + Sized {
    /// A [captains-log::filter::Filter](https://docs.rs/captains-log/latest/captains_log/filter/index.html) implementation.
    /// The type that new_logger returns.
    ///
    /// maybe a `Arc<LogFilter> or KeyFilter<Arc<LogFilter>>`
    type Logger: Filter + Send + 'static;

    /// Define the transport layer protocol
    ///
    /// Refers to [ServerTransport]
    type Transport: ServerTransport<Self>;

    /// Define the adaptor of async runtime
    ///
    /// Refers to [occams_rpc_core::runtime::AsyncIO](https://docs.rs/occams-rpc-core/latest/occams_rpc_core/runtime/index.html)
    type IO: AsyncIO;

    /// You should keep ServerConfig inside ServerFactory, get_config() will return the reference.
    fn get_config(&self) -> &ServerConfig;

    /// Construct a [captains_log::filter::Filter](https://docs.rs/captains-log/latest/captains_log/filter/trait.Filter.html) to oganize log of a client
    ///
    /// maybe a `Arc<LogFilter>` or `KeyFilter<Arc<LogFilter>>`
    fn new_logger(&self) -> Self::Logger;

    /// Define how the async runtime spawn a task
    ///
    /// You may spawn with globally runtime, or to a owned runtime executor
    fn spawn_detach<F, R>(&self, f: F)
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static;

    /// One of RespReceiver impl:
    /// - [crate::server_impl::RespReceiverTask]
    /// - [crate::server_impl::RespReceiverBuf]
    type RespReceiver: RespReceiver;

    /// The dispatch is likely to be a closure or object, in order to dispatch task to different worker
    fn new_dispatcher(&self) -> impl ReqDispatch<Self::RespReceiver>;
}

/// This trait is for server-side transport layer protocol.
///
/// The implementation can be found on:
///
/// - [occams-rpc-tcp](https://docs.rs/occams-rpc-tcp): For TCP and Unix socket
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

/// A temporary struct to hold data buffer for RespReceiverBuf
#[derive(Debug)]
pub struct RpcSvrResp {
    pub seq: u64,

    pub msg: Option<Vec<u8>>,

    pub blob: Option<Buffer>,

    pub res: Result<(), RpcError>,
}

pub trait ReqDispatch<R: RespReceiver>: Send + Sync + Sized + 'static {
    /// Define the task handler, called from connection reader coroutine.
    ///
    /// You might dispatch them to a worker pool.
    /// If you processing them directly in the connection coroutine, should make sure not
    /// blocking the thread for long.
    /// This in async fn, but you should avoid waiting as must as possible.
    /// Should return Err(()) when codec decode_req failed.
    fn dispatch_req<'a>(
        &'a self, req: RpcSvrReq<'a>, noti: RespNoti<R::ChannelItem>,
    ) -> impl Future<Output = Result<(), ()>> + Send;

    fn encode_resp<'a>(
        &'a self, task: &'a mut R::ChannelItem,
    ) -> (u64, Result<(Vec<u8>, Option<&'a Buffer>), &'a RpcError>);
}

pub trait RespReceiver: Send + 'static {
    type ChannelItem: Send + Unpin + 'static + fmt::Debug;

    /// NOTE: Because the msg encoded from task resp is not a ref, should return a owned buffer.
    ///
    /// In order to return `Vec<u8>` from RpcSvrResp, should take the msg out, thus require task to
    /// by &mut, but this does not affect encoding.
    fn encode_resp<'a, C: Codec>(
        codec: &'a C, task: &'a mut Self::ChannelItem,
    ) -> (u64, Result<(Vec<u8>, Option<&'a Buffer>), &'a RpcError>);
}

/// A writer channel to send response. Can be cloned anywhere.
pub struct RespNoti<T: Send + 'static>(
    pub(crate) crossfire::MTx<Result<T, (u64, Option<RpcError>)>>,
);

impl<T: Send + 'static> Clone for RespNoti<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Send + 'static> RespNoti<T> {
    pub fn new(tx: crossfire::MTx<Result<T, (u64, Option<RpcError>)>>) -> Self {
        Self(tx)
    }

    #[inline]
    pub fn done(self, task: T) {
        let _ = self.0.send(Ok(task));
    }

    #[inline]
    pub(crate) fn send_err(&self, seq: u64, err: Option<RpcError>) -> Result<(), ()> {
        if self.0.send(Err((seq, err))).is_err() { return Err(()) } else { Ok(()) }
    }
}

/// Sum up trait for server response task
pub trait ServerTaskResp: ServerTaskEncode + Send + Sized + Unpin + 'static + fmt::Debug {}

/// How to decode a server request
pub trait ServerTaskDecode<R: Send + Unpin + 'static>: Send + Sized + Unpin + 'static {
    fn decode_req<'a, C: Codec>(
        codec: &'a C, action: RpcAction<'a>, seq: u64, req: &'a [u8], blob: Option<Buffer>,
        noti: RespNoti<R>,
    ) -> Result<Self, ()>;
}

/// How to encode a server response
pub trait ServerTaskEncode {
    fn encode_resp<'a, C: Codec>(
        &'a self, codec: &'a C,
    ) -> (u64, Result<(Vec<u8>, Option<&'a Buffer>), &'a RpcError>);
}

/// How to notify Rpc framework when a task is done
pub trait ServerTaskDone<T: Send + 'static>: Sized + 'static {
    /// Should implement for enum delegation, not intended for user call
    fn _set_result(&mut self, res: Result<(), RpcError>) -> RespNoti<T>;

    /// For users, set the result in the task and send it back
    #[inline]
    fn set_result(mut self, res: Result<(), RpcError>)
    where
        T: std::convert::From<Self>,
    {
        // NOTE: To allow a trait to consume self, must require Sized
        let noti = self._set_result(res);
        let parent: T = self.into();
        noti.done(parent);
    }
}

/// Get RpcAction from a enum task, or a sub-type that fits multiple RpcActions
pub trait ServerTaskAction {
    fn get_action<'a>(&'a self) -> RpcAction<'a>;
}
