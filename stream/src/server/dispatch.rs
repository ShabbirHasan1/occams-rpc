use super::RpcSvrReq;
use super::task::*;
use occams_rpc_core::Codec;
use std::marker::PhantomData;

/// ReqDispatch should be a user-defined struct initialized for every connection, by ServerFacts::new_dispatcher.
///
/// ReqDispatch must have Sync, because the connection reader and writer access concurrently.
///
/// A `Codec` should be created and holds inside, shared by the read/write coroutine.
/// If you have encryption in the Codec, it could have shared states.
pub trait ReqDispatch<R: ServerTaskResp>: Send + Sync + Sized + 'static {
    /// Decode and handle the request, called from the connection reader coroutine.
    ///
    /// You can dispatch them to a worker pool.
    /// If you are processing them directly in the connection coroutine, should make sure not
    /// blocking the thread for long.
    /// This is an async fn, but you should avoid waiting as much as possible.
    /// Should return Err(()) when codec decode_req failed.
    fn dispatch_req<'a>(
        &'a self, req: RpcSvrReq<'a>, noti: RespNoti<R>,
    ) -> impl Future<Output = Result<(), ()>> + Send;

    fn get_codec(&self) -> &impl Codec;
}

/// A ReqDispatch trait impl with a closure, only useful for writing tests.
///
/// NOTE: The closure requires Clone.
///
/// # Example
///
/// ```no_compile,ignore
/// use occams_rpc_stream::server::{ServerFacts, ReqDispatch};
/// impl ServerFacts for YourServer {
///
///     ...
///
///     #[inline]
///     fn new_dispatcher(&self) -> impl ReqDispatch<Self::RespTask> {
///         let dispatch_f = move |task: FileServerTask| {
///             async move {
///                 todo!();
///             }
///         }
///         return ReqDispatchClosure::<MsgpCodec, YourServerTask, Self::RespTask, _, _>::new(
///             dispatch_f,
///         );
///     }
/// }
/// ```
pub struct ReqDispatchClosure<C, T, R, H, F>
where
    C: Codec,
    T: ServerTaskDecode<R>,
    R: ServerTaskResp,
    H: FnOnce(T) -> F + Send + Sync + 'static + Clone,
    F: Future<Output = Result<(), ()>> + Send + 'static,
{
    codec: C,
    task_handle: H,
    _phan: PhantomData<fn(&R, &T)>,
}

impl<C, T, R, H, F> ReqDispatchClosure<C, T, R, H, F>
where
    C: Codec,
    T: ServerTaskDecode<R>,
    R: ServerTaskResp,
    H: FnOnce(T) -> F + Send + Sync + 'static + Clone,
    F: Future<Output = Result<(), ()>> + Send + 'static,
{
    #[inline]
    pub fn new(task_handle: H) -> Self {
        Self { codec: C::default(), task_handle, _phan: Default::default() }
    }
}

impl<C, T, R, H, F> ReqDispatch<R> for ReqDispatchClosure<C, T, R, H, F>
where
    C: Codec,
    T: ServerTaskDecode<R>,
    R: ServerTaskResp,
    H: FnOnce(T) -> F + Send + Sync + 'static + Clone,
    F: Future<Output = Result<(), ()>> + Send + 'static,
{
    #[inline]
    async fn dispatch_req<'a>(&'a self, req: RpcSvrReq<'a>, noti: RespNoti<R>) -> Result<(), ()> {
        match <T as ServerTaskDecode<R>>::decode_req(
            &self.codec,
            req.action,
            req.seq,
            req.msg,
            req.blob,
            noti,
        ) {
            Err(_) => {
                error!("action {:?} seq={} decode err", req.action, req.seq);
                return Err(());
            }
            Ok(task) => {
                let handle = self.task_handle.clone();
                if let Err(_) = (handle)(task).await {
                    error!("action {:?} seq={} dispatch err", req.action, req.seq);
                    return Err(());
                }
                Ok(())
            }
        }
    }

    #[inline(always)]
    fn get_codec(&self) -> &impl Codec {
        &self.codec
    }
}
