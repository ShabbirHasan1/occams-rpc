# Rpc Client Task 的设计

用户需要实现 RpcClientTask 这个 trait。每个 Task 报文需要一个 Request 结构体 (允许为空)，也需要指定一个 Response 结构体，允许为空。
可以选择一种 Codec 来做序列化，比如 MsgpCodec。在 client 接收 Response 解码成功后，调用decode_resp 将 buffer 解码的结果设置到 Task 当中。
解码成功后，使用 task的 set_result 通知结果。

比如用户有一个结构体，使用 client_task 属性标记出 common, req, resp 的字段，过程宏根据 common 字段生成 Deref 和 DerefMut 到 common 属性，根据 req 生成 ClientTaskEncode, 根据 resp 生成 ClientTaskDecode。用户需要自己实现余下 RpcClientTask 要求的 action() 和 set_result() 两个方法。

```
#[client_task]
pub struct FileTask {
    #[field(common)]
    common: TaskCommon,
    sender: MTx<Self>,
    #[field(req)]
    req: FileIOReq,
    #[field(resp)]
    resp: Option<FileIOResp>，
    action: i32,
    res: Option<Result<(), RpcError>>,
}

```

Task 可选带有 Request Blob, 作为发送的附加数据，使用 `#[field(req_blob)]` 标识。
该类型需要实现 AsRef[u8]，会自动实现 ClientTaskEncode::get_req_blob().

Task 可选带有 Response Blob, 作为接受的附加数据，使用 `#[field(resp_blob)]` 标识.
会自动实现 ClientTaskDecode::get_resp_blob_mut(), 返回 &mut Option<T>.
resp_blob 字段类型要求是 Option<T>，T 只能属于 io_buffer::Buffer 或者 Vec<u8>，

# RpcServerTask 的设计

```

#[server_task]
pub enum FileTask;

#[server_task_type(IO=1)]
pub struct FileTaskIO {
    #[field(common)]
    common: ServerTaskCommon,
    #[field(req)]
    req: FileIOReq,
    #[field(req_blob)]
    write_data: Option<Buffer>,
    #[field(resp)]
    resp: Option<FileIOResp>，
    #[field(resp_blob)],
    read_data: Option<Buffer>,
    #[field(res)]
    res: Option<Result<(), RpcError>>,
    #[field(noti)],
    done_tx: Option<RpcRespNoti<FileTask>>,
}

```

# Transport 插件

我们假定不同种类的 Transport 相互不知道，也不需要做动态注册等分发逻辑。比如 rdma 是一个 transport, tcp 是另一个 transport。相互不干扰
