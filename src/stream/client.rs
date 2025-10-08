use super::RpcAction;
pub use super::client_impl::RpcClient;
pub use super::client_timer::ClientTaskTimer;
use crate::codec::Codec;
use crate::io::AllocateBuf;
use crate::runtime::AsyncIO;
use crate::*;
use captains_log::filter::Filter;
use crossfire::MAsyncRx;
use std::fmt;
use std::future::Future;
use std::io;
use std::ops::DerefMut;
use std::sync::{Arc, atomic::AtomicU64};

pub trait ClientFactory: Send + Sync + Sized + 'static {
    /// Define the codec to serialization and deserialization
    ///
    /// Refers to [crate::codec]
    type Codec: Codec;

    /// Define the RPC task from client-side
    ///
    /// Either one ClientTask or an enum of multiple ClientTask.
    /// If you have multiple task type, recommend to use the `enum_dispatch` crate.
    ///
    /// You can use [crate::macros] on task type
    type Task: ClientTask;

    /// A [captains-log::filter::Filter](https://docs.rs/captains-log/latest/captains_log/filter/index.html) implementation
    type Logger: Filter + Send;

    /// Define the transport layer protocol
    ///
    /// Refers to [ClientTransport]
    type Transport: ClientTransport<Self>;

    /// Define the adaptor of async runtime
    ///
    /// Refers to [crate::runtime]
    type IO: AsyncIO;

    /// Define how the async runtime spawn a task
    ///
    /// You may spawn globally, or to a specified runtime executor
    fn spawn_detach<F, R>(&self, f: F)
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static;

    /// Construct a logger filter to oganize log of a client
    fn new_logger(&self, client_id: u64, server_id: u64) -> Self::Logger;

    /// How to deal with RpcError
    ///
    /// You can overwrite this to implement retry logic
    fn error_handle(&self, task: Self::Task, err: RpcError) {
        task.set_result(Err(err));
    }

    /// You can overwrite this to assign a client_id
    #[inline(always)]
    fn get_client_id(&self) -> u64 {
        0
    }

    fn get_config(&self) -> &RpcConfig;

    /// Make a streaming connection to the server, returns [RpcClient] on success
    #[inline(always)]
    fn client_connect(
        self: Arc<Self>, addr: &str, server_id: u64, last_resp_ts: Option<Arc<AtomicU64>>,
    ) -> impl Future<Output = Result<RpcClient<Self>, RpcError>> + Send {
        async move {
            let client_id = self.get_client_id();
            let timeout = &self.get_config().timeout;
            let logger = self.new_logger(client_id, server_id);
            let conn = <Self::Transport as ClientTransport<Self>>::connect(
                addr, timeout, client_id, server_id, logger,
            )
            .await?;
            Ok(RpcClient::new(self, conn, client_id, server_id, last_resp_ts))
        }
    }
}

/// A ClientTransport implements network transport layer protocol
///
/// Current avaiable transport crate:
///
/// - TCP/unix transport: [occams-rpc-tcp](https://docs.rs/occams-rpc-tcp)
pub trait ClientTransport<F: ClientFactory>: fmt::Debug + Send + Sized + 'static {
    fn connect(
        addr: &str, timeout: &TimeoutSetting, client_id: u64, server_id: u64, logger: F::Logger,
    ) -> impl Future<Output = Result<Self, RpcError>> + Send;

    fn get_logger(&self) -> &F::Logger;

    fn close(&self) -> impl Future<Output = ()> + Send;

    fn flush_req(&self) -> impl Future<Output = Result<(), RpcError>>;

    fn write_task<'a>(
        &'a self, need_flush: bool, header: &'a [u8], action_str: Option<&'a [u8]>,
        msg_buf: &'a [u8], blob: Option<&'a [u8]>,
    ) -> impl Future<Output = io::Result<()>> + Send;

    fn recv_task(
        &self, factory: &F, codec: &F::Codec, close_ch: Option<&MAsyncRx<()>>,
        task_reg: &mut ClientTaskTimer<F>,
    ) -> impl std::future::Future<Output = Result<bool, RpcError>> + Send;
}

pub trait ClientTask:
    ClientTaskAction
    + ClientTaskEncode
    + ClientTaskDecode
    + DerefMut<Target = ClientTaskCommon>
    + Send
    + Sized
    + 'static
    + fmt::Debug
    + Unpin
{
    fn set_result(self, res: Result<(), RpcError>);
}

/// Encode the request to buffer that can be send to server
pub trait ClientTaskEncode {
    /// Return a sererialized msg of the request.
    fn encode_req<C: Codec>(&self, codec: &C) -> Result<Vec<u8>, ()>;

    #[inline(always)]
    /// Contain optional extra data to send to server side.
    fn get_req_blob(&self) -> Option<&[u8]> {
        None
    }
}

/// Decode the response from server and assign to the task struct
pub trait ClientTaskDecode {
    fn decode_resp<C: Codec>(&mut self, codec: &C, buf: &[u8]) -> Result<(), ()>;

    #[inline(always)]
    fn get_resp_blob_mut(&mut self) -> Option<&mut impl AllocateBuf> {
        None::<&mut Option<Vec<u8>>>
    }
}

pub trait ClientTaskAction {
    fn action<'a>(&'a self) -> RpcAction<'a>;
}

#[derive(Debug, Default)]
pub struct ClientTaskCommon {
    pub seq: u64,
}

impl ClientTaskCommon {
    pub fn seq(&self) -> u64 {
        self.seq
    }
    pub fn set_seq(&mut self, seq: u64) {
        self.seq = seq;
    }
}
