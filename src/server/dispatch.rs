use super::service::ServiceStatic;
use super::task::{APIServerReq, APIServerResp};
use captains_log::*;
use occams_rpc_core::Codec;
use occams_rpc_stream::{
    proto::RpcAction,
    server::{RpcSvrReq, dispatch::Dispatch, task::RespNoti},
};
use std::marker::PhantomData;
use std::sync::Arc;

pub trait APIDispatchTrait: Send + Sync + 'static + Clone {
    type Codec: Codec;

    fn dispatch(
        &self, req: APIServerReq<Self::Codec>,
    ) -> impl Future<Output = Result<(), ()>> + Send;
}

#[derive(Clone)]
pub struct APIDispatch<D: APIDispatchTrait>(D);

impl<D: APIDispatchTrait> Dispatch for APIDispatch<D> {
    type RespTask = APIServerResp;

    type Codec = D::Codec;

    #[inline]
    async fn dispatch_req<'a>(
        &'a self, codec: &Arc<Self::Codec>, req: RpcSvrReq<'a>, noti: RespNoti<Self::RespTask>,
    ) -> Result<(), ()> {
        if let RpcAction::Str(action) = req.action {
            if let Some((service, method)) = action.split_once('.') {
                return self
                    .0
                    .dispatch(APIServerReq::<Self::Codec> {
                        seq: req.seq,
                        service: service.to_string(),
                        method: method.to_string(),
                        req: Some(req.msg.to_vec()),
                        codec: codec.clone(),
                        noti,
                    })
                    .await;
            }
        }
        warn!("{:?} invalid action", req);
        return Err(());
    }
}

pub type DispatchInline<C, S> = APIDispatch<Inline<C, S>>;

pub struct Inline<C: Codec, S: ServiceStatic<C> + Clone> {
    service: S,
    _phan: PhantomData<fn(&C)>,
}

impl<C: Codec, S: ServiceStatic<C> + Clone> Inline<C, S> {
    #[inline]
    pub fn new(s: S) -> DispatchInline<C, S> {
        APIDispatch(Inline { service: s, _phan: Default::default() })
    }
}

impl<C: Codec, S: ServiceStatic<C> + Clone> Clone for Inline<C, S> {
    #[inline]
    fn clone(&self) -> Self {
        Self { service: self.service.clone(), _phan: Default::default() }
    }
}

impl<C: Codec, S: ServiceStatic<C> + Clone> APIDispatchTrait for Inline<C, S> {
    type Codec = C;

    #[inline]
    async fn dispatch(&self, req: APIServerReq<C>) -> Result<(), ()> {
        self.service.serve(req).await;
        return Ok(());
    }
}
