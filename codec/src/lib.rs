#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(docsrs, allow(unused_attributes))]

//! # occams-rpc-codec
//!
//! This crate provides [occams_rpc_core::Codec](https://docs.rs/occams-rpc-core/latest/occams_rpc_core/trait.Codec.html) implementations for [`occams-rpc`](https://docs.rs/occams-rpc) and [`occams-rpc-stream`](https://docs.rs/occams-rpc-stream).
//! It supports different serialization formats, such as `msgpack`.

pub use occams_rpc_core::Codec;
#[cfg(feature = "msgpack")]
mod msgpack;
#[cfg(feature = "msgpack")]
pub use msgpack::*;
