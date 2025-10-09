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
use occams_rpc::stream::client::{ClientTask, ClientTaskCommon};
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

#[client_task(action = Action::Read)]
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
(often decorated with `#[client_task]`).

This macro generates `From` implementations for each variant, allowing for easy conversion
from a specific task struct to the enum. It also delegates methods from `ClientTask`,
`ClientTaskAction`, `ClientTaskEncode`, and `ClientTaskDecode` to the inner task.

### Example:

```rust
use occams_rpc::stream::client::{ClientTask, ClientTaskCommon};
use occams_rpc::error::RpcError;
use occams_rpc_macros::{client_task, client_task_enum};
use serde_derive::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

#[derive(Default, Deserialize, Serialize, Debug, PartialEq, Clone)]
pub struct FileReq {
    pub path: String,
}

#[repr(u8)]
enum Action {
    Read = 1,
    Write = 2,
}

#[client_task(action = Action::Read)]
#[derive(Debug)]
pub struct FileReadTask {
    #[field(common)]
    common: ClientTaskCommon,
    #[field(req)]
    req: FileReq,
    #[field(resp)]
    resp: Option<Vec<u8>>,
    res: Option<Result<(), RpcError>>,
}

impl ClientTask for FileReadTask {
    fn set_result(mut self, res: Result<(), RpcError>) {
        self.res = Some(res);
    }
}

#[client_task(action = Action::Write)]
#[derive(Debug)]
pub struct FileWriteTask {
    #[field(common)]
    common: ClientTaskCommon,
    #[field(req)]
    req: FileReq,
    #[field(req_blob)]
    blob: Vec<u8>,
    #[field(resp)]
    resp: Option<()>,
    res: Option<Result<(), RpcError>>,
}

impl ClientTask for FileWriteTask {
    fn set_result(mut self, res: Result<(), RpcError>) {
        self.res = Some(res);
    }
}

#[client_task_enum]
#[derive(Debug)]
pub enum FileTask {
    Read(FileReadTask),
    Write(FileWriteTask),
}

// Usage
let read_task = FileReadTask {
    common: ClientTaskCommon::default(),
    req: FileReq { path: "/path/to/file".to_string() },
    resp: None,
    res: None,
};

// Convert specific task into the enum
let mut file_task: FileTask = read_task.into();

// The enum now delegates trait methods
file_task.set_seq(100);
assert_eq!(file_task.seq, 100);
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
When applied to an enum, it automatically implements `ServerTaskDecode`, `ServerTaskEncode`,
`ServerTaskDone`, and `ServerTaskAction`, delegating the logic to the enum's variants.

### Macro Arguments:

* `req`: Implements `ServerTaskDecode` for the enum. When this is specified, each variant must have an `#[action(...)]` attribute.
* `resp`: Implements `ServerTaskEncode` and `ServerTaskDone` for the enum.
* `resp_type = <Type>`: Required if only `req` is specified. It defines the response type `R` for `ServerTaskDecode<R>` and `ServerTaskDone<R>`.

### Variant Attributes:

* `#[action(...)]`: Associates an RPC action (numeric, string, or enum value) with an enum variant.
  Multiple actions can be specified (e.g., `#[action(1, 2, "read")]`).

### Example:

```rust
use occams_rpc::{
    codec::{Codec, MsgpCodec},
    error::RpcError,
    stream::server_impl::ServerTaskVariant,
    stream::{
        server::*,
        RpcAction, RpcActionOwned,
    },
};
use occams_rpc_macros::server_task_enum;
use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
struct OpenFileReq { pub path: String, }

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
struct OpenFileResp { pub fd: u64, }

#[server_task_enum(req, resp)]
#[derive(Debug)]
pub enum FileTask {
    #[action(1)]
    Open(ServerTaskVariant<FileTask, OpenFileReq, OpenFileResp>),
}

// The macro generates the following (conceptual) implementations:
// - `From<ServerTaskVariant<...>> for FileTask`
// - `ServerTaskDecode<FileTask> for FileTask`
// - `ServerTaskEncode for FileTask`
// - `ServerTaskDone<FileTask> for FileTask`
// - `ServerTaskAction for FileTask`

// Usage
let (tx, _rx) = crossfire::mpsc::unbounded_async();
let noti: RespNoti<FileTask> = RespNoti::new(tx);
let codec = MsgpCodec::default();

let req_msg = OpenFileReq { path: "/tmp/file".to_string() };
let req_buf = codec.encode(&req_msg).unwrap();

// 1. Decode request into the enum
let mut server_task = <FileTask as ServerTaskDecode<FileTask>>::decode_req(
    &codec, RpcAction::Num(1), 123, &req_buf, None, noti
).unwrap();

assert_eq!(server_task.get_action(), RpcAction::Num(1));

// 2. Process task and set result
if let FileTask::Open(task) = &mut server_task {
    task.res = Some(Ok(OpenFileResp { fd: 1024 }));
}

// 3. Encode response
let (seq, resp_result) = server_task.encode_resp(&codec);
assert_eq!(seq, 123);
let resp_buf = resp_result.unwrap().unwrap(); // Result<Result<Vec<u8>,...>>
let resp_msg: OpenFileResp = codec.decode(&resp_buf).unwrap();
assert_eq!(resp_msg.fd, 1024);
```
*/
#[proc_macro_attribute]
pub fn server_task_enum(
    attrs: proc_macro::TokenStream, input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    server_task_enum::server_task_enum_impl(attrs, input)
}
