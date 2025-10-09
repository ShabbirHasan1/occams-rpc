/*!
This crate provides procedural macros to simplify the implementation of RPC tasks for the `occams-rpc` framework.
These macros automatically generate boilerplate code for trait implementations, reducing manual effort and improving code clarity.

# Provided Macros

### Client-Side
- [`#[client_task]`](macro@client_task): For defining a client-side RPC task on a struct.
- [`#[client_task_enum]`](macro@client_task_enum): For creating an enum that delegates to client task variants.

### Server-Side
- [`#[server_task_enum]`](macro@server_task_enum): For defining a server-side RPC task enum.
*/

mod client_task;
mod client_task_enum;
mod server_task_enum;

/**
# `#[client_task]`

The `#[client_task]` attribute macro is used on a struct to designate it as a client-side RPC task.
It automatically implements `ClientTaskAction`, `ClientTaskEncode`, `ClientTaskDecode`, and `Deref`/`DerefMut`
to a common field.

To make the struct a complete RPC task, you must also implement the `ClientTask` trait, which primarily
involves providing a `set_result` method. By implementing `ClientTask` and using the macro,
the struct will automatically satisfy the bounds for `RpcClientTask`.

Fields within the struct must be annotated with `#[field(...)]` to specify their role.

### Field Attributes:

* `#[field(common)]`: **(Mandatory)** Marks a field that holds common task information (e.g., `ClientTaskCommon`).
  The macro generates `Deref` and `DerefMut` implementations that delegate to this field,
  allowing direct access to its members (like `seq`).

* `#[field(action)]`: Specifies a field that determines the RPC action. The field's value will be
  used to identify the task on the server. This is mutually exclusive with `#[client_task(action = ...)]`.

* `#[client_task(action = ...)]`: Sets a static action for the task, which can be a number, a string,
  or an enum variant. This is an alternative to `#[field(action)]`.

* `#[field(req)]`: **(Mandatory)** Designates the field containing the request payload for the RPC call.
  This field's content is serialized for the request.

* `#[field(resp)]`: **(Mandatory)** Designates the field where the decoded response will be stored.
  This field must be of type `Option<T>`.

* `#[field(req_blob)]`: (Optional) Marks a field for sending additional binary data (blob)
  with the request. The field type must implement `AsRef<[u8]>`.

* `#[field(resp_blob)]`: (Optional) Marks a field for receiving additional binary data (blob)
  with the response. The field type must be `Option<T>` where `T` implements `occams_rpc::io::AllocateBuf`.

### Example:

```rust
use occams_rpc::stream::client::{ClientTask, ClientTaskCommon, ClientTaskAction, ClientTaskEncode, ClientTaskDecode};
use occams_rpc::error::RpcError;
use occams_rpc_macros::client_task;
use serde_derive::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

#[derive(Default, Deserialize, Serialize, Debug, PartialEq, Clone)]
pub struct FileIOReq {
    pub path: String,
    pub offset: u64,
}

#[derive(Default, Deserialize, Serialize, Debug, PartialEq)]
pub struct FileIOResp {
    pub bytes_read: u64,
}

#[repr(u8)]
enum Action {
    Read = 1,
    Write = 2,
}

#[client_task(action = 1)]
#[derive(Debug)]
pub struct FileReadTask {
    #[field(common)]
    common: ClientTaskCommon,
    #[field(req)]
    req: FileIOReq,
    #[field(resp)]
    resp: Option<FileIOResp>,
    #[field(req_blob)]
    req_blob: Vec<u8>,
    #[field(resp_blob)]
    resp_blob: Option<Vec<u8>>,
    // Field to store the final result
    res: Option<Result<(), RpcError>>,
}

impl ClientTask for FileReadTask {
    fn set_result(mut self, res: Result<(), RpcError>) {
        // Custom logic to handle the task's result
        self.res = Some(res);
    }
}

// Usage
let mut task = FileReadTask {
    common: ClientTaskCommon { seq: 1, ..Default::default() },
    req: FileIOReq { path: "/path/to/file".to_string(), offset: 0 },
    resp: None,
    req_blob: vec![],
    resp_blob: Some(Vec::new()),
    res: None,
};

// Access common fields directly
task.set_seq(123);
assert_eq!(task.seq, 123);
```
*/
#[proc_macro_attribute]
pub fn client_task(
    attrs: proc_macro::TokenStream, input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    client_task::client_task_impl(attrs, input)
}

