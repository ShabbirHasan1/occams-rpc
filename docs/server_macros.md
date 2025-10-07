# `server_task_enum` Macro

## Overview

The `#[server_task_enum]` is a procedural attribute macro designed to streamline the creation of server-side task enums in `occams-rpc`. It automates the implementation of several key traits required for processing RPC requests, reducing boilerplate and improving code maintainability.

When applied to an enum, this macro generates implementations for `ServerTaskDecode`, `ServerTaskEncode`, and `ServerTaskDone`, as well as `From` implementations for each variant's inner type.

## Usage

To use the macro, apply it to an enum where each variant is a tuple-style variant with a single field representing a specific task type. Each variant must also have an `#[action]` attribute to associate it with an RPC action.

```rust
use occams_rpc::stream::server::{ServerTaskDecode, ServerTaskEncode, ServerTaskDone};
use occams_rpc_macros::server_task_enum;

// Define your subtypes for each task
struct SubTask1;
struct SubTask2;

// Implement the required traits for your subtypes
impl<'a, T: Send + 'static> ServerTaskDecode<'a, T> for SubTask1 { /* ... */ }
impl ServerTaskEncode for SubTask1 { /* ... */ }
impl<T: RpcServerTaskResp> ServerTaskDone<T> for SubTask1 { /* ... */ }

impl<'a, T: Send + 'static> ServerTaskDecode<'a, T> for SubTask2 { /* ... */ }
impl ServerTaskEncode for SubTask2 { /* ... */ }
impl<T: RpcServerTaskResp> ServerTaskDone<T> for SubTask2 { /* ... */ }


#[server_task_enum]
#[derive(Debug)]
pub enum ServerTask {
    #[action(action = 1)]
    Task1(SubTask1),

    #[action(action = "sub_task_2")]
    Task2(SubTask2),
}
```

## Generated Code

The macro generates the following implementations for the enum:

### `From<SubType> for Enum`

For each variant, a `From` implementation is generated to allow easy conversion from a subtype to the enum.

```rust
impl From<SubTask1> for ServerTask {
    fn from(task: SubTask1) -> Self {
        ServerTask::Task1(task)
    }
}
```

### `ServerTaskDecode`

The macro generates an implementation of `ServerTaskDecode` that decodes an incoming request based on the `RpcAction`. It matches the action from the request with the `#[action]` attribute of the variants and delegates the decoding to the corresponding subtype's `decode_req` method.

### `ServerTaskEncode`

An implementation of `ServerTaskEncode` is generated to encode the response. It matches on the enum variant and delegates the encoding to the inner subtype's `encode_resp` method.

### `ServerTaskDone`

An implementation of `ServerTaskDone` is generated to handle the completion of a task. It delegates the `set_result` call to the inner subtype.

### `get_action()`

A `get_action(&self) -> RpcAction` method is generated for the enum, which returns the `RpcAction` associated with the current variant.

## Requirements

- The macro must be applied to an enum.
- Each variant of the enum must be a tuple-style variant with a single field.
- Each variant must have an `#[action]` attribute, which can be a numeric or string literal (e.g., `#[action(action = 1)]` or `#[action(action = "my_action")]`).
- The inner type of each variant must implement `ServerTaskDecode`, `ServerTaskEncode`, and `ServerTaskDone`.

## Trait Delegation and Compile-Time Checks

The macro relies on a "delegation" pattern. It generates code that calls the trait methods on the inner subtypes. This means that if a subtype does not implement a required trait (e.g., `ServerTaskEncode`), the code will fail to compile with a clear error message about the missing trait implementation.

This approach ensures type safety and correctness at compile time, leveraging the Rust compiler to enforce the necessary constraints on the task subtypes.
