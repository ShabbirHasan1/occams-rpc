//! Provides procedural macros to simplify the implementation of RPC tasks for the `occams-rpc-stream` API.
//! These macros automatically generate boilerplate code for trait implementations, reducing manual effort and improving code clarity.
//!
//! # Provided Macros
//!
//! ### Client-Side
//! - [`#[client_task]`](macro@client_task): For defining a client-side RPC task on a struct. It will not generate ClientTask trait (it's optional to you to define ClientTaskAction with it)
//! - [`#[client_task_enum]`](macro@client_task_enum): For creating an enum that delegates to client task variants. It will generate ClientTask trait for the enum
//!
//! ### Server-Side
//! - [`#[server_task_enum]`](macro@server_task_enum): For defining a server-side RPC task enum.

pub use occams_rpc_stream_macros::*;