/**
# `#[client_task_enum]`

The `#[client_task_enum]` attribute is applied to an enum to delegate `ClientTask` related trait
implementations to its variants. Each variant must wrap a struct that is a valid client task
(often decorated with `#[client_task]`)

This macro generates `From` implementations for each variant, allowing for easy conversion
from a specific task struct to the enum. It also delegates methods from `ClientTask`,
`ClientTaskEncode`, and `ClientTaskDecode` to the inner task.

### `#[action]` on enum variants

As an alternative to defining the action inside the subtype, you can specify a static action
directly on an enum variant using the `#[action(...)]` attribute. Only one action (numeric or
string literal) is allowed per variant.

When `#[action(...)]` is used on a variant, the `get_action()` method for that variant will
return the specified static action. The inner type does not need to define an action in this case,
but if it does, the enum's action will take precedence.

### Example:

```rust
use occams_rpc::stream::client::{ClientTask, ClientTaskCommon, ClientTaskAction, ClientTaskEncode, ClientTaskDecode};
use occams_rpc::error::RpcError;
use occams_rpc_macros::{client_task, client_task_enum};
use serde_derive::{Deserialize, Serialize};

#[client_task(action = 999)] // Action can be a dummy one, it will be overridden
#[derive(Debug)]
pub struct FileOpenTask {
    #[field(common)]
    common: ClientTaskCommon,
    #[field(req)]
    req: String,
    #[field(resp)]
    resp: Option<()>
}
impl ClientTask for FileOpenTask {
    fn set_result(self, _res: Result<(), RpcError>) {}
}

#[client_task(action = 3)] // This action will be used as the variant doesn't specify one
#[derive(Debug)]
pub struct FileCloseTask {
    #[field(common)]
    common: ClientTaskCommon,
    #[field(req)]
    req: (),
    #[field(resp)]
    resp: Option<()>
}
impl ClientTask for FileCloseTask {
    fn set_result(self, _res: Result<(), RpcError>) {}
}


#[client_task_enum]
#[derive(Debug)]
pub enum FileTask {
    #[action(1)]
    Open(FileOpenTask),
    // This variant delegates action to the inner type
    Close(FileCloseTask),
}

// Usage
let open_task = FileOpenTask {
    common: ClientTaskCommon::default(),
    req: "/path/to/file".to_string(),
    resp: None,
};

let mut file_task: FileTask = open_task.into();
assert_eq!(file_task.get_action(), occams_rpc::stream::RpcAction::Num(1));

let close_task = FileCloseTask {
    common: ClientTaskCommon::default(),
    req: (),
    resp: None,
};

let mut file_task: FileTask = close_task.into();
assert_eq!(file_task.get_action(), occams_rpc::stream::RpcAction::Num(3));
```
*/
#[proc_macro_attribute]
pub fn client_task_enum(
    attrs: proc_macro::TokenStream, input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    client_task_enum::client_task_enum_impl(attrs, input)
}

/**
# `#[server_task_enum]`

The `#[server_task_enum]` macro streamlines the creation of server-side task enums.
When applied to an enum, it automatically implements necessary traits for processing RPC requests,
reducing boilerplate and improving code maintainability.

### Macro Arguments:

* `req`: Indicates that the enum handles requests. When specified, each variant must have an `#[action(...)]` attribute.
* `resp`: Indicates that the enum handles responses.
* `resp_type = <Type>`: Required if `req` is specified but `resp` is not. Defines the response type.

### Variant Attributes:

* `#[action(...)]`: Associates an RPC action (numeric, string, or enum value) with an enum variant.
  Multiple actions can be specified (e.g., `#[action(1, 2, "read")]`).

### Example:

```rust
use occams_rpc::stream::server_impl::ServerTaskVariant;
use occams_rpc_macros::server_task_enum;
use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
struct FileOpenReq { pub path: String, }

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
struct FileReadReq { pub path: String, pub offset: u64, pub len: u64, }

#[server_task_enum(req, resp)]
#[derive(Debug)]
pub enum FileTask {
    #[action(1)]
    Open(ServerTaskVariant<FileTask, FileOpenReq>),
    #[action("read_file")]
    Read(ServerTaskVariant<FileTask, FileReadReq>),
}
```
*/
#[proc_macro_attribute]
pub fn server_task_enum(
    attrs: proc_macro::TokenStream, input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    server_task_enum::server_task_enum_impl(attrs, input)
}
