use super::RpcAction;
use crate::codec::Codec;
use crate::error::*;
use io_buffer::Buffer;
use std::fmt::Display;
use std::ops::DerefMut;

#[derive(Debug, Default)]
pub struct TaskCommon {
    pub seq: u64,
}

impl TaskCommon {
    pub fn seq(&self) -> u64 {
        self.seq
    }
    pub fn set_seq(&mut self, seq: u64) {
        self.seq = seq;
    }
}

pub trait RpcClientTask:
    ClientTaskEncode
    + ClientTaskDecode
    + DerefMut<Target = TaskCommon>
    + Send
    + Sync
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

    /// Get a mut buffer ref for large blob to hold optional extra data response from server
    ///
    /// `blob_len` is from RespHead, this interface should make sure that buffer of such size allocated.
    #[inline(always)]
    fn get_resp_ext_buf_mut(&mut self, _blob_len: i32) -> Option<&mut [u8]> {
        None
    }
}

pub trait AllocateBuf {
    /// Alloc buffer or reserve space inside the Buffer
    fn reserve<'a>(&'a mut self, _blob_len: i32) -> Option<&'a mut [u8]>;
}

impl AllocateBuf for Option<Vec<u8>> {
    #[inline]
    fn reserve<'a>(&'a mut self, blob_len: i32) -> Option<&'a mut [u8]> {
        let blob_len = blob_len as usize;
        if let Some(buf) = self.as_mut() {
            if buf.len() != blob_len {
                if buf.capacity() < blob_len {
                    buf.reserve(blob_len - buf.capacity());
                }
                unsafe { buf.set_len(blob_len) };
            }
        } else {
            let mut v = Vec::with_capacity(blob_len);
            unsafe { v.set_len(blob_len) };
            self.replace(v);
        }
        return self.as_deref_mut();
    }
}

impl AllocateBuf for Option<Buffer> {
    #[inline]
    fn reserve<'a>(&'a mut self, blob_len: i32) -> Option<&'a mut [u8]> {
        if let Some(buf) = self.as_mut() {
            let blob_len = blob_len as usize;
            if buf.len() != blob_len {
                if buf.capacity() < blob_len {
                    return None;
                }
                buf.set_len(blob_len);
            }
        } else {
            if let Ok(v) = Buffer::alloc(blob_len) {
                self.replace(v);
            } else {
                // alloc failed
                return None;
            }
        }
        return self.as_deref_mut();
    }
}
