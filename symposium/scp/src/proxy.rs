use crate::{
    jsonrpc::{
        ChainHandler, Handled, JsonRpcConnection, JsonRpcCx, JsonRpcHandler, JsonRpcRequestCx,
    },
    util::json_cast,
};

mod messages;

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
    async fn handle_request(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        response: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, jsonrpcmsg::Error> {
        if method != "_proxy/successor/receive/request" {
            return Ok(Handled::No(response));
        }

        // We have just received a request from the successor which looks like
        //
        // ```json
        // {
        //    "method": "_proxy/successor/receive/request",
        //    "id": $outer_id,
        //    "params": {
        //        "message": {
        //            "id": $inner_id,
        //            ...
        //        }
        //    }
        // }
        // ```
        //
        // What we want to do is to (1) remember ; (2) forward the inner message
        // to our handler. The handler will send us a response R and we want to
        //
        //
        //
        let messages::FromSuccessorRequest {
            message:
                jsonrpcmsg::Request {
                    jsonrpc: inner_jsonrpc,
                    version: inner_version,
                    method: inner_method,
                    params: inner_params,
                    id: inner_id,
                },
        } = json_cast::<_, messages::FromSuccessorRequest>(params)?;

        // The user will send us a response that is intended for the proxy.
        // We repackage that into a `{message: ...}` struct that embeds
        // the response that will be sent to the proxy.
        let response = response.map(
            {
                let inner_jsonrpc = inner_jsonrpc.clone();
                let inner_version = inner_version.clone();
                let inner_id = inner_id.clone();
                move |response: serde_json::Value| {
                    serde_json::to_value(messages::ToSuccessorResponse {
                        message: jsonrpcmsg::Response {
                            jsonrpc: inner_jsonrpc.clone(),
                            version: inner_version.clone(),
                            result: Some(response),
                            error: None,
                            id: inner_id.clone(),
                        },
                    })
                    .map_err(|_| jsonrpcmsg::Error::internal_error())
                }
            },
            move |error: jsonrpcmsg::Error| {
                serde_json::to_value(messages::ToSuccessorResponse {
                    message: jsonrpcmsg::Response {
                        jsonrpc: inner_jsonrpc.clone(),
                        version: inner_version.clone(),
                        result: None,
                        error: Some(error),
                        id: inner_id.clone(),
                    },
                })
                .map_err(|_| jsonrpcmsg::Error::internal_error())
            },
        );

        self.handler
            .handle_request(&inner_method, &inner_params, response)
            .await
    }

    async fn handle_notification(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        cx: &JsonRpcCx,
    ) -> Result<Handled<()>, jsonrpcmsg::Error> {
        if method != "_proxy/successor/receive/notification" {
            return Ok(Handled::No(()));
        }

        let messages::FromSuccessorRequest {
            message:
                jsonrpcmsg::Request {
                    jsonrpc: _,
                    version: _,
                    method: inner_method,
                    params: inner_params,
                    id: None,
                },
        } = json_cast::<_, messages::FromSuccessorRequest>(params)?
        else {
            // We don't expect an `id` on a notification.
            return Err(jsonrpcmsg::Error::invalid_request());
        };

        self.handler
            .handle_notification(&inner_method, &inner_params, cx)
            .await
    }
}
