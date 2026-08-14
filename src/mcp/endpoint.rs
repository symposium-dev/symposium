//! Deciding whether a manifest-supplied URL may be connected to.
//!
//! A plugin names the endpoint, so this is the one place in the MCP path where
//! a remote destination is chosen by something other than the user. Two
//! separate checks, because neither alone is enough:
//!
//! * The **URL** is checked before anything is attempted, which catches a
//!   plaintext scheme or a literal private address.
//! * The **resolved addresses** are checked at connect time, because a
//!   hostname that looks public can resolve into the private range - including
//!   the cloud metadata endpoint, whose whole value to an attacker is that it
//!   answers without credentials.
//!
//! Loopback stays reachable over plain HTTP: a locally hosted server is a
//! normal development case, and it is already as trusted as the process.

use std::net::IpAddr;

use reqwest::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointError {
    NotAUrl(String),
    UnsupportedScheme(String),
    PlaintextRemote(String),
    NoHost,
    Blocked { host: String, addr: IpAddr },
    Unresolvable { host: String, detail: String },
}

impl std::fmt::Display for EndpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAUrl(url) => write!(f, "`{url}` is not a valid URL"),
            Self::UnsupportedScheme(scheme) => {
                write!(f, "scheme `{scheme}` is not supported; use https")
            }
            Self::PlaintextRemote(host) => write!(
                f,
                "http is only allowed for localhost; use https for `{host}`"
            ),
            Self::NoHost => write!(f, "the URL names no host"),
            Self::Blocked { host, addr } => write!(
                f,
                "`{host}` resolves to {addr}, which is a private or reserved address"
            ),
            Self::Unresolvable { host, detail } => {
                write!(f, "`{host}` could not be resolved: {detail}")
            }
        }
    }
}

impl std::error::Error for EndpointError {}

/// Check a URL's shape. Does not touch the network.
pub fn check_url(url: &str) -> Result<Url, EndpointError> {
    let parsed = Url::parse(url).map_err(|_| EndpointError::NotAUrl(url.to_string()))?;

    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(EndpointError::UnsupportedScheme(scheme.to_string()));
    }

    let host = parsed.host().ok_or(EndpointError::NoHost)?;
    let literal = match host {
        url::Host::Ipv4(v4) => Some(IpAddr::V4(v4)),
        url::Host::Ipv6(v6) => Some(IpAddr::V6(v6)),
        url::Host::Domain(_) => None,
    };
    let loopback = match literal {
        Some(addr) => addr.is_loopback(),
        None => parsed.host_str().is_some_and(is_loopback_name),
    };

    if scheme == "http" && !loopback {
        return Err(EndpointError::PlaintextRemote(host.to_string()));
    }

    // A literal address is decided here; a hostname waits for resolution.
    if let Some(addr) = literal
        && !loopback
        && !is_allowed(addr)
    {
        return Err(EndpointError::Blocked {
            host: host.to_string(),
            addr,
        });
    }

    Ok(parsed)
}

/// Resolve the host and reject the connection if any answer is private.
///
/// Every resolved address is checked, not just the first: a host answering
/// with one public and one internal address would otherwise pass.
pub async fn check_resolved(url: &Url) -> Result<(), EndpointError> {
    let host = url.host_str().ok_or(EndpointError::NoHost)?;
    if is_loopback_name(host) {
        return Ok(());
    }
    let port = url.port_or_known_default().unwrap_or(443);

    let addrs =
        tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| EndpointError::Unresolvable {
                host: host.to_string(),
                detail: e.to_string(),
            })?;

    for addr in addrs {
        let ip = addr.ip();
        if !is_allowed(ip) {
            return Err(EndpointError::Blocked {
                host: host.to_string(),
                addr: ip,
            });
        }
    }
    Ok(())
}

fn is_loopback_name(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|addr| addr.is_loopback())
}

/// Whether an address is a legitimate destination for a backing server.
///
/// Deliberately an allow-by-exclusion list of every range that is not a
/// public destination, since the interesting targets - link-local metadata
/// services, private LAN ranges - are exactly the ones an SSRF wants.
fn is_allowed(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                // 100.64.0.0/10, carrier-grade NAT.
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64)
                // 192.0.0.0/24, IETF protocol assignments.
                || v4.octets()[..3] == [192, 0, 0]
                // 198.18.0.0/15, benchmarking.
                || (v4.octets()[0] == 198 && (v4.octets()[1] & 0xfe) == 18)
                // 240.0.0.0/4, reserved.
                || v4.octets()[0] >= 240)
        }
        IpAddr::V6(v6) => {
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // fc00::/7, unique local.
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // fe80::/10, link local.
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // IPv4-mapped, which would otherwise bypass the v4 rules.
                || v6.to_ipv4_mapped().is_some_and(|v4| !is_allowed(IpAddr::V4(v4))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_is_accepted() {
        assert!(check_url("https://mcp.example.com/mcp").is_ok());
    }

    #[test]
    fn plaintext_is_rejected_unless_loopback() {
        assert_eq!(
            check_url("http://mcp.example.com/mcp"),
            Err(EndpointError::PlaintextRemote("mcp.example.com".into()))
        );
        assert!(check_url("http://localhost:8080/mcp").is_ok());
        assert!(check_url("http://127.0.0.1:8080/mcp").is_ok());
        assert!(check_url("http://[::1]:8080/mcp").is_ok());
    }

    #[test]
    fn a_non_http_scheme_is_rejected() {
        assert!(matches!(
            check_url("file:///etc/passwd"),
            Err(EndpointError::UnsupportedScheme(_))
        ));
        assert!(matches!(
            check_url("ws://mcp.example.com/mcp"),
            Err(EndpointError::UnsupportedScheme(_))
        ));
    }

    /// The address whose whole value to an attacker is answering without
    /// credentials.
    #[test]
    fn the_cloud_metadata_address_is_blocked() {
        assert!(matches!(
            check_url("https://169.254.169.254/latest/meta-data/"),
            Err(EndpointError::Blocked { .. })
        ));
    }

    #[test]
    fn private_literals_are_blocked() {
        for url in [
            "https://10.0.0.1/mcp",
            "https://192.168.1.1/mcp",
            "https://172.16.0.1/mcp",
            "https://100.64.0.1/mcp",
            "https://[fc00::1]/mcp",
            "https://[fe80::1]/mcp",
        ] {
            assert!(
                matches!(check_url(url), Err(EndpointError::Blocked { .. })),
                "{url} should be blocked, got {:?}",
                check_url(url)
            );
        }
    }

    /// An IPv4-mapped IPv6 literal reaches the same address by another
    /// spelling, so it has to fail the same way.
    #[test]
    fn ipv4_mapped_private_addresses_are_blocked() {
        assert!(matches!(
            check_url("https://[::ffff:10.0.0.1]/mcp"),
            Err(EndpointError::Blocked { .. })
        ));
    }

    #[test]
    fn public_addresses_are_allowed() {
        assert!(check_url("https://1.1.1.1/mcp").is_ok());
        assert!(check_url("https://[2606:4700::1111]/mcp").is_ok());
    }

    #[tokio::test]
    async fn resolution_blocks_a_host_pointing_inside() {
        // Resolves to 127.0.0.1 by convention, standing in for a public name
        // that answers with a private address.
        let url = Url::parse("https://localtest.me/mcp").expect("url");
        match check_resolved(&url).await {
            Err(EndpointError::Blocked { .. }) => {}
            // Without DNS the check cannot run; the unit tests above still
            // cover the decision.
            Err(EndpointError::Unresolvable { .. }) => {}
            other => panic!("expected a block, got {other:?}"),
        }
    }
}
