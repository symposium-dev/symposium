//! Signing in to a remote backing server.
//!
//! The meta-server cannot run this itself: it is a child process of the agent
//! whose stdout carries JSON-RPC and which has no terminal, so it can neither
//! open a browser nor show a URL. Authorization therefore happens here, in a
//! command the user runs, and the meta-server only ever reads the token that
//! results.
//!
//! The redirect lands on a loopback listener bound before the request is built,
//! since the port has to appear in the `redirect_uri` the provider is given.

use std::net::{Ipv4Addr, SocketAddr};

use anyhow::{Context, Result, bail};
use rmcp::transport::auth::{AuthorizationRequest, OAuthState};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::config::Symposium;
use crate::mcp::credentials::FileCredentialStore;
use crate::output::Output;

/// How long the user gets to complete the browser flow.
const CALLBACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

pub async fn login(
    sym: &Symposium,
    cwd: &std::path::Path,
    server: &str,
    out: &Output,
) -> Result<()> {
    let url = remote_url(sym, cwd, server).await?;

    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .context("could not bind a local port for the authorization redirect")?;
    let redirect_uri = format!(
        "http://127.0.0.1:{}/callback",
        listener.local_addr()?.port()
    );

    let mut state = OAuthState::new(&url, None)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if let OAuthState::Unauthorized(manager) = &mut state {
        manager.set_credential_store(FileCredentialStore::new(sym.config_dir(), server));
    }

    // The challenge carries the provider's own scope guidance, so the request
    // asks for what the server said it needs rather than a guess.
    let mut request = AuthorizationRequest::new(redirect_uri.clone()).with_client_name("symposium");
    if let Some(challenge) = challenge(sym, cwd, server).await {
        request = request.with_challenge(challenge);
    }

    state
        .start_authorization(request)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let OAuthState::Session(session) = &state else {
        bail!("authorization did not start");
    };
    let auth_url = session.get_authorization_url().to_string();

    out.info(format!("Opening {auth_url}"));
    out.info("Waiting for the browser to come back...".to_string());
    open_browser(&auth_url);

    let callback = tokio::time::timeout(
        CALLBACK_TIMEOUT,
        wait_for_callback(&listener, &redirect_uri),
    )
    .await
    .context("timed out waiting for the authorization redirect")??;

    session
        .handle_callback_url(&callback)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state
        .complete_authorization()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    out.done(format!("{server}: signed in"));
    Ok(())
}

pub async fn logout(sym: &Symposium, server: &str, out: &Output) -> Result<()> {
    use rmcp::transport::auth::CredentialStore;

    let store = FileCredentialStore::new(sym.config_dir(), server);
    if !store.exists() {
        out.info(format!("{server}: not signed in"));
        return Ok(());
    }
    store.clear().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    out.removed(format!("{server}: signed out"));
    Ok(())
}

/// The url of a resolved remote server, by name.
async fn remote_url(sym: &Symposium, cwd: &std::path::Path, server: &str) -> Result<String> {
    let resolution = crate::mcp::resolve::resolve(sym, cwd).await;

    for resolved in &resolution.servers {
        if resolved.name != server {
            continue;
        }
        return match &resolved.transport {
            crate::mcp::resolve::ServerTransport::Http { url, .. } => Ok(url.clone()),
            crate::mcp::resolve::ServerTransport::Stdio { .. } => {
                bail!("`{server}` is a local server; authorization applies to remote servers only")
            }
        };
    }

    if let Some(rejected) = resolution.rejected.iter().find(|r| r.server == server) {
        bail!("`{server}` is not usable: {}", rejected.reason);
    }
    bail!("no MCP server named `{server}` applies to this workspace")
}

