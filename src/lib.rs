#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(docsrs, allow(unused_attributes))]

//! # occams-rpc
//!
//! This crate provides a high-level remote API call interface for `occams-rpc`.
//! It is part of a modular, pluggable RPC for high throughput scenarios that supports various async runtimes.
//!
//! Currently a placeholder, API should be added in the future
//!
//! ## Components
//!
//! `occams-rpc` is built from a collection of crates that provide different functionalities:
//!
//! - [`occams-rpc-stream`](https://docs.rs/occams-rpc-stream): Provides the low-level streaming interface for stream processing.
//! - [`occams-rpc-codec`](https://docs.rs/occams-rpc-codec): Provides codecs for serialization, such as `msgpack`.
//! - [`occams-rpc-tokio`](https://docs.rs/occams-rpc-tokio): A runtime adapter for the `tokio` runtime.
//! - [`occams-rpc-smol`](https://docs.rs/occams-rpc-smol): A runtime adapter for the `smol` runtime.
//! - [`occams-rpc-tcp`](https://docs.rs/occams-rpc-tcp): A TCP transport implementation.
