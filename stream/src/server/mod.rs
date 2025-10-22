//! This module contains traits defined for the server-side
//!

pub mod task;
use task::*;

mod server;
pub use server::RpcServer;

pub mod dispatch;
use dispatch::ReqDispatch;

pub use occams_rpc_core::ServerConfig;

use crate::proto::RpcAction;
use captains_log::filter::Filter;
use io_buffer::Buffer;
use occams_rpc_core::{Codec, error::*, io::*, runtime::AsyncIO};
use std::{fmt, future::Future, io, sync::Arc};

/// A central hub defined by the user for the server-side, to define the customizable plugin.
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

    type RespTask: ServerTaskResp;

    /// Called when a server stream is established, initialize a ReqDispatch for the connection.
    ///
    /// The dispatch is likely to be a closure or object, in order to dispatch tasks to different workers
    fn new_dispatcher(&self) -> impl ReqDispatch<Self::RespTask>;
}

/// This trait is for server-side transport layer protocol.
///
/// The implementation can be found on:
///
/// - [occams-rpc-tcp](https://docs.rs/occams-rpc-tcp): For TCP and Unix socket
pub trait ServerTransport<F: ServerFactory>: Send + Sync + Sized + 'static + fmt::Debug {
    type Listener: AsyncListener;

    /// The ServerTransport holds a logger, the server will use it by reference.
    fn get_logger(&self) -> &F::Logger;

    /// The implementation is expected to store the conn_count until dropped
    fn new_conn(
        stream: <Self::Listener as AsyncListener>::Conn, f: &F, conn_count: Arc<()>,
    ) -> Self;

    /// Read a request from the socket
    fn read_req<'a>(
        &'a self, close_ch: &crossfire::MAsyncRx<()>,
    ) -> impl Future<Output = Result<RpcSvrReq<'a>, RpcIntErr>> + Send;

    /// Write our user task response
    fn write_resp<C: Codec, T: ServerTaskEncode>(
        &self, codec: &C, task: T,
    ) -> impl Future<Output = io::Result<()>> + Send;

    /// Write out ping resp or error
    fn write_resp_internal(
        &self, seq: u64, err: Option<RpcIntErr>,
    ) -> impl Future<Output = io::Result<()>> + Send;

    /// Flush the response for the socket writer, if the transport has buffering logic
    fn flush_resp(&self) -> impl Future<Output = io::Result<()>> + Send;

    /// Shutdown the write direction of the connection
    fn close_conn(&self) -> impl Future<Output = ()> + Send;
}

/// A temporary struct to hold data buffer return by ServerTransport
///
/// NOTE: `RpcAction` and `msg` contains slice that reference to ServerTransport's internal buffer,
/// you should parse and clone them.
pub struct RpcSvrReq<'a> {
    pub seq: u64,
    pub action: RpcAction<'a>,
    pub msg: &'a [u8],
    pub blob: Option<Buffer>, // for write, this contains data
}

impl<'a> fmt::Debug for RpcSvrReq<'a> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "req(seq={}, action={:?})", self.seq, self.action)
    }
}

/// A Struct to hold pre encoded buffer for server response
#[allow(dead_code)]
#[derive(Debug)]
pub struct RpcSvrResp {
    pub seq: u64,

    pub msg: Option<Vec<u8>>,

    pub blob: Option<Buffer>,

    pub res: Option<Result<(), EncodedErr>>,
}

impl task::ServerTaskEncode for RpcSvrResp {
    #[inline]
    fn encode_resp<'a, 'b, C: Codec>(
        &'a mut self, _codec: &'b C, buf: &'b mut Vec<u8>,
    ) -> (u64, Result<(usize, Option<&'a [u8]>), EncodedErr>) {
        match self.res.take().unwrap() {
            Ok(_) => {
                if let Some(msg) = self.msg.as_ref() {
                    use std::io::Write;
                    buf.write_all(&msg).expect("fill msg");
                    return (self.seq, Ok((msg.len(), self.blob.as_deref())));
                } else {
                    return (self.seq, Ok((0, self.blob.as_deref())));
                }
            }
            Err(e) => return (self.seq, Err(e)),
        }
    }
}
