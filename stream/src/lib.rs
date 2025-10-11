#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(docsrs, allow(unused_attributes))]

//! # occams-rpc-stream
//!
//! This crate provides a low-level streaming interface for `occams-rpc`.
//! It is used for stream processing and is part of the modular design of `occams-rpc`.
//!
//! ## Components
//!
//! `occams-rpc` is built from a collection of crates that provide different functionalities:
//!
//! - [`occams-rpc`](https://docs.rs/occams-rpc): Provides the high-level remote API call interface.
//! - [`occams-rpc-codec`](https://docs.rs/occams-rpc-codec): Provides codecs for serialization, such as `msgpack`.
//! - [`occams-rpc-tokio`](https://docs.rs/occams-rpc-tokio): A runtime adapter for the `tokio` runtime.
//! - [`occams-rpc-smol`](https://docs.rs/occams-rpc-smol): A runtime adapter for the `smol` runtime.
//! - [`occams-rpc-tcp`](https://docs.rs/occams-rpc-tcp): A TCP transport implementation.
//!

#[macro_use]
extern crate captains_log;

pub mod client;
pub mod client_impl;
pub mod client_timer;
pub mod macros;
pub mod proto;
pub mod server;
pub mod server_impl;
pub mod throttler;
pub use occams_rpc_core::error;
pub use occams_rpc_core::{RpcConfig, TimeoutSetting};
