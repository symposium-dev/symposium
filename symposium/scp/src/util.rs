use agent_client_protocol as acp;

/// Cast from `N` to `M` by serializing/deserialization to/from JSON.
pub fn json_cast<N, M>(params: N) -> Result<M, acp::Error>
where
    N: serde::Serialize,
    M: serde::de::DeserializeOwned,
{
    let json = serde_json::to_value(params).map_err(|_| acp::Error::parse_error())?;
    let m = serde_json::from_value(json).map_err(|_| acp::Error::parse_error())?;
    Ok(m)
}

pub fn internal_error(message: impl ToString) -> acp::Error {
    acp::Error::internal_error().with_data(message.to_string())
}

pub fn parse_error(message: impl ToString) -> acp::Error {
    acp::Error::parse_error().with_data(message.to_string())
}
