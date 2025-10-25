use agent_client_protocol as acp;

pub fn json_cast<N, M>(params: N) -> Result<M, acp::Error>
where
    N: serde::Serialize,
    M: serde::de::DeserializeOwned,
{
    let json = serde_json::to_value(params).map_err(|_| acp::Error::parse_error())?;
    let m = serde_json::from_value(json).map_err(|_| acp::Error::parse_error())?;
    Ok(m)
}

/// Create an internal error from a string message.
/// This is a convenience helper to avoid the verbose `Error::into_internal_error(std::io::Error::other(...))` pattern.
pub fn internal_error(message: impl ToString) -> acp::Error {
    acp::Error::new((-32603, message.to_string()))
}
