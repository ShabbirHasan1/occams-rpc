use std::time::Duration;

/// General config for client-side
#[derive(Clone)]
pub struct ClientConfig {
    /// timeout of RpcTask waiting for response, in seconds.
    pub task_timeout: usize,
    /// socket read timeout
    pub read_timeout: Duration,
    /// Socket write timeout
    pub write_timeout: Duration,
    /// Socket idle time to be close. for connection pool.
    pub idle_timeout: Duration,
    /// connect timeout
    pub connect_timeout: Duration,
    /// How many async RpcTask in the queue, prevent overflow server capacity
    pub thresholds: usize,
    /// In bytes. when non-zero, overwrite the default DEFAULT_BUF_SIZE of transport
    pub stream_buf_size: usize,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            task_timeout: 20,
            read_timeout: Duration::from_secs(5),
            write_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(10),
            thresholds: 128,
            stream_buf_size: 0,
        }
    }
}

/// General config for server-side
#[derive(Clone)]
pub struct ServerConfig {
    /// socket read timeout
    pub read_timeout: Duration,
    /// Socket write timeout
    pub write_timeout: Duration,
    /// Socket idle time to be close.
    pub idle_timeout: Duration,
    /// wait for all connections to be close with a timeout
    pub server_close_wait: Duration,
    /// In bytes. when non-zero, overwrite the default DEFAULT_BUF_SIZE of transport
    pub stream_buf_size: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            read_timeout: Duration::from_secs(5),
            write_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(120),
            server_close_wait: Duration::from_secs(90),
            stream_buf_size: 0,
        }
    }
}
