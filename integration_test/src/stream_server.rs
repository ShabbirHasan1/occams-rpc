use crossfire::*;
use crossfire::*;
use io_buffer::Buffer;
use log::*;
use occams_rpc::codec::MsgpCodec;
use occams_rpc::stream::server::*;
use occams_rpc::stream::server_impl::*;
use occams_rpc::stream::*;

use occams_rpc::macros::*;
use occams_rpc::*;
use serde_derive::{Deserialize, Serialize};
use std::fmt;

use super::stream_client::{FileAction, FileIOReq, FileIOResp, FileOpenReq};
use captains_log::filter::LogFilter;

pub struct FileServer {
    config: RpcConfig,
}

impl ServerFactory for FileServer {
    type Logger = captains_log::filter::LogFilter;

    type Transport = occams_rpc_tcp::TcpServer<Self>;

    #[cfg(feature = "tokio")]
    type IO = occams_rpc_tokio::TokioRT;
    #[cfg(not(feature = "tokio"))]
    type IO = occams_rpc_smol::SmolRT;

    type RespReceiver = RespReceiverTask<FileServerTask>;

    fn new_dispatcher(&self) -> impl ReqDispatch<Self::RespReceiver> {
        async fn dispatch(task: FileServerTask) -> Result<(), ()> {
            todo!();
        }
        return TaskReqDispatch::<MsgpCodec, FileServerTask, Self::RespReceiver, _, _>::new(
            dispatch,
        );
    }

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
    fn new_logger(&self) -> Self::Logger {
        // TODO fixme
        LogFilter::new()
    }

    #[inline]
    fn get_config(&self) -> &RpcConfig {
        &self.config
    }
}

#[server_task_enum(req, resp)]
pub enum FileServerTask {
    #[action(FileAction::Open)]
    Open(ServerTaskOpen),
    #[action(FileAction::Read, FileAction::Write)]
    IO(ServerTaskIO),
}

pub type ServerTaskOpen = ServerTaskVariantFull<FileServerTask, FileOpenReq, ()>;
pub type ServerTaskIO = ServerTaskVariantFull<FileServerTask, FileIOReq, FileIOResp>;
