use std::{collections::HashMap, net::SocketAddr};

use acp_proxy::McpDisconnectNotification;
use agent_client_protocol as acp;
use agent_client_protocol::McpServer;
use futures::{SinkExt, StreamExt as _, channel::mpsc};
use scp::{JsonRpcConnection, JsonRpcConnectionCx, MessageAndCx};
use tokio::net::TcpStream;
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};
use tracing::info;

use crate::conductor::ConductorMessage;

/// Maintains bridges for MCP message routing.
#[derive(Default)]
pub struct McpBridgeListeners {
    /// Mapping of acp:$UUID URLs to TCP bridge information for MCP message routing
    listeners: HashMap<String, McpBridgeListener>,
}

/// TCP port on which an MCP bridge is listening.
#[derive(Copy, Clone, Debug)]
pub struct McpPort {
    tcp_port: u16,
}

/// Information about an MCP bridge that is listening for connections from MCP clients.
///
/// When a component provides an MCP server with ACP transport (`acp:$UUID`),
/// the conductor spawns a TCP listener and transforms the server spec to
/// use stdio transport.
#[derive(Clone, Debug)]
struct McpBridgeListener {
    /// The TCP port we bound for this bridge
    tcp_port: McpPort,
}

impl McpBridgeListeners {
    /// Transforms MCP servers with `acp:$UUID` URLs for agents that need bridging.
    ///
    /// For each MCP server with an `acp:` URL:
    /// 1. Spawns a TCP listener on an ephemeral port
    /// 2. Stores the mapping for message routing
    /// 3. Transforms the server to use stdio transport pointing to `conductor mcp $PORT`
    ///
    /// Returns the modified NewSessionRequest with transformed MCP servers.
    pub async fn transform_mcp_servers(
        &mut self,
        cx: &JsonRpcConnectionCx,
        mcp_server: &mut McpServer,
        conductor_tx: &mpsc::Sender<ConductorMessage>,
        conductor_command: &[String],
    ) -> Result<(), acp::Error> {
        use agent_client_protocol::McpServer;

        let McpServer::Http { name, url, headers } = mcp_server else {
            return Ok(());
        };

        if !url.starts_with("acp:") {
            return Ok(());
        }

        if !headers.is_empty() {
            return Err(acp::Error::internal_error());
        }

        info!(
            server_name = name,
            acp_url = url,
            "Detected MCP server with ACP transport, spawning TCP bridge"
        );

        // Spawn TCP listener on ephemeral port
        let tcp_port = self
            .spawn_tcp_listener(cx, &url, conductor_tx.clone())
            .await?;

        info!(
            server_name = name,
            acp_url = url,
            tcp_port.tcp_port,
            "Spawned TCP listener for MCP bridge"
        );

        // Transform to stdio transport pointing to conductor mcp process
        // First element is the command, rest are args
        tracing::debug!(
            conductor_command = ?conductor_command,
            "Transforming MCP server to stdio"
        );
        let command = std::path::PathBuf::from(&conductor_command[0]);
        let mut args: Vec<String> = conductor_command[1..].to_vec();
        args.push("mcp".to_string());
        args.push(tcp_port.tcp_port.to_string());

        let transformed = McpServer::Stdio {
            name: name.clone(),
            command,
            args,
            env: vec![],
        };
        *mcp_server = transformed;

        Ok(())
    }

