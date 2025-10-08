use occams_rpc::{
    codec::{Codec, MsgpCodec},
    stream::client::{ClientTaskAction, ClientTaskCommon, ClientTaskDecode, ClientTaskEncode},
};
use occams_rpc_macros::client_task;
use serde_derive::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

#[derive(Default, Deserialize, Serialize, Debug, PartialEq, Clone)]
pub struct FileIOReq {
    pub inode: u64,
    pub offset: i64,
}

#[derive(Default, Deserialize, Serialize, Debug, PartialEq)]
pub struct FileIOResp {
    pub read_size: u64,
}

#[test]
fn test_client_task_macro() {
    #[client_task]
    #[derive(Debug)]
    pub struct FileTask {
        #[field(common)]
        common: ClientTaskCommon,
        #[field(req)]
        req: FileIOReq,
        #[field(resp)]
        resp: Option<FileIOResp>,
    }
    let mut task = FileTask {
        common: ClientTaskCommon { seq: 123, ..Default::default() },
        req: FileIOReq { inode: 1, offset: 100 },
        resp: None,
    };
    let codec = MsgpCodec::default();

    // Test Deref and DerefMut
    assert_eq!(task.seq, 123);
    task.seq = 456;
    assert_eq!(task.deref().seq, 456);
    task.deref_mut().seq = 789;
    assert_eq!(task.seq, 789);

    // Test ClientTaskEncode
    let req_buf = task.encode_req(&codec).expect("encode");
    let req_copy: FileIOReq = codec.decode(&req_buf).expect("decode");
    assert_eq!(task.req, req_copy);

    // Test ClientTaskDecode
    let resp = FileIOResp { read_size: 1024 };
    let resp_buffer = codec.encode(&resp).expect("encode");
    let result = task.decode_resp(&codec, &resp_buffer);
    assert!(result.is_ok());
    assert_eq!(task.resp, Some(resp));
}

#[client_task]
pub struct FileTaskWithBlob {
    #[field(common)]
    common: ClientTaskCommon,
    #[field(req)]
    req: FileIOReq,
    #[field(req_blob)]
    blob: Vec<u8>,
    #[field(resp)]
    resp: Option<FileIOResp>,
}

#[test]
fn test_client_task_macro_with_req_blob() {
    let req_data = FileIOReq { inode: 2, offset: 200 };
    let blob_data = vec![1, 2, 3, 4, 5];

    let task = FileTaskWithBlob {
        common: ClientTaskCommon { seq: 123, ..Default::default() },
        req: req_data.clone(),
        resp: None,
        blob: blob_data.clone(),
    };

    // Test ClientTaskEncode::get_req_blob
    let retrieved_blob = task.get_req_blob();
    assert_eq!(retrieved_blob, Some(blob_data.as_slice()));
    let codec = MsgpCodec::default();

    // Test ClientTaskEncode::encode_req (should not include blob)
    let encoded_req_only = task.encode_req(&codec).expect("encode");
    let req_copy: FileIOReq = codec.decode(&encoded_req_only).expect("decode");
    assert_eq!(task.req, req_copy);

    // Test ClientTaskDecode (should still work for resp)
    let mut task_decode = FileTaskWithBlob {
        common: ClientTaskCommon { seq: 123, ..Default::default() },
        req: req_data.clone(),
        resp: None,
        blob: blob_data.clone(),
    };
    let resp = FileIOResp { read_size: 2048 };
    let resp_buffer = codec.encode(&resp).expect("encode");
    let result = task_decode.decode_resp(&codec, &resp_buffer);
    assert!(result.is_ok());
    assert_eq!(task_decode.resp, Some(resp));
}

#[client_task]
pub struct FileTaskWithRespBlob {
    #[field(common)]
    common: ClientTaskCommon,
    #[field(req)]
    req: FileIOReq,
    #[field(resp)]
    resp: Option<FileIOResp>,
    #[field(resp_blob)]
    resp_blob: Option<Vec<u8>>,
}

#[test]
fn test_client_task_macro_with_resp_blob() {
    let req_data = FileIOReq { inode: 3, offset: 300 };
    let resp_data = FileIOResp { read_size: 4096 };
    let initial_resp_blob = Some(vec![6, 7, 8, 9, 10]);

    let mut task = FileTaskWithRespBlob {
        common: ClientTaskCommon { seq: 123, ..Default::default() },
        req: req_data.clone(),
        resp: None,
        resp_blob: initial_resp_blob,
    };

    // Test ClientTaskDecode::reserve_resp_blob
    assert!(task.reserve_resp_blob(20).is_some());

    let codec = MsgpCodec::default();

    // Test ClientTaskEncode (should still work for req)
    let encoded_req_only = task.encode_req(&codec).expect("encode");
    let req_copy: FileIOReq = codec.decode(&encoded_req_only).expect("decode");
    assert_eq!(task.req, req_copy);

    // Test ClientTaskDecode (should still work for resp)
    let resp_buffer = codec.encode(&resp_data).expect("encode");
    let result = task.decode_resp(&codec, &resp_buffer);
    assert!(result.is_ok());
    assert_eq!(task.resp, Some(resp_data));
}

