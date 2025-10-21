use crossfire::{AsyncRx, MTx, mpsc};
use occams_rpc_core::Codec;
use occams_rpc_core::error::{EncodedErr, RpcErrCodec, RpcError, RpcIntErr};
use occams_rpc_stream::client::{
    ClientCaller, ClientFactory, ClientTask, ClientTaskAction, ClientTaskCommon, ClientTaskDecode,
    ClientTaskDone, ClientTaskEncode,
};
use occams_rpc_stream::proto::RpcAction;
use std::fmt;
use std::io::Write;

#[cfg(any(feature = "tokio", feature = "smol"))]
use captains_log::filter::LogFilter;
#[cfg(any(feature = "tokio", feature = "smol"))]
use occams_rpc_core::ClientConfig;
#[cfg(any(feature = "tokio", feature = "smol"))]
use occams_rpc_stream::client::{ClientFactory, ClientTransport};
#[cfg(any(feature = "tokio", feature = "smol"))]
use std::sync::Arc;

#[cfg(any(feature = "tokio", feature = "smol"))]
pub struct APIClientFactory<C: Codec, T: ClientTransport<Self>> {
    pub logger: Arc<LogFilter>,
    config: ClientConfig,
    #[cfg(feature = "tokio")]
    rt: occams_rpc_tokio::TokioRT,
    #[cfg(feature = "smol")]
    rt: occams_rpc_tokio::SmolRT,
    _phan: std::marker::PhantomData<fn(&T, &C)>,
}

#[cfg(any(feature = "tokio", feature = "smol"))]
impl<C: Codec, T: ClientTransport<Self>> APIClientFactory<C, T> {
    pub fn new(
        config: ClientConfig, #[cfg(feature = "tokio")] rt: occams_rpc_tokio::TokioRT,
        #[cfg(feature = "smol")] rt: occams_rpc_tokio::SmolRT,
    ) -> Self {
        Self { logger: Arc::new(LogFilter::new()), config, rt, _phan: Default::default() }
    }

    #[inline]
    pub fn set_log_level(&self, level: log::Level) {
        self.logger.set_level(level);
    }
}

#[cfg(any(feature = "tokio", feature = "smol"))]
impl<C: Codec, T: ClientTransport<Self>> ClientFactory for APIClientFactory<C, T> {
    type Logger = Arc<LogFilter>;
    type Codec = C;
    type Transport = T;
    type Task = ClientReq;

    #[cfg(feature = "tokio")]
    type IO = occams_rpc_tokio::TokioRT;
    #[cfg(feature = "smol")]
    type IO = occams_rpc_smol::SmolRT;

    #[inline]
    fn spawn_detach<F, R>(&self, f: F)
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static,
    {
        self.rt.spawn_detach(f);
    }

    #[inline]
    fn new_logger(&self, _conn_id: &str) -> Arc<LogFilter> {
        self.logger.clone()
    }

    #[inline]
    fn get_config(&self) -> &ClientConfig {
        &self.config
    }
}

pub struct ClientReq {
    pub common: ClientTaskCommon,
    pub req_msg: Option<Vec<u8>>,
    /// action is in "Service.method" format
    pub action: String,
    pub resp: Option<Vec<u8>>,
    res: Option<Result<(), EncodedErr>>,
    noti: Option<MTx<Self>>,
}

impl ClientTaskEncode for ClientReq {
    #[inline]
    fn encode_req<C: Codec>(&self, _codec: &C, buf: &mut Vec<u8>) -> Result<usize, ()> {
        if let Some(msg) = self.req_msg.as_ref() {
            // The msg is pre encoded
            buf.write_all(msg).expect("append msg");
            Ok(msg.len())
        } else {
            Ok(0)
        }
    }
}

impl ClientTaskDecode for ClientReq {
    #[inline]
    fn decode_resp<C: Codec>(&mut self, _codec: &C, buf: &[u8]) -> Result<(), ()> {
        // Ignore the Codec, as we don't known the resp type yet
        if buf.len() > 0 {
            self.resp.replace(buf.to_vec());
        }
        Ok(())
    }
}

