# Rpc Client Task Macro Design

The `#[client_task]` procedural macro simplifies the implementation of client-side RPC tasks by automatically generating necessary trait implementations and boilerplate code. It allows users to define a struct representing an RPC task and specify its common fields, request, response, and optional blob data using `#[field(...)]` attributes.

## Usage Example

Here's an example of a struct using the `#[client_task]` macro:

```rust
#[client_task]
pub struct FileIOTask {
    #[field(action)]
    action: u8,
    #[field(common)]
    common: TaskCommon,
    sender: MTx<FileTask>,
    #[field(req)]
    req: FileIOReq,
    #[field(resp)]
    resp: Option<FileIOResp>,
    action: i32,
    res: Option<Result<(), RpcError>>,
}

#[client_task]
pub struct FileOpen {
    #[action(Action::Open)]
    #[field(common)]
    common: TaskCommon,
    sender: MTx<FileTask>,
    #[field(req)]
    req: String,
    #[field(resp)]
    resp: Option<()>,
    action: i32,
    res: Option<Result<(), RpcError>>,
}

pub enum Action {
    Open=1,
    Read=2,
    Write=3,
}

```

## Field Attributes

The macro processes specific fields within the task struct based on the `#[field(...)]` attributes:

*   `#[field(common)]`:
    *   **Purpose:** Designates a field that holds common task-related information.
    *   **Requirements:** This field is mandatory. The macro generates `Deref` and `DerefMut` implementations for the task struct, delegating to this `common` field. This allows direct access to the common fields of the task struct.

*   `#[field(action)]`
    *   Purpose: implement ClientTaskAction trait to return RpcAction according to the field
    *   Requirements: there's no `action=` specified within the client_task attribute. the field is a numeric or string type, or an enum type that repr by number.

*   attribute parameter action in `#[client_task(action=...)]`
    *   Purpose: implement ClientTaskAction trait with a static value
    *   Requirements: there's no `#[field(action)]` specified within the struct.  `action()` parameter must be only one, can be numeric, or string, or an enum repr by number.

*   `#[field(req)]`:
    *   **Purpose:** Designates the field containing the request payload for the RPC call.
    *   **Requirements:** This field is mandatory. The macro uses this field to implement the `ClientTaskEncode::encode_req` method, serializing its content using the provided `Codec`.

*   `#[field(resp)]`:
    *   **Purpose:** Designates the field where the decoded response from the RPC call will be stored.
    *   **Requirements:** This field is mandatory and *must* be of type `Option<T>`. The macro implements `ClientTaskDecode::decode_resp`, which decodes the incoming buffer into `T` and sets it to `Some(decoded_value)` in this field.

*   `#[field(req_blob)]`:
    *   **Purpose:** Designates an optional field for sending additional binary data (a "request blob") with the RPC request.
    *   **Requirements:** This field is optional. If present, its type must implement `AsRef<u8>`. The macro implements `ClientTaskEncode::get_req_blob`, returning a reference to this blob.

*   `#[field(resp_blob)]`:
    *   **Purpose:** Designates an optional field for receiving additional binary data (a "response blob") with the RPC response.
    *   **Requirements:** This field is optional. If present, its type *must* be `Option<T>`, where `T` implements `occams_rpc::io::AllocateBuf`. The macro implements `ClientTaskDecode::get_resp_blob_mut`, returning a mutable reference to this `Option<T>`.

## Generated Trait Implementations

The `#[client_task]` macro automatically generates the following trait implementations for the decorated struct:

*   `std::ops::Deref` and `std::ops::DerefMut`:
    *   These traits are implemented to delegate to the field marked with `#[field(common)]`, providing convenient access to its members.

*   `ClientTaskEncode`:
    *   `encode_req<C: occams_rpc::codec::Codec>(&self, codec: &C) -> Result<Vec<u8>, ()>`: Encodes the content of the `#[field(req)]` field using the provided codec.
    *   `get_req_blob(&self) -> Option<&[u8]>`: If `#[field(req_blob)]` is present, returns `Some` reference to the blob data; otherwise, it returns `None`.

*   `ClientTaskDecode`:
    *   `decode_resp<C: occams_rpc::codec::Codec>(&mut self, codec: &C, buffer: &[u8]) -> Result<(), ()>`: Decodes the response buffer using the provided codec and stores the result in the `#[field(resp)]` field.
    *   `get_resp_blob_mut(&mut self) -> Option<&mut impl occams_rpc::io::AllocateBuf>`: If `#[field(resp_blob)]` is present, returns `Some` mutable reference to the response blob `Option<T>`; otherwise, it returns `None`.

## User Responsibilities

While the macro handles much of the boilerplate for encoding and decoding, users are still responsible for implementing other aspects of their `RpcClientTask`, such as any logic for handling the task's result (e.g., `set_result()`).
