use crate::TestRunnner;
use crate::stream_client::*;
use crate::stream_server::*;
use occams_rpc::RpcConfig;

#[test]
fn test_client_server() {
    let runner = TestRunnner::new();
    let mut config = RpcConfig::default();

    async fn dispatch_tast(task: FileServerTask) -> Result<(), ()> {
        todo!();
    }
    // TODO allocate local port for test
    let addr = "127.0.0.1:26000";
    let server = init_server(dispatch_tast, config.clone(), &addr).expect("server listen");
    runner.block_on(async move {
        let client = init_client(config, &addr, None).await.expect("connect client");
    })
}
