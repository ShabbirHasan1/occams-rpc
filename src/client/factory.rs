use super::task::APIClientReq;
use super::{AsyncEndpoint, BlockingEndpoint};
use captains_log::filter::LogFilter;
use occams_rpc_core::{ClientConfig, Codec};
use occams_rpc_stream::client::{ClientFactory, ClientPool, ClientTransport, FailoverPool};
use std::sync::Arc;

/// An example factory for API Clients
pub struct APIClientFactory<C: Codec> {
    pub logger: Arc<LogFilter>,
    config: ClientConfig,
    rt: crate::RT,
    _phan: std::marker::PhantomData<fn(&C)>,
}

impl<C: Codec> APIClientFactory<C> {
    pub fn new(config: ClientConfig, rt: crate::RT) -> Arc<Self> {
        Arc::new(Self { logger: Arc::new(LogFilter::new()), config, rt, _phan: Default::default() })
    }

    #[inline]
    pub fn set_log_level(&self, level: log::Level) {
        self.logger.set_level(level);
    }

    pub fn create_endpoint_async<T: ClientTransport<crate::RT>>(
        self: Arc<Self>, addr: &str,
    ) -> AsyncEndpoint<ClientPool<Self, T>> {
        return AsyncEndpoint::new(ClientPool::new(self.clone(), addr, 0));
    }

    pub fn create_endpoint_async_failover<T: ClientTransport<crate::RT>>(
        self: Arc<Self>, addrs: Vec<String>, round_robin: bool, retry_limit: usize,
    ) -> AsyncEndpoint<Arc<FailoverPool<Self, T>>> {
        return AsyncEndpoint::new(Arc::new(FailoverPool::new(
            self.clone(),
            addrs,
            round_robin,
            retry_limit,
            0,
        )));
    }

    pub fn create_endpoint_blocking<T: ClientTransport<crate::RT>>(
        self: Arc<Self>, addr: &str,
    ) -> BlockingEndpoint<ClientPool<Self, T>> {
        return BlockingEndpoint::new(ClientPool::new(self.clone(), addr, 0));
    }

    pub fn create_endpoint_blocking_failover<T: ClientTransport<crate::RT>>(
        self: Arc<Self>, addrs: Vec<String>, round_robin: bool, retry_limit: usize,
    ) -> BlockingEndpoint<Arc<FailoverPool<Self, T>>> {
        return BlockingEndpoint::new(Arc::new(FailoverPool::new(
            self.clone(),
            addrs,
            round_robin,
            retry_limit,
            0,
        )));
    }
}

impl<C: Codec> ClientFactory for APIClientFactory<C> {
    type Logger = Arc<LogFilter>;
    type Codec = C;
    type Task = APIClientReq;

    type IO = crate::RT;

    #[inline]
    fn spawn_detach<F, R>(&self, f: F)
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static,
    {
        self.rt.spawn_detach(f);
    }

    #[inline]
    fn new_logger(&self, _conn_id: &str) -> Arc<LogFilter> {
        self.logger.clone()
    }

    #[inline]
    fn get_config(&self) -> &ClientConfig {
        &self.config
    }
}
