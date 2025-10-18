use crate::client::*;
use crate::server::*;
use crate::*;
use crossfire::mpsc;
use occams_rpc_core::error::RpcError;
use occams_rpc_stream::client::{ClientConfig, ClientTaskGetResult};
use occams_rpc_stream::error::RpcIntErr;
use occams_rpc_stream::server::{ServerConfig, ServerTaskDone};
use std::time::Duration;

#[logfn]
#[rstest]
#[case(true)]
#[case(false)]
fn test_client_task_timeout(runner: TestRunner, #[case] is_tcp: bool) {
    // Set a short timeout for the client
    let client_config = ClientConfig {
        task_timeout: 2, // seconds
        ..Default::default()
    };
    let server_config = ServerConfig::default();

    let dispatch_task = move |task: FileServerTask| {
        async move {
            match task {
                FileServerTask::Open(open_task) => {
                    info!("Server received Open task, will delay response: {:?}", open_task.req);
                    // Delay for longer than the client's timeout
                    crate::RT::sleep(Duration::from_secs(4)).await;
                    open_task.set_result(Ok(()));
                    Ok(())
                }
                FileServerTask::IO(mut io_task) => {
                    // Other tasks succeed immediately
                    io_task.resp = Some(Default::default());
                    io_task.set_result(Ok(()));
                    Ok(())
                }
            }
        }
    };

    runner.block_on(async move {
        let server_bind_addr = if is_tcp { "127.0.0.1:0" } else { "/tmp/occams-rpc-test-socket" };
        let (_server, actual_server_addr) =
            init_server(dispatch_task, server_config.clone(), &server_bind_addr)
                .expect("server listen");
        debug!("client addr {:?}", actual_server_addr);
        let client =
            init_client(client_config, &actual_server_addr, None).await.expect("connect client");

        // Test Open task that should time out
        let (tx, rx) = mpsc::unbounded_async();
        let open_task = FileClientTaskOpen::new(tx.clone(), "/tmp/test.txt".to_string());
        client.send_task(open_task.into(), true).await.expect("send open task");

        let completed_open_task = rx.recv().await.unwrap();
        assert!(matches!(completed_open_task, FileClientTask::Open(_)));

        let result = completed_open_task.get_result();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), &RpcError::Rpc(RpcIntErr::Timeout));
        log::info!("Open task timed out as expected.");
    });
}
