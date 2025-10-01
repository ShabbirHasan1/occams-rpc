use super::{RpcAction, TaskCommon};
use crate::buffer::AllocateBuf;
use crate::codec::Codec;
use crate::config::RpcConfig;
use crate::error::*;
use captains_log::LogFilter;
use std::fmt::Display;
use std::ops::{Deref, DerefMut};

pub use super::client_impl::RpcClient;

pub trait ClientFactory: Send + Sync + Sized + 'static {
    type Codec: Codec;

    type Task: RpcClientTask;

    type Logger: Deref<Target = LogFilter>;

    fn new_logger(client_id: u64, server_id: u64) -> Self::Logger;

    fn error_handle(&self, task: Self::Task, err: RpcError) {
        task.set_result(Err(err));
    }

    fn get_client_id(&self) -> u64;

    fn get_config(&self) -> &RpcConfig;
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

pub struct RetryTaskInfo<T: RpcClientTask + Send + Unpin + 'static> {
    pub task: T,
    pub task_err: RpcError,
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
