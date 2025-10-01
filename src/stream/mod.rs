// low level of rpc

pub mod client;
mod client_impl;
pub mod client_timer;
pub mod proto;
//pub mod server;
pub mod throttler;

#[derive(Debug, Clone, Copy)]
pub enum RpcAction<'a> {
    Str(&'a str),
    Num(i32),
}

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
