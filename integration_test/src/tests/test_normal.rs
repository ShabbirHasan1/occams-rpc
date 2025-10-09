use crate::TestRunnner;
use crate::stream_client::*;
use crate::stream_server::*;
use crossfire::mpsc;
use io_buffer::{Buffer, rand_buffer}; // Added rand_buffer
use log::info;
use occams_rpc::RpcConfig;
use occams_rpc::stream::RpcAction;
use occams_rpc::stream::client::ClientTaskDone;
use occams_rpc::stream::server::{ServerTaskAction, ServerTaskDone}; // New import
use std::convert::TryFrom; // New import

static mut STORED_DATA: Option<Buffer> = None; // Simulated data store

#[test]
fn test_client_server() {
    let runner = TestRunnner::new();
    let mut config = RpcConfig::default();

    async fn dispatch_task(task: FileServerTask) -> Result<(), ()> {
        match task {
            FileServerTask::Open(mut open_task) => {
                info!("Server received Open task: {:?}", open_task.req);
                // Simulate opening a file
                open_task.set_result(Ok(()));
                Ok(())
            }
            FileServerTask::IO(mut io_task) => {
                match io_task.get_action() {
                    RpcAction::Num(action_num) => {
                        let action = FileAction::try_from(action_num as u8).map_err(|e| {
                            log::error!("Error converting action num to FileAction: {}", e);
                            ()
                        })?;

                        match action {
                            FileAction::Read => {
                                info!("Server received Read task: {:?}", io_task.req);
                                // Retrieve stored data
                                let data_to_read = unsafe {
                                    STORED_DATA.take().unwrap_or_else(|| Buffer::from(vec![]))
                                };
                                let ret_size = data_to_read.len() as u64;

                                // Set response blob
                                io_task.resp_blob = Some(data_to_read);

                                // Set response message
                                io_task.resp = Some(FileIOResp { ret_size });

                                io_task.set_result(Ok(()));
                                Ok(())
                            }
                            FileAction::Write => {
                                info!("Server received Write task: {:?}", io_task.req);
                                // Simulate writing data
                                if let Some(blob) = io_task.req_blob.take() {
                                    info!("Server received blob data len: {}", blob.len());
                                    let ret_size = blob.len() as u64;
                                    unsafe {
                                        STORED_DATA = Some(blob);
                                    }; // Store the data
                                    io_task.resp = Some(FileIOResp { ret_size });
                                    io_task.set_result(Ok(()));
                                } else {
                                    log::error!("Write task received without blob data.");
                                    io_task.set_result(Err(occams_rpc::error::RpcError::Text(
                                        "No data to write".to_string(),
                                    )));
                                }
                                Ok(())
                            }
                            FileAction::Open => {
                                // Should not happen as Open is handled by a separate variant
                                log::error!("Received unexpected FileAction::Open for IO task.");
                                io_task.set_result(Err(occams_rpc::error::RpcError::Text(
                                    "Unexpected FileAction::Open".to_string(),
                                )));
                                Err(())
                            }
                        }
                    }
                    _ => {
                        log::error!("Unexpected RpcAction type for IO task.");
                        io_task.set_result(Err(occams_rpc::error::RpcError::Text(
                            "Unexpected RpcAction type".to_string(),
                        )));
                        Err(())
                    }
                }
            }
        }
    }

    let server_bind_addr = "127.0.0.1:0"; // Bind to a random ephemeral port
    let (server, actual_server_addr) =
        init_server(dispatch_task, config.clone(), &server_bind_addr).expect("server listen");
    let client_connect_addr = format!("127.0.0.1:{}", actual_server_addr.port());

    runner.block_on(async move {
        let client = init_client(config, &client_connect_addr, None).await.expect("connect client");

        // Test Open task
        let (tx, rx) = mpsc::unbounded_blocking();
        let open_task = FileClientTaskOpen::new(tx.clone(), "/tmp/test_file.txt".to_string());
        client.send_task(open_task.into(), true).await.expect("send open task");
        let completed_open_task = rx.recv().unwrap();
        assert!(matches!(completed_open_task, FileClientTask::Open(_)));
        assert!(completed_open_task.get_result().unwrap().is_ok());
        log::info!("Open task completed successfully.");

        // Test Write task
        let random_data_len = 1024;
        let write_data = rand_buffer(random_data_len);
        let write_task = FileClientTaskWrite::new(tx.clone(), 1, 0, write_data.clone());
        client.send_task(write_task.into(), true).await.expect("send write task");
        let completed_write_task = rx.recv().unwrap();
        assert!(matches!(completed_write_task, FileClientTask::Write(_)));
        assert!(completed_write_task.get_result().unwrap().is_ok());
        if let FileClientTask::Write(task) = completed_write_task {
            assert_eq!(task.resp.unwrap().ret_size, write_data.len() as u64);
        }
        log::info!("Write task completed successfully.");

        // Test Read task
        let read_task = FileClientTaskRead::new(tx.clone(), 1, 0, random_data_len);
        client.send_task(read_task.into(), true).await.expect("send read task");
        let completed_read_task = rx.recv().unwrap();
        assert!(matches!(completed_read_task, FileClientTask::Read(_)));
        assert!(completed_read_task.get_result().unwrap().is_ok());
        if let FileClientTask::Read(task) = completed_read_task {
            assert_eq!(task.resp.unwrap().ret_size, random_data_len as u64);
            assert_eq!(task.read_data.unwrap().as_ref(), write_data.as_ref()); // Compare with original random data
        }
        log::info!("Read task completed successfully.");
    });
}
