pub use super::client_impl::RpcClient;
pub use super::client_timer::RpcClientTaskTimer;
use super::{RpcAction, TaskCommon};
use crate::buffer::AllocateBuf;
use crate::codec::Codec;
use crate::transport::*;
use crate::*;
use captains_log::LogFilter;
use crossfire::MAsyncRx;
use std::fmt::{Debug, Display};
use std::future::Future;
use std::io;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, atomic::AtomicU64};

pub trait ClientFactory: Send + Sync + Sized + 'static {
    type Codec: Codec;

    type Task: RpcClientTask;

    type Logger: Deref<Target = LogFilter>;

    type Transport: Transport<Self>;

    fn new_logger(client_id: u64, server_id: u64) -> Self::Logger;

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

    #[inline(always)]
    fn client_connect(
        self: Arc<Self>, addr: &str, server_id: u64, last_resp_ts: Option<Arc<AtomicU64>>,
    ) -> impl Future<Output = Result<RpcClient<Self>, RpcError>> + Send {
        async move {
            let timeout = self.get_config().timeout.connect_timeout;
            let conn = <Self::Transport as Transport<Self>>::connect(addr, timeout).await?;
            Ok(RpcClient::new(self, server_id, conn, last_resp_ts))
        }
    }
}

pub trait ClientTransport<F: ClientFactory>: Debug + Send + 'static {
    fn get_logger(&self) -> &F::Logger;

    fn close(&self) -> impl Future<Output = ()> + Send;

    fn flush_req(&self) -> impl Future<Output = Result<(), RpcError>>;

    fn write_task<'a>(
        &'a self, need_flush: bool, header: &'a [u8], action_str: Option<&'a [u8]>,
        msg_buf: &'a [u8], blob: Option<&'a [u8]>,
    ) -> impl Future<Output = io::Result<()>> + Send;

    fn recv_task(
        &self, factory: &F, codec: &F::Codec, close_ch: Option<&MAsyncRx<()>>,
        task_reg: &mut RpcClientTaskTimer<F>,
    ) -> impl std::future::Future<Output = Result<bool, RpcError>> + Send;
}

pub trait RpcClientTask:
    ClientTaskEncode
    + ClientTaskDecode
    + DerefMut<Target = TaskCommon>
    + Send
    + Sized
    + 'static
    + Display
    + Unpin
{
    fn action<'a>(&'a self) -> RpcAction<'a>;

    fn set_result(self, res: Result<(), RpcError>);
}

pub trait ClientTaskEncode {
    /// Return a sererialized msg of the request.
    fn encode_req<C: Codec>(&self, codec: &C) -> Result<Vec<u8>, ()>;

    #[inline(always)]
    /// Contain optional extra data to send to server side.
    fn get_req_blob(&self) -> Option<&[u8]> {
        None
    }
}

pub trait ClientTaskDecode {
    fn decode_resp<C: Codec>(&mut self, codec: &C, buf: &[u8]) -> Result<(), ()>;

    #[inline(always)]
    fn get_resp_blob_mut(&mut self) -> Option<&mut impl AllocateBuf> {
        None::<&mut Option<Vec<u8>>>
    }
}
