#[macro_use]
extern crate captains_log;

mod client_impl;
pub use client_impl::ClientConn;
pub mod graceful;
pub mod net;
use crate::net::*;

use log::*;
use occams_rpc::stream::client::ClientFactory;
use occams_rpc::transport::Transport;
use occams_rpc::{TimeoutSetting, error};
use std::str::FromStr;
use std::time::Duration;

pub struct TcpTransport();

impl<F: ClientFactory> Transport<F> for TcpTransport {
    type Connection = UnifyStream;

    type Client = ClientConn<F>;

    #[inline]
    async fn connect(addr: &str, timeout: Duration) {
        match UnifyAddr::from_str(addr) {
            Err(e) => {
                error!("Cannot parsing addr {}: {:?}", addr, e);
                return Err(error::RPC_ERR_CONNECT);
            }
            Ok(_addr) => match UnifyStream::connect_timeout(&_addr, timeout).await {
                Ok(stream) => {
                    return Ok(stream);
                }
                Err(e) => {
                    warn!("Cannot connect addr {}: {:?}", addr, e);
                    return Err(error::RPC_ERR_CONNECT);
                }
            },
        }
    }

    #[inline(always)]
    fn new_client_conn(
        stream: Self::Connection, logger: F::Logger, client_id: u64, server_id: u64,
        timeout: &TimeoutSetting,
    ) -> Self::Client {
        ClientConn::new(stream, logger, client_id, server_id, timeout)
    }
}
