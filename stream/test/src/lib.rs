extern crate occams_rpc_stream;

pub mod client;
pub mod server;

extern crate captains_log;
extern crate log;

use captains_log::*;

#[cfg(feature = "tokio")]
use tokio::runtime::Runtime;

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

#[cfg(test)]
pub mod tests;
