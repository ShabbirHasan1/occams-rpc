use crossfire::*;
use occams_rpc::codec::MsgpCodec;
use occams_rpc::stream::client::*;

use captains_log::filter::LogFilter;
use io_buffer::Buffer;
use occams_rpc::macros::*;
use occams_rpc::*;
use serde_derive::{Deserialize, Serialize};
use std::sync::{Arc, atomic::AtomicU64}; // Re-added

pub struct FileClient {
    config: RpcConfig,
}

impl FileClient {
    pub fn new(config: RpcConfig) -> Self {
        Self { config }
    }
}

pub async fn init_client(
    config: RpcConfig, addr: &str, last_resp_ts: Option<Arc<AtomicU64>>,
) -> Result<RpcClient<FileClient>, RpcError> {
    let factory = Arc::new(FileClient::new(config));
    RpcClient::connect(factory, addr, 0, last_resp_ts).await
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
            let _ = smol::spawn(f).detach();
        }
    }

    #[inline]
    fn new_logger(&self, _client_id: u64, _server_id: u64) -> Self::Logger {
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

impl TryFrom<u8> for FileAction {
    type Error = RpcError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(FileAction::Open),
            2 => Ok(FileAction::Read),
            3 => Ok(FileAction::Write),
            _ => Err(RpcError::Text(format!("Invalid FileAction value: {}", value))),
        }
    }
}

#[derive(Debug)]
#[client_task_enum]
pub enum FileClientTask {
    #[action(FileAction::Open)]
    Open(FileClientTaskOpen),
    #[action(FileAction::Read)]
    Read(FileClientTaskRead),
    #[action(FileAction::Write)]
    Write(FileClientTaskWrite),
}

#[derive(Default, Deserialize, Serialize, Debug)]
pub struct FileOpenReq {
    pub path: String,
}

#[client_task(debug)]
pub struct FileClientTaskOpen {
    #[field(common)]
    pub common: ClientTaskCommon,
    #[field(req)]
    pub req: FileOpenReq,
    #[field(resp)]
    pub resp: Option<()>,
    #[field(res)]
    pub res: Option<Result<(), RpcError>>,
    #[field(noti)]
    pub sender: Option<MTx<FileClientTask>>,
}

impl FileClientTaskOpen {
    pub fn new(sender: MTx<FileClientTask>, path: String) -> Self {
        Self {
            common: Default::default(),
            sender: Some(sender),
            req: FileOpenReq { path },
            res: None,
            resp: None,
        }
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

#[client_task(debug)]
pub struct FileClientTaskRead {
    #[field(common)]
    pub common: ClientTaskCommon,
    #[field(req)]
    pub req: FileIOReq,
    #[field(resp)]
    pub resp: Option<FileIOResp>,
    #[field(resp_blob)]
    pub read_data: Option<Buffer>,
    #[field(res)]
    pub res: Option<Result<(), RpcError>>,
    #[field(noti)]
    pub sender: Option<MTx<FileClientTask>>,
}

impl FileClientTaskRead {
    pub fn new(sender: MTx<FileClientTask>, inode: u64, offset: i64, len: usize) -> Self {
        Self {
            common: Default::default(),
            sender: Some(sender),
            res: None,
            req: FileIOReq { inode, offset, len },
            resp: None,
            read_data: None,
        }
    }
}

#[client_task(debug)]
pub struct FileClientTaskWrite {
    #[field(common)]
    pub common: ClientTaskCommon,
    #[field(req)]
    pub req: FileIOReq,
    #[field(req_blob)]
    pub data: Buffer,
    #[field(resp)]
    pub resp: Option<FileIOResp>,
    #[field(res)]
    pub res: Option<Result<(), RpcError>>,
    #[field(noti)]
    pub sender: Option<MTx<FileClientTask>>,
}

impl FileClientTaskWrite {
    pub fn new(sender: MTx<FileClientTask>, inode: u64, offset: i64, data: Buffer) -> Self {
        Self {
            common: Default::default(),
            sender: Some(sender),
            res: None,
            req: FileIOReq { inode, offset, len: data.len() },
            data,
            resp: None,
        }
    }
}
