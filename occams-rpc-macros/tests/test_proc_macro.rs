use occams_rpc::{
    codec::{Codec, MsgpCodec},
    stream::client_task::{AllocateBuf, ClientTaskDecode, ClientTaskEncode, TaskCommon},
};
use occams_rpc_macros::client_task;
use serde_derive::{Deserialize, Serialize};
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::ops::{Deref, DerefMut};

#[derive(Default, Deserialize, Serialize, Debug, PartialEq, Clone)]
pub struct FileIOReq {
    pub inode: u64,
    pub offset: i64,
}

impl Display for FileIOReq {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "inode: {}, offset: {}", self.inode, self.offset)
    }
}

#[derive(Default, Deserialize, Serialize, Debug, PartialEq)]
pub struct FileIOResp {
    pub read_size: u64,
}

impl Display for FileIOResp {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "read_size: {}", self.read_size)
    }
}

#[client_task]
pub struct FileTask {
    #[field(common)]
    common: TaskCommon,
    #[field(req)]
    req: FileIOReq,
    #[field(resp)]
    resp: Option<FileIOResp>,
}

#[test]
fn test_client_task_macro() {
    let mut task = FileTask {
        common: TaskCommon { seq: 123, ..Default::default() },
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
    common: TaskCommon,
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
        common: TaskCommon { seq: 123, ..Default::default() },
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
        common: TaskCommon { seq: 123, ..Default::default() },
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
    common: TaskCommon,
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
        common: TaskCommon { seq: 123, ..Default::default() },
        req: req_data.clone(),
        resp: None,
        resp_blob: initial_resp_blob,
    };

    // Test ClientTaskDecode::get_resp_blob_mut
    let resp_blob_mut = task.get_resp_blob_mut();
    assert!(resp_blob_mut.is_some());

    assert_eq!(resp_blob_mut.unwrap().reserve(10).unwrap().len(), 10);
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
