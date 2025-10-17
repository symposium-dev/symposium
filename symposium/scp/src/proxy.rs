use crate::jsonrpc::{
    ChainHandler, Handled, JsonRpcConnection, JsonRpcCx, JsonRpcHandler, JsonRpcRequestCx,
};

pub trait JsonRpcConnectionExt<H: JsonRpcHandler> {
    fn on_receive_from_successor<H1>(
        self,
        handler: H1,
    ) -> JsonRpcConnection<ChainHandler<H, FromProxyHandler<H1>>>
    where
        H1: JsonRpcHandler;
}

impl<H: JsonRpcHandler> JsonRpcConnectionExt<H> for JsonRpcConnection<H> {
    fn on_receive_from_successor<H1>(
        self,
        handler: H1,
    ) -> JsonRpcConnection<ChainHandler<H, FromProxyHandler<H1>>>
    where
        H1: JsonRpcHandler,
    {
        self.on_receive(FromProxyHandler { handler })
    }
}

pub struct FromProxyHandler<H>
where
    H: JsonRpcHandler,
{
    handler: H,
}

impl<H> JsonRpcHandler for FromProxyHandler<H>
where
    H: JsonRpcHandler,
{
    // async fn handle_request(
    //     &mut self,
    //     method: &str,
    //     params: &Option<jsonrpcmsg::Params>,
    //     response: JsonRpcRequestCx<jsonrpcmsg::Response>,
    // ) -> Result<Handled<JsonRpcRequestCx<jsonrpcmsg::Response>>, jsonrpcmsg::Error> {
    //     if method == "_proxy/successor/receive" {

    //     }
    // }

    // async fn handle_notification(
    //     &mut self,
    //     method: &str,
    //     params: &Option<jsonrpcmsg::Params>,
    //     cx: &JsonRpcCx,
    // ) -> Result<Handled<()>, jsonrpcmsg::Error> {
    //     Ok(Handled::No(()))
    // }
}
