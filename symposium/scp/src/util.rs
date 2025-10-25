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

/// Create an internal error from an error value.
/// This is a convenience helper that can be used directly with `map_err`:
/// ```
/// some_result.map_err(scp::util::internal_error)
/// ```
pub fn internal_error(error: impl std::fmt::Display) -> acp::Error {
    acp::Error::new((-32603, error.to_string()))
}
