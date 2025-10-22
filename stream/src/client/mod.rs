//! The module contains traits defined for the client-side

pub use occams_rpc_core::ClientConfig;

pub mod task;
use task::{ClientTask, ClientTaskDone};

pub mod stream;
pub mod timer;
use timer::ClientTaskTimer;

mod pool;
pub use pool::ClientPool;

mod throttler;

use captains_log::filter::Filter;
use crossfire::MAsyncRx;
use occams_rpc_core::{Codec, error::RpcIntErr, runtime::AsyncIO};
use std::future::Future;
use std::sync::Arc;
use std::{fmt, io};

/// A trait implemented by the user for the client-side, to define the customizable plugin.
pub trait ClientFactory: Send + Sync + Sized + 'static {
    /// A [captains-log::filter::Filter](https://docs.rs/captains-log/latest/captains_log/filter/index.html) implementation
    /// The type that new_logger returns.
    ///
    /// maybe a `Arc<LogFilter> or KeyFilter<Arc<LogFilter>>`
    type Logger: Filter + Send + Sync + 'static;

    /// Define the codec to serialization and deserialization
    ///
    /// Refers to [occams_rpc_core::Codec](https://docs.rs/occams-rpc-core/latest/occams_rpc_core/trait.Codec.html)
    type Codec: Codec;

    /// Define the RPC task from client-side
    ///
    /// Either one ClientTask or an enum of multiple ClientTask.
    /// If you have multiple task type, recommend to use the `enum_dispatch` crate.
    ///
    /// You can use macro [client_task_enum](crate::client::task::client_task_enum) and [client_task](crate::client::task::client_task) on task type
    type Task: ClientTask;

    /// Define the transport layer protocol
    ///
    /// Refers to [ClientTransport]
    type Transport: ClientTransport<Self>;

    /// Define the adaptor of async runtime
    ///
    /// Refers to [occams_rpc_core::runtime::AsyncIO](https://docs.rs/occams-rpc-core/latest/occams_rpc_core/runtime/index.html)
    type IO: AsyncIO;

    /// Define how the async runtime spawn a task
    ///
    /// You may spawn with globally runtime, or to a owned runtime executor
    fn spawn_detach<F, R>(&self, f: F)
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static;

    /// You should keep clientConfig inside ServerFactory, get_config() will return the reference.
    fn get_config(&self) -> &ClientConfig;

    /// Construct a [captains_log::filter::Filter](https://docs.rs/captains-log/latest/captains_log/filter/trait.Filter.html) to oganize log of a client
    ///
    fn new_logger(&self, conn_id: &str) -> Self::Logger;
    /// TODO Fix the logger interface

    /// How to deal with error
    ///
    /// You can overwrite this to implement retry logic
    #[inline(always)]
    fn error_handle(&self, task: Self::Task) {
        task.done();
    }

    /// You can overwrite this to assign a client_id
    #[inline(always)]
    fn get_client_id(&self) -> u64 {
        0
    }
}

pub trait ClientCaller<F: ClientFactory>: Send {
    fn send_req(&self, task: F::Task) -> impl Future<Output = ()> + Send;
}

pub trait ClientCallerBlocking<F: ClientFactory>: Send {
    fn send_req_blocking(&self, task: F::Task);
}

impl<F: ClientFactory, C: ClientCaller<F> + Send + Sync> ClientCaller<F> for Arc<C> {
    #[inline(always)]
    async fn send_req(&self, task: F::Task) {
        self.as_ref().send_req(task).await
    }
}

impl<F: ClientFactory, C: ClientCallerBlocking<F> + Send + Sync> ClientCallerBlocking<F>
    for Arc<C>
{
    #[inline(always)]
    fn send_req_blocking(&self, task: F::Task) {
        self.as_ref().send_req_blocking(task);
    }
}

/// This trait is for client-side transport layer protocol.
///
/// The implementation can be found on:
///
/// - [occams-rpc-tcp](https://docs.rs/occams-rpc-tcp): For TCP and Unix socket
pub trait ClientTransport<F: ClientFactory>: fmt::Debug + Send + Sized + 'static {
    /// How to establish an async connection.
    ///
    /// conn_id: used for log fmt, can by the same of addr.
    fn connect(
        addr: &str, conn_id: &str, config: &ClientConfig, logger: F::Logger,
    ) -> impl Future<Output = Result<Self, RpcIntErr>> + Send;

    /// The ClientTransport holds a logger, the server will use it by reference.
    fn get_logger(&self) -> &F::Logger;

    /// Shutdown the write direction of the connection
    fn close_conn(&self) -> impl Future<Output = ()> + Send;

    /// Flush the request for the socket writer, if the transport has buffering logic
    fn flush_req(&self) -> impl Future<Output = io::Result<()>> + Send;

    /// Write out the encoded request task
    fn write_req<'a>(
        &'a self, buf: &'a [u8], blob: Option<&'a [u8]>, need_flush: bool,
    ) -> impl Future<Output = io::Result<()>> + Send;

    /// Read the response and decode it from the socket, find and notify the registered ClientTask
    fn read_resp(
        &self, factory: &F, codec: &F::Codec, close_ch: Option<&MAsyncRx<()>>,
        task_reg: &mut ClientTaskTimer<F>,
    ) -> impl std::future::Future<Output = Result<bool, RpcIntErr>> + Send;
}