impl ClientTaskDone for ClientReq {
    #[inline]
    fn set_custom_error<C: Codec>(&mut self, _codec: &C, e: EncodedErr) {
        // Ignore the Codec, as we don't known the error type yet
        self.res.replace(Err(e.into()));
    }

    #[inline]
    fn set_rpc_error(&mut self, e: RpcIntErr) {
        self.res.replace(Err(e.into()));
    }

    #[inline]
    fn set_ok(&mut self) {
        self.res.replace(Ok(()));
    }

    #[inline]
    fn done(mut self) {
        let _ = self.noti.take().unwrap().send(self);
    }
}

impl std::ops::Deref for ClientReq {
    type Target = ClientTaskCommon;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.common
    }
}

impl std::ops::DerefMut for ClientReq {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.common
    }
}

impl ClientTaskAction for ClientReq {
    #[inline]
    fn get_action<'a>(&'a self) -> RpcAction<'a> {
        RpcAction::Str(self.action.as_str())
    }
}

impl fmt::Debug for ClientReq {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ClientReq(seq={}, action={})", self.seq, self.action)
    }
}

impl ClientTask for ClientReq {}

pub struct APIClient<F, C>
where
    F: ClientFactory<Task = ClientReq>,
    C: ClientCaller<F>,
{
    caller: C,
    codec: F::Codec,
    done_tx: MTx<ClientReq>,
    done_rx: AsyncRx<ClientReq>,
}

impl<F, C> APIClient<F, C>
where
    F: ClientFactory<Task = ClientReq>,
    C: ClientCaller<F>,
{
    pub fn new(caller: C) -> Self {
        let (tx, rx) = mpsc::bounded_tx_blocking_rx_async::<ClientReq>(1);
        Self { caller, codec: Default::default(), done_tx: tx, done_rx: rx }
    }

    #[inline]
    fn make_req<Req>(&self, service_method: &'static str, req: &Req) -> ClientReq
    where
        Req: serde::Serialize + fmt::Debug,
    {
        let req_buf = self.codec.encode(req).expect("encode");
        ClientReq {
            common: Default::default(),
            req_msg: Some(req_buf),
            action: service_method.to_string(),
            resp: None,
            res: None,
            noti: Some(self.done_tx.clone()),
        }
    }

    pub async fn call<Req, Resp, E>(
        &self, service_method: &'static str, req: &Req,
    ) -> Result<Resp, RpcError<E>>
    where
        Req: serde::Serialize + fmt::Debug,
        Resp: for<'a> serde::Deserialize<'a> + Send + fmt::Debug + 'static + Default,
        E: RpcErrCodec,
    {
        ClientCaller::send_req(&self.caller, self.make_req(service_method, req)).await;
        match self.done_rx.recv().await {
            Ok(mut task) => {
                let res = task.res.take().unwrap();
                match res {
                    Ok(()) => {
                        if let Some(resp) = task.resp {
                            match self.codec.decode(&resp) {
                                Ok(resp_msg) => return Ok(resp_msg),
                                Err(()) => return Err(RpcIntErr::Decode.into()),
                            }
                        } else {
                            return Ok(Resp::default());
                        }
                    }
                    Err(EncodedErr::Rpc(e)) => {
                        return Err(RpcError::Rpc(e));
                    }
                    Err(EncodedErr::Num(n)) => match E::decode(&self.codec, Ok(n)) {
                        Ok(e) => return Err(RpcError::User(e)),
                        Err(()) => return Err(RpcIntErr::Decode.into()),
                    },
                    Err(EncodedErr::Buf(buf)) => match E::decode(&self.codec, Err(&buf)) {
                        Ok(e) => return Err(RpcError::User(e)),
                        Err(()) => return Err(RpcIntErr::Decode.into()),
                    },
                    _ => unreachable!(),
                }
            }
            Err(_) => {
                return Err(RpcIntErr::Internal.into());
            }
        }
    }
}