/// The server's own `WWW-Authenticate` challenge, if it offers one.
///
/// Obtained by connecting without credentials, which is exactly what the
/// meta-server does; a server that needs no authorization simply connects and
/// reports nothing to carry into the request.
async fn challenge(sym: &Symposium, cwd: &std::path::Path, server: &str) -> Option<String> {
    let resolution = crate::mcp::resolve::resolve(sym, cwd).await;
    let resolved = resolution.servers.iter().find(|s| s.name == server)?;
    let spec = resolved.spawn_spec(sym).await.ok()?;

    match crate::mcp::client::BackingServer::spawn(&spec).await {
        Err(crate::mcp::client::ClientError::AuthRequired { challenge, .. }) => Some(challenge),
        _ => None,
    }
}

/// Wait for the request that carries the authorization code.
///
/// Requests that are not the redirect are answered and ignored rather than
/// taken as the callback: a browser routinely asks for `/favicon.ico`, and
/// treating the first connection as the redirect turns that into a failed login
/// with a misleading "missing code".
async fn wait_for_callback(listener: &TcpListener, redirect_uri: &str) -> Result<String> {
    let base = redirect_uri
        .split('/')
        .take(3)
        .collect::<Vec<_>>()
        .join("/");

    loop {
        let (stream, _) = listener.accept().await.context("no redirect arrived")?;
        let mut stream = BufReader::new(stream);

        let mut request_line = String::new();
        stream
            .read_line(&mut request_line)
            .await
            .context("could not read the redirect request")?;

        // `GET /callback?code=...&state=... HTTP/1.1`
        let target = match request_line.split_whitespace().nth(1) {
            Some(target) => target.to_string(),
            None => continue,
        };
        let carries_code = target
            .split_once('?')
            .is_some_and(|(_, query)| query.split('&').any(|p| p.starts_with("code=")));

        let (status, body) = if carries_code {
            (
                "200 OK",
                "Signed in. You can close this tab and return to the terminal.",
            )
        } else {
            ("404 Not Found", "")
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.get_mut().write_all(response.as_bytes()).await;
        let _ = stream.get_mut().flush().await;

        if carries_code {
            return Ok(format!("{base}{target}"));
        }
    }
}

/// Best effort: the URL is printed either way, so a failure here is not fatal.
fn open_browser(url: &str) {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    // Passed as an argument, never through a shell: the spec calls out shell
    // interpolation of an authorization URL as a command-injection route.
    let _ = std::process::Command::new(opener)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A browser routinely fetches `/favicon.ico`. Treating the first
    /// connection as the redirect turned that into a failed login reporting a
    /// missing code, which is what this guards against.
    #[tokio::test]
    async fn a_stray_request_does_not_consume_the_callback() {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let redirect_uri = format!("http://127.0.0.1:{port}/callback");

        tokio::spawn(async move {
            for request in [
                "GET /favicon.ico HTTP/1.1\r\nHost: localhost\r\n\r\n",
                "GET /callback?code=abc&state=xyz HTTP/1.1\r\nHost: localhost\r\n\r\n",
            ] {
                let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                    .await
                    .expect("connect");
                stream.write_all(request.as_bytes()).await.expect("write");
                let mut sink = Vec::new();
                let _ = tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut sink).await;
            }
        });

        let full = wait_for_callback(&listener, &redirect_uri)
            .await
            .expect("callback");
        assert!(full.contains("code=abc"), "got: {full}");
    }

    #[tokio::test]
    async fn the_callback_url_is_rebuilt_from_the_request_line() {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let redirect_uri = format!("http://127.0.0.1:{port}/callback");

        let client = tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .expect("connect");
            stream
                .write_all(b"GET /callback?code=abc&state=xyz HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .expect("write");
            let mut response = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut response)
                .await
                .expect("read");
            String::from_utf8_lossy(&response).to_string()
        });

        let full = wait_for_callback(&listener, &redirect_uri)
            .await
            .expect("callback");
        assert_eq!(
            full,
            format!("http://127.0.0.1:{port}/callback?code=abc&state=xyz")
        );

        let response = client.await.expect("client");
        assert!(response.starts_with("HTTP/1.1 200 OK"), "got: {response}");
    }
}
