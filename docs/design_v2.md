# The occams-rpc design

## Design goal

A modular, plugable RPC that supports various async runtimes.

* interface types:
  - stream: Interface for stream processing.
  - api: Interface for remote function call.
* transport types:
  - TCP transport: optimized for high throughput internal network.
  - RDMA transport: optimized for low latency internal network.
  - Quic transport: optimized for high throughput public network.
* codec:
  - Msgpack
  - bincode
* runtime: provides AsyncIO trait for runtime adapter
  - tokio
  - smol (async_io)

## Stream interface

### Stream protocol

str/stream/proto.rs: Defines general proto

`RpcAction`: numeric action is for fewer type variants, while string action is for the implmentation of API interface.

Request format: each request package has a fixed-length header (ReqHead), then followed by structured encoded "msg", then optionally followed by a "blob" (which allocated as owned buffer).

Response format: each reponse package has a fixed-length header (RespHead), then followed by structured encoded "msg", then optionally followed by a "blob" (which allocated as owned buffer)

### Stream Client


### Stream Server

src/stream/server.rs:

`ServerFactory` is the interafce for user can assign or customize with various plugin (transport, codec, ...)

src/stream/server_impl.rs:

`RpcServer` is the server implementation, listen and accept connections through transport. The new connection is process by ServerHandle::run interface. We typically use the `ServerHandleTaskStream` implmentation, which utilizes a coroutine to read and a coroutine to write, so that the throughput can at max use 2 cpu cores with one connection.

We intended to support service-level graceful restart and graceful shutdown.

As a packet received by server, should have gone through the steps:

    decode -> request task -> dispatch -> processed -> response task -> encode -> write

#### decode

An enum task should impl ServerTaskDecode::decode_req(action ... ), regconize sub task type by RpcAction.

For numeric, more used in the stream interface, suitable to use a generated match to call the sub types decode_req.

The macro can be:

```

#[server_task_enum]
pub enum MyServerTask {
    [action=xxx]
    Type1(SubType1)
    [action=xxx]
    Type2(SubType2)
}
```

(action may be num or "string")

For SubType1, need to generate MyServerTask::From SubType1. If user process using SubType, and call subtype::set_result(), should transform into() the supertype to call RpcRespNoti.done().

If user use the ServerTaskVariant/ServerTaskVariantFull generic as subtype, which already implemented ServerTaskEncode/Encode,
 they cannot impl they own method due to the "Orphan Rule" limitation of Rust,
 they should impl wrapper types with new-type pattern. (It's not certain for me Deref, should test)

So the macro is on MyServerTask:

* Need to assign action for the variants for ServerTaskDecode::decode_req, provides get_action(&self) for user

* Need to impl From variants

For string, might used in the API interface, can be first match in a HashMap to decide which class to handle, an decode accoding to string (class::method) to a specified Req type.


#### Dispatch

The dispatcher is repsponsible for decode the task and dispatch to backend processing. (There may be struct like worker pool in other threads/coroutines to handle)

Codec is intend to support encryption, so there should be shared state, need to initialized as Arc<Codec> share between
conn reader and writer. Therefore, The dispatcher should be a Arc being shared between reader and writer.

Because the connection of reader and writer is parallel, we should define ReqDispatch trait and RespReceiver trait separately. The ReqDispatch relies on RespReceiver because it should know about the task type of done channel.

We set RespReceiver in ServerFactory as an associate type. but we leave the ReqDispatch to be infered From
` fn new_dispatcher(&self) -> impl ReqDispatch<Self::RespReceiver>``, because like TaskReqDispatch, usually comes with
closure capturing the connext about how to dispatch the task. There may be a Fn or a struct defined by user.
