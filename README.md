# occams-rpc

A modular, plugable RPC for high throughput scenario, supports various runtime,
with a low-level streaming interface, and high-level remote API call interface.

## Components

`occams-rpc` is built from a collection of crates that provide different functionalities:

- core: [`occams-rpc-core`](https://docs.rs/occams-rpc-core): core utils crate
- coder: [`occams-rpc-codec`](https://docs.rs/occams-rpc-codec): Provides codecs for serialization, such as `msgpack`.
- runtimes:
  - [`occams-rpc-tokio`](https://docs.rs/occams-rpc-tokio): A runtime adapter for the `tokio` runtime.
  - [`occams-rpc-smol`](https://docs.rs/occams-rpc-smol): A runtime adapter for the `smol` runtime.
- transports:
  - [`occams-rpc-tcp`](https://docs.rs/occams-rpc-tcp): A TCP transport implementation.


## Streaming API

[occams-rpc-stream](https://crates.io/occams-rpc-stream) [document](https://docs.rs/occams-rpc-stream)

The interface is designed to optimize throughput and lower
CPU consumption for high-performance services.

Each connection is a full-duplex, multiplexed stream.
There's a `seq` ID assigned to a packet to track
a request and response. The timeout of a packet is checked in batches every second.
We utilize the [crossfire](https://docs.rs/crossfire) channel for parallelizing the work with
coroutines.

With an [RpcClient](https://docs.rs/occams-rpc-stream/latest/occams_rpc_stream/client_impl/struct.RpcClient.html), the user sends packets in sequence,
with a throttler controlling the IO depth of in-flight packets.
An internal timer then registers the request through a channel, and when the response
is received, it can optionally notify the user through a user-defined channel or another mechanism.

In an [RpcServer](https://docs.rs/occams-rpc-stream/latest/occams_rpc_stream/server_impl/struct.RpcServer.html), for each connection, there is one coroutine to read requests and one
coroutine to write responses. Requests can be dispatched with a user-defined
[ReqDispatch](https://docs.rs/occams-rpc-stream/latest/occams_rpc_stream/server/trait.ReqDispatch.html) trait implementation.
