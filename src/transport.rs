use crate::stream::client::{ClientFactory, ClientTransport};
use crate::{TimeoutSetting, error::RpcError};
use std::future::Future;
use std::time::Duration;

pub trait Transport<F: ClientFactory>: Send {
    type Connection;

    type Client: ClientTransport<F> + Send;

    fn connect(
        addr: &str, timeout: Duration,
    ) -> impl Future<Output = Result<Self::Connection, RpcError>> + Send;

    fn new_client_conn(
        stream: Self::Connection, logger: F::Logger, client_id: u64, server_id: u64,
        timeout: &TimeoutSetting,
    ) -> Self::Client;
}
