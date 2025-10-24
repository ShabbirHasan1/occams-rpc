use super::client::{FileAction, FileIOReq, FileIOResp, FileOpenReq};
use occams_rpc_codec::MsgpCodec;
use occams_rpc_stream::server::{dispatch::*, task::*, *};
use occams_rpc_tcp::TcpServer;
use std::sync::Arc;

use captains_log::filter::LogFilter;
use nix::errno::Errno;

pub fn init_server<H, FH>(
    server_handle: H, config: ServerConfig, addr: &str,
) -> Result<(RpcServer<FileServer<H, FH>>, String), std::io::Error>
where
    H: FnOnce(FileServerTask) -> FH + Send + Sync + 'static + Clone,
    FH: Future<Output = Result<(), ()>> + Send + 'static,
{
    let facts = Arc::new(FileServer::new(server_handle, config));
    let mut server = RpcServer::new(facts);
    let local_addr = server.listen::<TcpServer<crate::RT>>(addr)?;
    Ok((server, local_addr))
}

pub struct FileServer<H, FH>
where
    H: FnOnce(FileServerTask) -> FH + Send + Sync + 'static + Clone,
    FH: Future<Output = Result<(), ()>> + Send + 'static,
{
    config: ServerConfig,
    server_handle: H,
    logger: Arc<LogFilter>,
}

impl<H, FH> FileServer<H, FH>
where
    H: FnOnce(FileServerTask) -> FH + Send + Sync + 'static + Clone,
    FH: Future<Output = Result<(), ()>> + Send + 'static,
{
    pub fn new(server_handle: H, config: ServerConfig) -> Self {
        Self { config, server_handle, logger: Arc::new(LogFilter::new()) }
    }
}

impl<H, FH> ServerFacts for FileServer<H, FH>
where
    H: FnOnce(FileServerTask) -> FH + Send + Sync + 'static + Clone,
    FH: Future<Output = Result<(), ()>> + Send + 'static,
{
    type Logger = Arc<LogFilter>;

    type IO = crate::RT;

    type RespTask = FileServerTask;

    #[inline]
    fn new_dispatcher(&self) -> impl ReqDispatch<Self::RespTask> {
        return ReqDispatchClosure::<MsgpCodec, FileServerTask, Self::RespTask, _, _>::new(
            self.server_handle.clone(),
        );
    }

    #[inline]
    fn spawn_detach<F, R>(&self, f: F)
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static,
    {
        crate::async_spawn!(f);
    }

    #[inline]
    fn new_logger(&self) -> Self::Logger {
        self.logger.clone()
    }

    #[inline]
    fn get_config(&self) -> &ServerConfig {
        &self.config
    }
}

#[server_task_enum(req, resp, error = Errno)]
#[derive(Debug)]
pub enum FileServerTask {
    #[action(FileAction::Open)]
    Open(ServerTaskOpen),
    #[action(FileAction::Read, FileAction::Write)]
    IO(ServerTaskIO),
}

pub type ServerTaskOpen = ServerTaskVariantFull<FileServerTask, FileOpenReq, (), Errno>;
pub type ServerTaskIO = ServerTaskVariantFull<FileServerTask, FileIOReq, FileIOResp, Errno>;
