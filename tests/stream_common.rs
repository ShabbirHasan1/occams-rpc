use captains_log::recipe;
use crossfire::*;
use display_attr::*;
use io_buffer::Buffer;
use log::*;
use occams_rpc::codec::MsgpCodec;
use occams_rpc::stream::client::*;
use occams_rpc::stream::*;
use occams_rpc::*;
use occams_rpc_macros::*;
use serde_derive::{Deserialize, Serialize};
use std::fmt;
use tokio::runtime::{Builder, Runtime};

pub fn setup() -> Runtime {
    recipe::raw_file_logger("/tmp/rpc_test.log", Level::Trace).test().build().expect("log");
    let rt = Builder::new_multi_thread().worker_threads(8).enable_all().build().unwrap();
    rt
}

#[derive(Default, Deserialize, Serialize, DisplayAttr)]
#[display(fmt = "inode={} offset={}", inode, offset)]
pub struct FileIOReq {
    pub inode: u64,
    pub offset: i64,
}

#[derive(Default, Deserialize, Serialize, DisplayAttr, Debug)]
#[display(fmt = "read_size={}", read_size)]
pub struct FileIOResp {
    pub read_size: u64,
}

macro_rules! unwrap_msg {
    ($seq: expr, $action: expr, $msg: expr, $req_cls: tt) => {
        if let Some(_msg) = $msg {
            match rmp_serde::decode::from_slice::<$req_cls>(_msg) {
                Err(e) => {
                    println!("seq={} action={} msg decode error: {:#?}", $seq, $action, e);
                    Err(())
                }
                Ok(_msg) => {
                    let task = TestServerTask { seq: $seq, action: $action };
                    Ok(task)
                }
            }
        } else {
            println!("seq={} action={} missing msg field", $seq, $action);
            Err(())
        }
    };
}

pub struct TestServerTask {
    seq: u64,
    action: i32,
}

pub fn test_task_decoder(
    seq: u64, action: i32, msg: Option<&[u8]>, _ext: Option<Buffer>,
) -> Result<TestServerTask, ()> {
    unwrap_msg!(seq, action, msg, FileIOReq)
}

#[client_task]
pub struct FileTask {
    #[field(common)]
    common: ClientTaskCommon,
    sender: MTx<Self>,
    #[field(req)]
    req: FileIOReq,
    #[field(resp)]
    resp: Option<FileIOResp>,
    action: i32,
    res: Option<Result<(), RpcError>>,
}

impl fmt::Debug for FileTask {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "FileTask seq={} action={}", self.common.seq, self.action)
    }
}

impl FileTask {
    pub fn new(action: i32, sender: MTx<Self>) -> Self {
        Self {
            common: ClientTaskCommon::default(),
            sender,
            action,
            res: None,
            req: FileIOReq { inode: 1, offset: 0 },
            resp: None,
        }
    }
}

impl RpcClientTask for FileTask {
    fn action<'a>(&'a self) -> RpcAction<'a> {
        return RpcAction::Num(self.action);
    }

    fn set_result(self, _res: Result<(), RpcError>) {
        // mock implementation
    }
}
