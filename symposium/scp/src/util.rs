use agent_client_protocol as acp;

pub(crate) fn json_cast<N, M>(params: N) -> Result<M, jsonrpcmsg::Error>
where
    N: serde::Serialize,
    M: serde::de::DeserializeOwned,
{
    let json = serde_json::to_value(params).map_err(|_| jsonrpcmsg::Error::parse_error())?;
    let m = serde_json::from_value(json).map_err(|_| jsonrpcmsg::Error::parse_error())?;
    Ok(m)
}

pub(crate) fn acp_to_jsonrpc_error(err: acp::Error) -> jsonrpcmsg::Error {
    jsonrpcmsg::Error {
        code: err.code,
        message: err.message,
        data: err.data,
    }
}