#[repr(u8)]
#[derive(Debug, PartialEq)]
#[allow(dead_code)] // Allow unused variants for test enum
enum Action {
    Open = 1,
    Read = 2,
    Write = 3,
}

#[test]
fn test_client_task_macro_field_action() {
    #[derive(Default, Deserialize, Serialize, Debug, PartialEq, Clone)]
    pub struct MyReq {
        pub data: u32,
    }

    #[derive(Default, Deserialize, Serialize, Debug, PartialEq)]
    pub struct MyResp {
        pub result: bool,
    }

    #[client_task]
    #[derive(Debug)]
    pub struct FieldActionTaskNum {
        #[field(common)]
        common: ClientTaskCommon,
        #[field(action)]
        action_num: u8, // Numeric action field
        #[field(req)]
        req: MyReq,
        #[field(resp)]
        resp: Option<MyResp>,
    }

    #[client_task]
    #[derive(Debug)]
    pub struct FieldActionTaskStr {
        #[field(common)]
        common: ClientTaskCommon,
        #[field(action)]
        action_str: String, // String action field
        #[field(req)]
        req: MyReq,
        #[field(resp)]
        resp: Option<MyResp>,
    }

    // Test FieldActionTaskNum
    let task_num = FieldActionTaskNum {
        common: ClientTaskCommon { seq: 1, ..Default::default() },
        action_num: 5,
        req: MyReq { data: 10 },
        resp: None,
    };
    assert_eq!(task_num.get_action(), occams_rpc::stream::RpcAction::Num(5));

    // Test FieldActionTaskStr
    let task_str = FieldActionTaskStr {
        common: ClientTaskCommon { seq: 2, ..Default::default() },
        action_str: "my_action".to_string(),
        req: MyReq { data: 20 },
        resp: None,
    };
    assert_eq!(task_str.get_action(), occams_rpc::stream::RpcAction::Str("my_action"));
}

#[test]
fn test_client_task_macro_static_action() {
    #[derive(Default, Deserialize, Serialize, Debug, PartialEq, Clone)]
    pub struct MyReq {
        pub data: u32,
    }

    #[derive(Default, Deserialize, Serialize, Debug, PartialEq)]
    pub struct MyResp {
        pub result: bool,
    }

    #[client_task(action = 10)] // Numeric static action
    #[derive(Debug)]
    pub struct StaticActionTaskNum {
        #[field(common)]
        common: ClientTaskCommon,
        #[field(req)]
        req: MyReq,
        #[field(resp)]
        resp: Option<MyResp>,
    }

    #[client_task(action = "static_str_action")] // String static action
    #[derive(Debug)]
    pub struct StaticActionTaskStr {
        #[field(common)]
        common: ClientTaskCommon,
        #[field(req)]
        req: MyReq,
        #[field(resp)]
        resp: Option<MyResp>,
    }

    #[client_task(action(Action::Open))] // Enum static action
    #[derive(Debug)]
    pub struct StaticActionTaskEnum {
        #[field(common)]
        common: ClientTaskCommon,
        #[field(req)]
        req: MyReq,
        #[field(resp)]
        resp: Option<MyResp>,
    }

    // Test StaticActionTaskNum
    let task_num = StaticActionTaskNum {
        common: ClientTaskCommon { seq: 1, ..Default::default() },
        req: MyReq { data: 10 },
        resp: None,
    };
    assert_eq!(task_num.get_action(), occams_rpc::stream::RpcAction::Num(10));

    // Test StaticActionTaskStr
    let task_str = StaticActionTaskStr {
        common: ClientTaskCommon { seq: 2, ..Default::default() },
        req: MyReq { data: 20 },
        resp: None,
    };
    assert_eq!(task_str.get_action(), occams_rpc::stream::RpcAction::Str("static_str_action"));

    // Test StaticActionTaskEnum
    let task_enum = StaticActionTaskEnum {
        common: ClientTaskCommon { seq: 3, ..Default::default() },
        req: MyReq { data: 30 },
        resp: None,
    };
    assert_eq!(task_enum.get_action(), occams_rpc::stream::RpcAction::Num(Action::Open as i32));
}
