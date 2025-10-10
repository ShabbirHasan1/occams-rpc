use std::time::Duration;

#[derive(Clone)]
pub struct RpcConfig {
    pub timeout: TimeoutSetting,
    /// How many async RpcTask in the queue, prevent overflow server capacity
    pub thresholds: usize,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self { timeout: TimeoutSetting::default(), thresholds: 128 }
    }
}

#[derive(Clone)]
pub struct TimeoutSetting {
    /// timeout of RpcTask waiting for response, in seconds.
    pub task_timeout: usize,
    /// socket read timeout
    pub read_timeout: Duration,
    /// Socket write timeout
    pub write_timeout: Duration,
    /// Socket idle time to be close.
    pub idle_timeout: Duration,
    /// connect timeout
    pub connect_timeout: Duration,
}

impl Default for TimeoutSetting {
    fn default() -> Self {
        Self {
            task_timeout: 20,
            read_timeout: Duration::from_secs(5),
            write_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(10),
        }
    }
}