    /// Spawns a TCP listener for an MCP bridge and stores the mapping.
    ///
    /// Binds to `localhost:0` to get an ephemeral port, then stores the
    /// `acp_url → tcp_port` mapping in `self.mcp_bridges`.
    ///
    /// Returns the bound port number.
    async fn spawn_tcp_listener(
        &mut self,
        cx: &JsonRpcConnectionCx,
        acp_url: &String,
        conductor_tx: mpsc::Sender<ConductorMessage>,
    ) -> anyhow::Result<McpPort> {
        use tokio::net::TcpListener;

        // If there is already a listener for the ACP URL, return its TCP port
        if let Some(listener) = self.listeners.get(acp_url) {
            return Ok(listener.tcp_port);
        }

        // Bind to ephemeral port
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let tcp_port = McpPort {
            tcp_port: listener.local_addr()?.port(),
        };

        info!(
            acp_url = acp_url,
            tcp_port.tcp_port, "Bound TCP listener for MCP bridge"
        );

        // Store mapping for message routing (Phase 2b/3)
        self.listeners
            .insert(acp_url.clone(), McpBridgeListener { tcp_port });

        // Phase 2b: Accept connections from `conductor mcp $PORT`
        cx.spawn({
            let acp_url = acp_url.clone();
            let mut conductor_tx = conductor_tx.clone();
            async move {
                info!(
                    acp_url = acp_url,
                    tcp_port.tcp_port, "Waiting for bridge connection"
                );

                // Accept connections
                loop {
                    let (stream, addr) = listener
                        .accept()
                        .await
                        .map_err(acp::Error::into_internal_error)?;

                    let (to_mcp_client_tx, to_mcp_client_rx) = mpsc::channel(128);

                    conductor_tx
                        .send(ConductorMessage::McpConnectionReceived {
                            acp_url: acp_url.clone(),
                            actor: McpBridgeConnectionActor {
                                stream,
                                addr,
                                conductor_tx: conductor_tx.clone(),
                                to_mcp_client_rx,
                            },
                            connection: McpBridgeConnection { to_mcp_client_tx },
                        })
                        .await
                        .map_err(|_| acp::Error::internal_error())?;
                }
            }
        })?;

        Ok(tcp_port)
    }
}

/// Information about an MCP bridge that is listening for connections from MCP clients.
///
/// When a component provides an MCP server with ACP transport (`acp:$UUID`),
/// the conductor spawns a TCP listener and transforms the server spec to
/// use stdio transport.
#[derive(Debug)]
pub struct McpBridgeConnectionActor {
    /// TCP stream to the MCP client
    stream: TcpStream,

    /// Socket address we are connected on
    #[expect(dead_code)]
    addr: SocketAddr,

    /// Sender for messages to the conductor
    conductor_tx: mpsc::Sender<ConductorMessage>,

    /// Receiver for messages from the conductor to the MCP client
    to_mcp_client_rx: mpsc::Receiver<MessageAndCx>,
}

impl McpBridgeConnectionActor {
    /// Run the actor, forwarding incoming messages from the MCP client to the conductor
    /// and outgoing messages from the conductor to the MCP client.
    pub async fn run(mut self, connection_id: String) -> Result<(), acp::Error> {
        info!(connection_id, "Bridge connected");

        let (read_half, write_half) = self.stream.into_split();

        // Establish bidirectional JSON-RPC connection
        // The bridge will send MCP requests (tools/call, etc.) to the conductor
        // The conductor can also send responses back
        let result = JsonRpcConnection::new(write_half.compat_write(), read_half.compat())
            .name(format!("mpc-client-to-conductor({connection_id})"))
            // When we receive a message from the MCP client, forward it to the conductor
            .on_receive_message({
                let mut conductor_tx = self.conductor_tx.clone();
                let connection_id = connection_id.clone();
                async move |message: scp::MessageAndCx| {
                    conductor_tx
                        .send(ConductorMessage::McpClientToMcpServer {
                            connection_id: connection_id.clone(),
                            message,
                        })
                        .await
                        .map_err(|_| acp::Error::internal_error())
                }
            })
            // When we receive messages from the conductor, forward them to the MCP client
            .with_client(async move |mcp_client_cx| {
                while let Some(message) = self.to_mcp_client_rx.next().await {
                    mcp_client_cx.send_proxied_message(message)?;
                }
                Ok(())
            })
            .await;

        self.conductor_tx
            .send(ConductorMessage::McpConnectionDisconnected {
                notification: McpDisconnectNotification { connection_id },
            })
            .await
            .map_err(|_| acp::Error::internal_error())?;

        result
    }
}

/// Information about an MCP bridge that is listening for connections from MCP clients.
///
/// When a component provides an MCP server with ACP transport (`acp:$UUID`),
/// the conductor spawns a TCP listener and transforms the server spec to
/// use stdio transport.
#[derive(Clone, Debug)]
pub struct McpBridgeConnection {
    /// Channel to send messages from MCP server (ACP proxy) to the MCP client (ACP agent).
    to_mcp_client_tx: mpsc::Sender<MessageAndCx>,
}

impl McpBridgeConnection {
    pub async fn send(&mut self, message: MessageAndCx) -> Result<(), acp::Error> {
        self.to_mcp_client_tx
            .send(message)
            .await
            .map_err(|_| acp::Error::internal_error())
    }
}
