use crate::client::*;
use crate::server::*;
use crate::*;
use crossfire::mpsc;
use nix::errno::Errno;
use occams_rpc_core::error::RpcError;
use occams_rpc_stream::client::{ClientConfig, ClientTaskGetResult};
use occams_rpc_stream::server::{ServerConfig, ServerTaskDone};

#[logfn]
#[rstest]
#[case(true)]
#[case(false)]
fn test_server_returns_error(runner: TestRunner, #[case] is_tcp: bool) {
    let client_config = ClientConfig::default();
    let server_config = ServerConfig::default();

    let dispatch_task = move |task: FileServerTask| {
        async move {
            match task {
                FileServerTask::Open(open_task) => {
                    info!("Server received Open task, will return error: {:?}", open_task.req);
                    open_task.set_result(Err(Errno::EACCES));
                    Ok(())
                }
                FileServerTask::IO(mut io_task) => {
                    // For IO tasks, just succeed to make sure we can test both cases.
                    info!("Server received IO task, will succeed: {:?}", io_task.req);
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

        // Test Open task that should fail
        let (tx, rx) = mpsc::unbounded_async();
        let open_task = FileClientTaskOpen::new(tx.clone(), "/root/secret.txt".to_string());
        client.send_task(open_task.into(), true).await.expect("send open task");

        let completed_open_task = rx.recv().await.unwrap();
        assert!(matches!(completed_open_task, FileClientTask::Open(_)));

        let result = completed_open_task.get_result();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), &RpcError::User(Errno::EACCES));
        log::info!("Open task failed as expected.");

        // Test a Write task that should succeed
        let write_task = FileClientTaskWrite::new(tx.clone(), 1, 0, vec![1, 2, 3].into());
        client.send_task(write_task.into(), true).await.expect("send write task");
        let completed_write_task = rx.recv().await.unwrap();
        assert!(matches!(completed_write_task, FileClientTask::Write(_)));
        assert!(completed_write_task.get_result().is_ok());
        log::info!("Write task completed successfully.");
    });
}
