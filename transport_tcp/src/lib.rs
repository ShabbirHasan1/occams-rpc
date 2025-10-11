#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(docsrs, allow(unused_attributes))]

//! # occams-rpc-tcp
//!
//! This crate provides a TCP transport implementation for [`occams-rpc`](https://docs.rs/occams-rpc).
//! It is used for both client and server communication over TCP.

#[macro_use]
extern crate captains_log;
mod client;
pub use client::*;
//pub mod graceful;
pub mod net;
mod server;
pub use server::*;
