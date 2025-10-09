use occams_rpc::{
    codec::MsgpCodec,
    error::RpcError,
    stream::client::{
        ClientTask, ClientTaskAction, ClientTaskCommon, ClientTaskDecode, ClientTaskEncode,
    },
    stream::RpcAction,
};
use occams_rpc_macros::{client_task, client_task_enum};

#[client_task(action = 1)]
#[derive(Debug)]
struct TaskA {
    #[field(common)]
    common: ClientTaskCommon,
    #[field(req)]
    req: String,
    #[field(resp)]
    resp: Option<String>,
    res: Option<Result<(), RpcError>>,
}

impl ClientTask for TaskA {
    fn set_result(mut self, res: Result<(), RpcError>) {
        self.res = Some(res);
    }
}

#[client_task(action = "task_b")]
#[derive(Debug)]
struct TaskB {
    #[field(common)]
    common: ClientTaskCommon,
    #[field(req)]
    req: u32,
    #[field(resp)]
    resp: Option<u32>,
    res: Option<Result<(), RpcError>>,
}

impl ClientTask for TaskB {
    fn set_result(mut self, res: Result<(), RpcError>) {
        self.res = Some(res);
    }
}

#[client_task(action = 3)]
#[derive(Debug)]
struct TaskC {
    #[field(common)]
    common: ClientTaskCommon,
    #[field(req)]
    req: (),
    #[field(resp)]
    resp: Option<()>,
    #[field(req_blob)]
    req_blob: Vec<u8>,
    #[field(resp_blob)]
    resp_blob: Option<Vec<u8>>,
    res: Option<Result<(), RpcError>>,
}

impl ClientTask for TaskC {
    fn set_result(mut self, res: Result<(), RpcError>) {
        self.res = Some(res);
    }
}

#[client_task_enum]
#[derive(Debug)]
enum MyTask {
    A(TaskA),
    B(TaskB),
    C(TaskC),
}

#[test]
fn test_client_task_enum_delegation() {
    let task_a = TaskA {
        common: ClientTaskCommon::default(),
        req: "hello".to_string(),
        resp: None,
        res: None,
    };

    let task_b = TaskB { common: ClientTaskCommon::default(), req: 123, resp: None, res: None };

    let task_c = TaskC {
        common: ClientTaskCommon::default(),
        req: (),
        resp: None,
        req_blob: vec![1, 2, 3],
        resp_blob: Some(vec![]),
        res: None,
    };

    // Test From impls
    let mut enum_task_a: MyTask = task_a.into();

    // Test Deref
    enum_task_a.set_seq(10);

    // Test ClientTaskAction delegation
    assert_eq!(enum_task_a.get_action(), RpcAction::Num(1));

    let mut enum_task_b: MyTask = task_b.into();
    assert_eq!(enum_task_b.get_action(), RpcAction::Str("task_b"));
    let mut enum_task_c: MyTask = task_c.into();
    assert_eq!(enum_task_c.get_action(), RpcAction::Num(3));

    // Test ClientTask delegation
    enum_task_a.set_result(Ok(()));

    // Test ClientTaskEncode/Decode delegation
    let codec = MsgpCodec::default();
    let msg_buf = enum_task_b.encode_req(&codec).expect("encode_req ok");

    if let MyTask::B(t) = &mut enum_task_b {
        t.resp = None;
    }
    assert!(enum_task_b.decode_resp(&codec, &msg_buf).is_ok());
    if let MyTask::B(t) = enum_task_b {
        assert!(t.resp.is_some());
    } else {
        panic!("Wrong enum variant");
    }

    // Test blob delegation
    assert_eq!(enum_task_c.get_req_blob(), Some(vec![1, 2, 3].as_slice()));
    assert!(enum_task_c.reserve_resp_blob(10).is_some());
}

#[test]
fn test_client_task_enum_with_action_attribute() {
    #[client_task(action = 999)] // Dummy action
    #[derive(Debug)]
    struct TaskNoAction {
        #[field(common)]
        common: ClientTaskCommon,
        #[field(req)]
        req: String,
        #[field(resp)]
        resp: Option<String>,
        res: Option<Result<(), RpcError>>,
    }

    impl ClientTask for TaskNoAction {
        fn set_result(mut self, res: Result<(), RpcError>) {
            self.res = Some(res);
        }
    }

    #[client_task_enum]
    #[derive(Debug)]
    enum MyTaskWithAction {
        #[action(100)]
        A(TaskNoAction),
        #[action("action_b")]
        B(TaskB),
    }

    let task_a = TaskNoAction {
        common: ClientTaskCommon::default(),
        req: "hello".to_string(),
        resp: None,
        res: None,
    };

    let task_b = TaskB { common: ClientTaskCommon::default(), req: 123, resp: None, res: None };

    let enum_task_a: MyTaskWithAction = task_a.into();
    assert_eq!(enum_task_a.get_action(), RpcAction::Num(100));

    let enum_task_b: MyTaskWithAction = task_b.into();
    assert_eq!(enum_task_b.get_action(), RpcAction::Str("action_b"));
}
