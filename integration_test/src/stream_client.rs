use crossfire::*;
use log::*;
use occams_rpc::codec::MsgpCodec;
use occams_rpc::stream::client::*;
use occams_rpc::stream::*;

use captains_log::filter::LogFilter;
use io_buffer::Buffer;
use occams_rpc::macros::*;
use occams_rpc::*;
use serde_derive::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

pub struct FileClient {
    config: RpcConfig,
}

impl ClientFactory for FileClient {
    type Codec = MsgpCodec;

    type Task = FileClientTask;

    type Logger = captains_log::filter::LogFilter;

    type Transport = occams_rpc_tcp::TcpClient<Self>;

    #[cfg(feature = "tokio")]
    type IO = occams_rpc_tokio::TokioRT;
    #[cfg(not(feature = "tokio"))]
    type IO = occams_rpc_smol::SmolRT;

    #[inline]
    fn spawn_detach<F, R>(&self, f: F)
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static,
    {
        #[cfg(feature = "tokio")]
        {
            let _ = tokio::spawn(f);
        }
        #[cfg(not(feature = "tokio"))]
        {
            let _ = smol::spawn(f);
        }
    }

    #[inline]
    fn new_logger(&self, client_id: u64, server_id: u64) -> Self::Logger {
        // TODO fixme
        LogFilter::new()
    }

    #[inline]
    fn get_config(&self) -> &RpcConfig {
        &self.config
    }
}

#[derive(PartialEq, Debug)]
#[repr(u8)]
pub enum FileAction {
    Open = 1,
    Read = 2,
    Write = 3,
}

#[client_task_enum]
pub enum FileClientTask {
    Open(FileClientTaskOpen),
    Read(FileClientTaskRead),
    Write(FileClientTaskWrite),
}

macro_rules! impl_client_task {
    ($cls: path) => {
        impl ClientTaskDone for $cls {
            fn set_result(self, res: Result<(), RpcError>) {
                self.res.replace(res);
                self.sender.send(self.into());
            }
        }

        impl fmt::Debug for $cls {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{} seq={} {}", $cls, self.common.seq, self.req)
            }
        }
    };
}

#[derive(Default, Deserialize, Serialize, Debug)]
pub struct FileOpenReq {
    pub path: String,
}

#[client_task(action=FileAction::Open)]
pub struct FileClientTaskOpen {
    #[field(common)]
    common: ClientTaskCommon,
    #[field(req)]
    req: FileOpenReq,
    #[field(resp)]
    resp: (),
    res: Option<Result<(), RpcError>>,
    sender: MTx<FileClientTask>,
}

impl_client_task!(FileClientTaskOpen);

impl FileClientTaskOpen {
    pub fn new(sender: MTx<Self>, path: String) -> Self {
        Self { common: Default::default(), sender, req: FileOpenReq { path }, res: None, resp: () }
    }
}

#[derive(Default, Deserialize, Serialize, Debug)]
pub struct FileIOReq {
    pub inode: u64,
    pub offset: i64,
    pub len: usize,
}

#[derive(Default, Deserialize, Serialize, Debug)]
pub struct FileIOResp {
    pub ret_size: u64,
}

#[client_task(action=FileAction::Read)]
pub struct FileClientTaskRead {
    #[field(common)]
    common: ClientTaskCommon,
    #[field(req)]
    req: FileIOReq,
    #[field(resp)]
    resp: Option<FileIOResp>,
    #[field(resp_blob)]
    read_result: Option<Buffer>,
    res: Option<Result<(), RpcError>>,
    sender: MTx<Self>,
}

impl_client_task!(FileClientTaskRead);

impl FileClientTaskRead {
    pub fn new(sender: MTx<Self>, inode: u64, offset: i64, len: usize) -> Self {
        Self {
            common: Default::default(),
            sender,
            res: None,
            req: FileIOReq { inode, offset, len },
            resp: None,
            data: None,
        }
    }
}

#[client_task(action=FileAction::Write)]
pub struct FileClientTaskWrite {
    #[field(common)]
    common: ClientTaskCommon,
    #[field(req)]
    req: FileIOReq,
    #[field(req_blob)]
    data: Buffer,
    #[field(resp)]
    resp: Option<FileIOResp>,
    res: Option<Result<(), RpcError>>,
    sender: MTx<Self>,
}

impl_client_task!(FileClientTaskWrite);

impl FileClientTaskWrite {
    pub fn new(sender: MTx<Self>, inode: u64, offset: i64, data: Buffer) -> Self {
        Self {
            common: Default::default(),
            sender,
            res: None,
            req: FileIOReq { inode, offset, len: data.len() },
            data,
            resp: None,
        }
    }
}
