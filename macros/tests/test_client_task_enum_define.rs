/// This test is to be sure that macro use full path calls without any trait imported
#[test]
fn test_client_task_enum_define() {
    use occams_rpc::error::RpcError;
    use occams_rpc::stream::client::{ClientTaskCommon, ClientTaskDone};
    use occams_rpc_macros::{client_task, client_task_enum};
    use serde_derive::{Deserialize, Serialize};

    #[derive(PartialEq)]
    #[repr(u8)]
    enum FileAction {
        Open = 1,
        Close = 2,
    }

    #[client_task]
    #[derive(Debug)]
    pub struct FileOpenTask {
        #[field(common)]
        common: ClientTaskCommon,
        #[field(req)]
        req: String,
        #[field(resp)]
        resp: Option<()>,
    }
    impl ClientTaskDone for FileOpenTask {
        fn set_result(self, _res: Result<(), RpcError>) {}
    }

    #[client_task(action = 2)] // This action will be used as the variant doesn't specify one
    #[derive(Debug)]
    pub struct FileCloseTask {
        #[field(common)]
        common: ClientTaskCommon,
        #[field(req)]
        req: (),
        #[field(resp)]
        resp: Option<()>,
    }
    impl ClientTaskDone for FileCloseTask {
        fn set_result(self, _res: Result<(), RpcError>) {}
    }

    #[client_task_enum]
    #[derive(Debug)]
    pub enum FileTask {
        #[action(FileAction::Open)]
        Open(FileOpenTask),
        // This variant delegates action to the inner type
        Close(FileCloseTask),
    }
}
