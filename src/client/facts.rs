use super::task::APIClientReq;
use super::{AsyncEndpoint, BlockingEndpoint};
use captains_log::filter::LogFilter;
use occams_rpc_core::{ClientConfig, Codec};
use occams_rpc_stream::client::{ClientFacts, ClientPool, ClientTransport};
use std::sync::Arc;

/// An example ClientFacts for API Clients
pub struct APIClientFactsDefault<C: Codec> {
    pub logger: Arc<LogFilter>,
    config: ClientConfig,
    rt: crate::RT,
    _phan: std::marker::PhantomData<fn(&C)>,
}

impl<C: Codec> APIClientFactsDefault<C> {
    pub fn new(config: ClientConfig, rt: crate::RT) -> Arc<Self> {
        Arc::new(Self { logger: Arc::new(LogFilter::new()), config, rt, _phan: Default::default() })
    }

    #[inline]
    pub fn set_log_level(&self, level: log::Level) {
        self.logger.set_level(level);
    }
}

impl<C: Codec> ClientFacts for APIClientFactsDefault<C> {
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
