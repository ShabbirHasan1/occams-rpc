# occams-rpc

A modular, pluggable RPC for high throughput scenario, supports various runtimes,
with a low-level streaming interface, and high-level remote API call interface.

## Components

`occams-rpc` is built from a collection of crates that provide different functionalities:

- core: [`occams-rpc-core`](https://docs.rs/occams-rpc-core): core utils crate
- codec: [`occams-rpc-codec`](https://docs.rs/occams-rpc-codec): Provides codecs for serialization, such as `msgpack`.
- runtimes:
  - [`occams-rpc-tokio`](https://docs.rs/occams-rpc-tokio): A runtime adapter for the `tokio` runtime.
  - [`occams-rpc-smol`](https://docs.rs/occams-rpc-smol): A runtime adapter for the `smol` runtime.
- transports:
  - [`occams-rpc-tcp`](https://docs.rs/occams-rpc-tcp): A TCP transport implementation.
- stream interface:
  - [`occams-rpc-stream`](https://docs.rs/occams-rpc-stream)
- Rust api interface:
  - [`occams-rpc`](https://docs.rs/occams-rpc)

## Streaming interface

The interface is designed to optimize throughput and lower
CPU consumption for high-performance services.

Each connection is a full-duplex, multiplexed stream.
There's a `seq` ID assigned to a packet to track
a request and response. The timeout of a packet is checked in batches every second.
We utilize the [crossfire](https://docs.rs/crossfire) channel for parallelizing the work with
coroutines.

With an `ClientStream` (used in `ClientPool` and `FailoverPool`), the request sends in sequence, flush in batches,
 and wait a slicing window throttler controlling the number of in-flight packets.
An internal timer then registers the request through a channel, and when the response is received,
 it can optionally notify the user through a user-defined channel or another mechanism.

In an `RpcServer`, for each connection, there is one coroutine to read requests and one
coroutine to write responses. Requests can be dispatched with a user-defined
`Dispatch` trait implementation.

## API call interface

- Independent from async runtime (with plugins)
- With service trait very similar to grpc / tarpc (stream in API interface is not supported
currently)
- Support latest `impl Future` definition of rust since 1.75, also support legacy `async_trait`
wrapper
- Each method can have different custom error type (requires the type implements [RpcErrCodec trait](https://docs.rs/occams-rpc-core/latest/occams_rpc_core/error/trait.RpcErrCodec.html))
- based on [occams-rpc-stream](https://docs.rs/occams-rpc-stream): Full duplex in each connection, with slicing window threshold, allow maximizing throughput and lower cpu usage.

(Warning: The API and feature is still evolving, might changed in the future)
