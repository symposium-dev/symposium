use crate::jsonrpc::{Handled, JsonRpcCx, JsonRpcHandler};

use super::JsonRpcRequestCx;

#[derive(Default)]
pub struct NullHandler {}

impl JsonRpcHandler for NullHandler {}

pub struct ChainHandler<H1, H2>
where
    H1: JsonRpcHandler,
    H2: JsonRpcHandler,
{
    handler1: H1,
    handler2: H2,
}

impl<H1, H2> ChainHandler<H1, H2>
where
    H1: JsonRpcHandler,
    H2: JsonRpcHandler,
{
    pub fn new(handler1: H1, handler2: H2) -> Self {
        Self { handler1, handler2 }
    }
}

impl<H1, H2> JsonRpcHandler for ChainHandler<H1, H2>
where
    H1: JsonRpcHandler,
    H2: JsonRpcHandler,
{
    async fn handle_request(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        response: JsonRpcRequestCx<jsonrpcmsg::Response>,
    ) -> Result<Handled<JsonRpcRequestCx<jsonrpcmsg::Response>>, jsonrpcmsg::Error> {
        match self
            .handler1
            .handle_request(method, params, response)
            .await?
        {
            Handled::Yes => Ok(Handled::Yes),
            Handled::No(response) => self.handler2.handle_request(method, params, response).await,
        }
    }

    async fn handle_notification(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        cx: &JsonRpcCx,
    ) -> Result<Handled<()>, jsonrpcmsg::Error> {
        match self
            .handler1
            .handle_notification(method, params, cx)
            .await?
        {
            Handled::Yes => Ok(Handled::Yes),
            Handled::No(()) => self.handler2.handle_notification(method, params, cx).await,
        }
    }
}
