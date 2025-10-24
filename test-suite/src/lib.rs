pub mod api;
pub mod stream;

extern crate captains_log;
extern crate log;
pub use captains_log::logfn;
pub use occams_rpc_core::runtime::AsyncIO;

use captains_log::*;
use rstest::*;
use std::fmt;

#[cfg(feature = "tokio")]
use tokio::runtime::Runtime;

#[cfg(feature = "tokio")]
pub type RT = occams_rpc_tokio::TokioRT;
#[cfg(not(feature = "tokio"))]
pub type RT = occams_rpc_smol::SmolRT;

pub type Codec = occams_rpc_codec::MsgpCodec;

#[macro_export]
macro_rules! async_spawn {
    ($f: expr) => {{
        #[cfg(feature = "tokio")]
        {
            let _ = tokio::spawn($f);
        }
        #[cfg(not(feature = "tokio"))]
        {
            let _ = smol::spawn($f).detach();
        }
    }};
}

#[fixture]
pub fn runner() -> TestRunner {
    TestRunner::new()
}

impl fmt::Debug for TestRunner {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "")
    }
}

pub struct TestRunner {
    #[cfg(feature = "tokio")]
    rt: Runtime,
}

impl TestRunner {
    pub fn new() -> Self {
        recipe::raw_file_logger("/tmp/rpc_test.log", Level::Trace).test().build().expect("log");
        Self {
            #[cfg(feature = "tokio")]
            rt: tokio::runtime::Builder::new_multi_thread()
                .worker_threads(8)
                .enable_all()
                .build()
                .unwrap(),
        }
    }

    pub fn block_on<F: Future<Output = ()> + Send + 'static>(&self, f: F) {
        #[cfg(feature = "tokio")]
        {
            self.rt.block_on(f);
        }
        #[cfg(not(feature = "tokio"))]
        {
            smol::block_on(f);
        }
    }
}
