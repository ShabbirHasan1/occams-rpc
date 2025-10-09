# Rpc Client Task Macro Design

The `#[client_task_enum]` and `#[client_task]` procedural macro simplifies the implementation of client-side RPC tasks by automatically generating necessary trait implementations and boilerplate code.

`#[client_task]` attribute allows users to define a struct representing an RPC task and specify its common fields, request, response, and optional blob data using `#[field(...)]` attributes.
It will not generate ClientTask trait because the ClientTaskAction is optional for task variants.

`#[client_task_enum]` attribute allow user to wrap an enum and delegate the ClienTask trait requirements to it's variants.
It will generate ClientTask trait and the ClientTaskAction according to #[action()] specified for enum variants.

### `#[action]` on enum variants

As an alternative to defining the action inside the subtype, you can specify a static action directly on an enum variant using the `#[action(...)]` attribute. Only one action (numeric, string literal, or a numeric `repr` enum variant) is allowed per variant.

When `#[action(...)]` is used on a variant, the `get_action()` method for that variant will return the specified static action, and the inner type does not need to provide an action.

## Usage Example

Here's an example of a struct using the `#[client_task]` macro:

```rust

#[client_task_enum]
pub enum FileTask {
    #[action(1)]
    Open(FileOpenTask),
    #[action("io_op")]
    IO(FileIOTask),
    // This variant delegates action to the inner type
    Close(FileCloseTask),
}

// This task does not need to specify an action, as it's handled by the enum variant.
#[client_task]
pub struct FileOpenTask {
    #[field(common)]
    common: TaskCommon,
    #[field(req)]
    req: String,
    #[field(resp)]
    resp: Option<()>
}

#[client_task]
pub struct FileIOTask {
    #[field(common)]
    common: TaskCommon,
    #[field(req)]
    req: FileIOReq,
    #[field(resp)]
    resp: Option<FileIOResp>,
}

// This task must specify its own action because the enum variant does not.
#[client_task(action = 3)]
pub struct FileCloseTask {
    #[field(common)]
    common: TaskCommon,
    #[field(req)]
    req: (),
    #[field(resp)]
    resp: Option<()>
}

```

## Field Attributes of `#[client_task]`

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

*  `#[field(res)]` and `#[field(noti)]`:
    *   Purpose: When both present, Generate ClientTaskDone trait
    *   requirements: the `res` fielid required to be `Option<Result<(), RpcError>>`, and the `noti` field require to be `Option<crossfire::MTx>`, or other unbounded channel has a send() method:


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

*   `ClientTaskDone`:
    ```rust
    impl occams_rpc::stream::client::ClientTaskDone for T {
        fn set_result(self, res: Result<(), Error>) {
            let noti = self.noti.take();
            let _ = crossfire::BlockingTxTrait::send(&noti, self.into());
        }
    }
    ```

*   when `#[client_task(debug)]` is specified, you will get a more specified Debug generated according to `req` and `resp` field
    ```rust
    impl std::fmt::Debug for T {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "{}(seq={} {:?}", std::any::type_name::<Self>(), self.seq, self.req)?;
            if let Some(resp) = self.resp.as_ref() {
                write!(f, " {:?})", self.resp)?;
            }
            Ok(())
        }
    }
    ```


The `#[client_task_enum]` will implement `From` to assist convertion from it's variants to parent enum, and delegating `ClienTaskEnode`, `ClientTaskDecode`, `ClientTaskAction`, `RpcClientTask` to it's variants.

## User Responsibilities

While the macro handles much of the boilerplate for encoding and decoding, users are still responsible for implementing other aspects of their `RpcClientTaskDone`, define logic for handling the task's result (e.g., `set_result()`).

**Note:** The implementation of `ClientTaskAction` is optional for structs decorated with `#[client_task]`. If `ClientTaskAction` is not implemented by the struct, then the `#[client_task_enum]` variant wrapping it must provide an `#[action]` attribute.
