// low level of rpc

pub mod client;
pub mod client_impl;
pub mod client_timer;
pub mod proto;
pub mod server;
pub mod server_impl;
pub mod throttler;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RpcAction<'a> {
    Str(&'a str),
    Num(i32),
}
