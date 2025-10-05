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
