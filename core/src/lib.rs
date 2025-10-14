//! # occams-rpc-core
//!
//! This crate provides the core utilities for [`occams-rpc`](https://docs.rs/occams-rpc).
//! It includes common types, traits, and error handling mechanisms used by other crates in the workspace.

mod codec;
pub use codec::Codec;
mod config;
pub use config::*;
pub mod error;
pub mod graceful;
pub mod io;
pub mod runtime;
