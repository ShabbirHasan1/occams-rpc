use occams_rpc::{
    codec::{Codec, MsgpCodec},
    error::RpcError,
    stream::server_impl::ServerTaskVariant,
    stream::{
        server::{RpcRespNoti, RpcSvrResp, ServerTaskDecode, ServerTaskDone, ServerTaskEncode},
        RpcAction,
    },
};
use occams_rpc_macros::server_task_enum;
use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
struct ReqMsg1 {
    pub val: u32,
}
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
struct ReqMsg2 {
    pub name: String,
}

// Dummy message structs
#[derive(Serialize, Deserialize, Debug, Default)]
struct Msg1;
#[derive(Serialize, Deserialize, Debug, Default)]
struct Msg2;

#[test]
fn test_server_task_enum_req_macro() {
    #[server_task_enum(req, resp_type=RpcSvrResp)]
    #[derive(Debug)]
    pub enum ExampleServerTaskReq {
        #[action(1)]
        Task1(ServerTaskVariant<RpcSvrResp, ReqMsg1>),
        #[action("sub_task_2")]
        Task2(ServerTaskVariant<RpcSvrResp, ReqMsg2>),
    }

    let codec = MsgpCodec::default();
    let (tx, _rx) = crossfire::mpsc::unbounded_async();
    let noti: RpcRespNoti<RpcSvrResp> = RpcRespNoti::new(tx);

    // Test decode_req with numeric action and actual data
    let req_msg_1_data = ReqMsg1 { val: 100 };
    let req_msg_1_buf = codec.encode(&req_msg_1_data).unwrap();
    let task1 = <ExampleServerTaskReq as ServerTaskDecode<RpcSvrResp>>::decode_req(
        &codec,
        RpcAction::Num(1),
        123,
        &req_msg_1_buf,
        None,
        noti.clone(),
    )
    .unwrap();
    if let ExampleServerTaskReq::Task1(req_task_variant) = task1 {
        assert_eq!(req_task_variant.seq, 123);
        assert_eq!(req_task_variant.msg, req_msg_1_data);
        assert!(req_task_variant.blob.is_none()); // Assuming no blob is sent for this test
    } else {
        panic!("Decoded to wrong variant");
    }

    // Test decode_req with string action and actual data
    let req_msg_2_data = ReqMsg2 { name: "test_name".to_string() };
    let req_msg_2_buf = codec.encode(&req_msg_2_data).unwrap();
    let task2 = <ExampleServerTaskReq as ServerTaskDecode<RpcSvrResp>>::decode_req(
        &codec,
        RpcAction::Str("sub_task_2"),
        456,
        &req_msg_2_buf,
        None,
        noti.clone(),
    )
    .unwrap();
    assert_eq!(task2.get_action(), RpcAction::Str("sub_task_2"));
    if let ExampleServerTaskReq::Task2(req_task_variant) = task2 {
        assert_eq!(req_task_variant.seq, 456);
        assert_eq!(req_task_variant.msg, req_msg_2_data);
        assert!(req_task_variant.blob.is_none()); // Assuming no blob is sent for this test
    } else {
        panic!("Decoded to wrong variant");
    }

    // Test From impls
    let req_task_variant_from_sub = ServerTaskVariant {
        seq: 3,
        msg: ReqMsg1::default(),
        blob: None,
        res: None,
        noti: Some(noti.clone()),
    };
    let server_task_from_sub: ExampleServerTaskReq = req_task_variant_from_sub.into();
    assert!(matches!(server_task_from_sub, ExampleServerTaskReq::Task1(_)));
}

#[test]
fn test_server_task_enum_resp_macro() {
    #[server_task_enum(resp)]
    #[derive(Debug)]
    pub enum ExampleServerTaskResp {
        Task1(ServerTaskVariant<ExampleServerTaskResp, Msg1>),
        Task2(ServerTaskVariant<ExampleServerTaskResp, Msg2>),
    }
    let codec = MsgpCodec::default();
    let (tx, _rx) = crossfire::mpsc::unbounded_async();
    let noti: RpcRespNoti<ExampleServerTaskResp> = RpcRespNoti::new(tx);

    // Create ServerTaskVariant instances using decode_req
    let msg1_buf = codec.encode(&Msg1::default()).unwrap();
    let variant1: ServerTaskVariant<ExampleServerTaskResp, Msg1> =
        <ServerTaskVariant<ExampleServerTaskResp, Msg1> as ServerTaskDecode<
            ExampleServerTaskResp,
        >>::decode_req(&codec, RpcAction::Num(1), 123, &msg1_buf, None, noti.clone())
        .unwrap();

    let msg2_buf = codec.encode(&Msg2::default()).unwrap();
    let variant2: ServerTaskVariant<ExampleServerTaskResp, Msg2> =
        <ServerTaskVariant<ExampleServerTaskResp, Msg2> as ServerTaskDecode<
            ExampleServerTaskResp,
        >>::decode_req(&codec, RpcAction::Num(2), 456, &msg2_buf, None, noti.clone())
        .unwrap();

    let mut task1_for_encode = ExampleServerTaskResp::Task1(variant1);
    <ExampleServerTaskResp as ServerTaskDone<ExampleServerTaskResp>>::set_result(
        &mut task1_for_encode,
        Err(RpcError::Text("some error".to_string())),
    );
    let (seq1, resp1) =
        <ExampleServerTaskResp as ServerTaskEncode>::encode_resp(&task1_for_encode, &codec);
    assert_eq!(seq1, 123);
    assert_eq!(resp1.unwrap_err(), &RpcError::Text("some error".to_string()));

    let mut task2_for_encode = ExampleServerTaskResp::Task2(variant2);
    <ExampleServerTaskResp as ServerTaskDone<ExampleServerTaskResp>>::set_result(
        &mut task2_for_encode,
        Ok(()),
    );
    let (seq2, resp2) =
        <ExampleServerTaskResp as ServerTaskEncode>::encode_resp(&task2_for_encode, &codec);
    assert_eq!(seq2, 456);
    assert!(resp2.is_ok());

    // Test set_result
    let mut task_for_set_result = ExampleServerTaskResp::Task1(
        <ServerTaskVariant<ExampleServerTaskResp, Msg1> as ServerTaskDecode<
            ExampleServerTaskResp,
        >>::decode_req(&codec, RpcAction::Num(1), 789, &msg1_buf, None, noti.clone())
        .unwrap(),
    );
    let _: RpcRespNoti<ExampleServerTaskResp> = <ExampleServerTaskResp as ServerTaskDone<
        ExampleServerTaskResp,
    >>::set_result(&mut task_for_set_result, Ok(()));

    // Test set_result_done
    #[allow(unused_mut)]
    let mut task_for_set_result_done = ExampleServerTaskResp::Task2(
        <ServerTaskVariant<ExampleServerTaskResp, Msg2> as ServerTaskDecode<
            ExampleServerTaskResp,
        >>::decode_req(&codec, RpcAction::Num(2), 101, &msg2_buf, None, noti.clone())
        .unwrap(),
    );
    task_for_set_result_done.set_result_done(Ok(()));
}
