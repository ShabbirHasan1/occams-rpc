use super::client::{FileAction, FileIOReq, FileIOResp, FileOpenReq};
use occams_rpc_codec::MsgpCodec;
use occams_rpc_stream::server::*;
use occams_rpc_stream::server_impl::*;
use std::net::SocketAddr;
use std::sync::Arc; // New import

use occams_rpc_stream::{macros::*, *};

use captains_log::filter::LogFilter;

pub fn init_server<H, FH>(
    server_handle: H, config: RpcConfig, addr: &str,
) -> Result<(RpcServer<FileServer<H, FH>>, SocketAddr), std::io::Error>
where
    H: FnOnce(FileServerTask) -> FH + Send + Sync + 'static + Clone,
    FH: Future<Output = Result<(), ()>> + Send + 'static,
{
    let factory = Arc::new(FileServer::new(server_handle, config));
    let mut server = RpcServer::new(factory);
    let local_addr = server.listen(addr)?;
    Ok((server, local_addr))
}

pub struct FileServer<H, FH>
where
    H: FnOnce(FileServerTask) -> FH + Send + Sync + 'static + Clone,
    FH: Future<Output = Result<(), ()>> + Send + 'static,
{
    config: RpcConfig,
    server_handle: H,
}

impl<H, FH> FileServer<H, FH>
where
    H: FnOnce(FileServerTask) -> FH + Send + Sync + 'static + Clone,
    FH: Future<Output = Result<(), ()>> + Send + 'static,
{
    pub fn new(server_handle: H, config: RpcConfig) -> Self {
        Self { config, server_handle }
    }
}

impl<H, FH> ServerFactory for FileServer<H, FH>
where
    H: FnOnce(FileServerTask) -> FH + Send + Sync + 'static + Clone,
    FH: Future<Output = Result<(), ()>> + Send + 'static,
{
    type Logger = captains_log::filter::LogFilter;

    type Transport = occams_rpc_tcp::TcpServer<Self>;

    #[cfg(feature = "tokio")]
    type IO = occams_rpc_tokio::TokioRT;
    #[cfg(not(feature = "tokio"))]
    type IO = occams_rpc_smol::SmolRT;

    type RespReceiver = RespReceiverTask<FileServerTask>;

    #[inline]
    fn new_dispatcher(&self) -> impl ReqDispatch<Self::RespReceiver> {
        return TaskReqDispatch::<MsgpCodec, FileServerTask, Self::RespReceiver, _, _>::new(
            self.server_handle.clone(),
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
            let _ = smol::spawn(f).detach();
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
#[derive(Debug)]
pub enum FileServerTask {
    #[action(FileAction::Open)]
    Open(ServerTaskOpen),
    #[action(FileAction::Read, FileAction::Write)]
    IO(ServerTaskIO),
}

pub type ServerTaskOpen = ServerTaskVariantFull<FileServerTask, FileOpenReq, ()>;
pub type ServerTaskIO = ServerTaskVariantFull<FileServerTask, FileIOReq, FileIOResp>;
