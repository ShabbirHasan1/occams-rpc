use io_buffer::Buffer;
use occams_rpc::{
    codec::{Codec, MsgpCodec},
    error::RpcError,
    stream::{
        RpcAction,
        server::{RpcRespNoti, RpcSvrResp, ServerTaskDecode, ServerTaskEncode},
        server_impl::ServerTaskVariant,
    },
};
use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
struct TestReqMsg {
    pub value: String,
    pub number: u32,
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone)]
struct TestRespMsg {
    pub result: bool,
    pub code: u16,
}

#[test]
fn test_server_task_variant_decode_req() {
    let codec = MsgpCodec::default();
    let (tx, _rx) = crossfire::mpsc::unbounded_async();
    let noti: RpcRespNoti<RpcSvrResp> = RpcRespNoti::new(tx);

    let req_msg = TestReqMsg { value: "test_request".to_string(), number: 123 };
    let req_buf = codec.encode(&req_msg).unwrap();
    let action = RpcAction::Num(1);
    let seq = 456;
    let blob = Some(Buffer::from("test_blob".as_bytes().to_vec()));

    let decoded_task: ServerTaskVariant<RpcSvrResp, TestReqMsg> =
        ServerTaskVariant::decode_req(&codec, action, seq, &req_buf, blob.clone(), noti.clone())
            .unwrap();

    assert_eq!(decoded_task.seq, seq);
    assert_eq!(decoded_task.msg, req_msg);
    assert!(decoded_task.blob.is_some());
    assert_eq!(decoded_task.blob.unwrap().as_ref(), blob.unwrap().as_ref());
    assert!(decoded_task.res.is_none());
    assert!(decoded_task.noti.is_some());

    // Test with no blob
    let decoded_task_no_blob: ServerTaskVariant<RpcSvrResp, TestReqMsg> =
        ServerTaskVariant::decode_req(&codec, action, seq, &req_buf, None, noti).unwrap();
    assert!(decoded_task_no_blob.blob.is_none());
}
