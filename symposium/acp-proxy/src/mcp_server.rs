use agent_client_protocol::{self as acp, NewSessionRequest};
use futures::channel::mpsc;
use futures::{FutureExt, future::BoxFuture};
use futures::{SinkExt, StreamExt};
use fxhash::FxHashMap;
use rmcp::ServiceExt;
use scp::{
    Handled, JsonRpcConnection, JsonRpcConnectionCx, JsonRpcHandler, JsonRpcMessage,
    JsonRpcRequestCx, MessageAndCx, UntypedMessage,
};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::{
    JsonRpcCxExt, McpConnectRequest, McpConnectResponse, McpDisconnectNotification,
    McpOverAcpNotification, McpOverAcpRequest, SuccessorNotification, SuccessorRequest,
};

/// Manages MCP services offered to successor proxies and agents.
///
/// Use the [`Self::add_rmcp_server`] method to register MCP servers implemented using the [`rmcp`] crate.
///
/// This struct is a handle to the underlying registry. Cloning the struct produces a second handle to the same registry.
///
/// # Handling requests
///
/// You must add the registery (or a clone of it) to the [`JsonRpcConnection`] so that it can intercept MCP requests.
/// Typically you do this by providing it as an argument to the [`]
#[derive(Clone, Default, Debug)]
pub struct McpServiceRegistry {
    data: Arc<Mutex<McpServiceRegistryData>>,
}

#[derive(Default, Debug)]
struct McpServiceRegistryData {
    registered_by_name: FxHashMap<String, Arc<RegisteredMcpServer>>,
    registered_by_url: FxHashMap<String, Arc<RegisteredMcpServer>>,
    connections: FxHashMap<String, mpsc::Sender<MessageAndCx>>,
}

impl McpServiceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add the MCP server to the registry and return `self`. Useful for chaining.
    /// Equivalent to [`Self::add_rmcp_server`].
    ///
    /// # Parameters
    ///
    /// - `name`: The name of the server.
    /// - `make_service`: A function that creates the service (e.g., `YourService::new`).
    pub fn with_rmcp_server<S>(
        self,
        name: impl ToString,
        make_service: impl Fn() -> S + 'static + Send + Sync,
    ) -> Result<Self, acp::Error>
    where
        S: rmcp::Service<rmcp::RoleServer>,
    {
        self.add_rmcp_server(name, make_service)?;
        Ok(self)
    }

    /// Add an MCP server implemented using the rmcp crate.
    ///
    /// # Parameters
    ///
    /// - `name`: The name of the server.
    /// - `make_service`: A function that creates the service (e.g., `YourService::new`).
    pub fn add_rmcp_server<S>(
        &self,
        name: impl ToString,
        make_service: impl Fn() -> S + 'static + Send + Sync,
    ) -> Result<(), acp::Error>
    where
        S: rmcp::Service<rmcp::RoleServer>,
    {
        struct SpawnRmcpService<F> {
            make_service: F,
        }

        impl<F, S> DynSpawnMcpServer for SpawnRmcpService<F>
        where
            F: Fn() -> S + Send + Sync + 'static,
            S: rmcp::Service<rmcp::RoleServer>,
        {
            fn spawn(
                &self,
                outgoing_bytes: Pin<Box<dyn tokio::io::AsyncWrite + Send>>,
                incoming_bytes: Pin<Box<dyn tokio::io::AsyncRead + Send>>,
            ) -> BoxFuture<'static, Result<(), acp::Error>> {
                let server = (self.make_service)();
                async move {
                    let running_server = server
                        .serve((incoming_bytes, outgoing_bytes))
                        .await
                        .map_err(acp::Error::into_internal_error)?;

                    // Keep the server alive by waiting for it to finish
                    running_server
                        .waiting()
                        .await
                        .map(|_quit_reason| ())
                        .map_err(acp::Error::into_internal_error)
                }
                .boxed()
            }
        }

        let name = name.to_string();
        self.add_mcp_service(name, Arc::new(SpawnRmcpService { make_service }))
    }

    /// Internal helper for adding services, independent of how they are implemented
    fn add_mcp_service(
        &self,
        name: String,
        spawn: Arc<dyn DynSpawnMcpServer>,
    ) -> Result<(), acp::Error> {
        let name = name.to_string();
        if let Some(_) = self.get_registered_server_by_name(&name) {
            return Err(scp::util::internal_error(format!(
                "Server with name '{}' already exists",
                name
            )));
        }

        let uuid = uuid::Uuid::new_v4().to_string();
        let service = Arc::new(RegisteredMcpServer {
            name,
            url: format!("acp:{uuid}"),
            spawn,
        });
        self.insert_registered_server(service);
        Ok(())
    }

    fn insert_registered_server(&self, service: Arc<RegisteredMcpServer>) {
        let mut data = self.data.lock().expect("not poisoned");
        data.registered_by_name
            .insert(service.name.clone(), service.clone());
        data.registered_by_url
            .insert(service.url.clone(), service.clone());
    }

    fn get_registered_server_by_name(&self, name: &str) -> Option<Arc<RegisteredMcpServer>> {
        self.data
            .lock()
            .expect("not poisoned")
            .registered_by_name
            .get(name)
            .cloned()
    }

    fn get_registered_server_by_url(&self, url: &str) -> Option<Arc<RegisteredMcpServer>> {
        self.data
            .lock()
            .expect("not poisoned")
            .registered_by_url
            .get(url)
            .cloned()
    }

    fn insert_connection(&self, connection_id: &str, tx: mpsc::Sender<scp::MessageAndCx>) {
        self.data
            .lock()
            .expect("not poisoned")
            .connections
            .insert(connection_id.to_string(), tx);
    }

    fn get_connection(&self, connection_id: &str) -> Option<mpsc::Sender<scp::MessageAndCx>> {
        self.data
            .lock()
            .expect("not poisoned")
            .connections
            .get(connection_id)
            .cloned()
    }

    fn remove_connection(&self, connection_id: &str) -> bool {
        self.data
            .lock()
            .expect("not poisoned")
            .connections
            .remove(connection_id)
            .is_some()
    }

    async fn handle_connect_request(
        &self,
        result: Result<SuccessorRequest<McpConnectRequest>, agent_client_protocol::Error>,
        request_cx: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, agent_client_protocol::Error> {
        // Check if we parsed this message successfully.
        let SuccessorRequest { request } = match result {
            Ok(request) => request,
            Err(err) => {
                request_cx.respond_with_error(err)?;
                return Ok(Handled::Yes);
            }
        };

        // Check if we have a registered server with the given URL. If not, don't try to handle the request.
        let Some(registered_server) = self.get_registered_server_by_url(&request.acp_url) else {
            return Ok(Handled::No(request_cx));
        };

        let request_cx = request_cx.cast::<McpConnectResponse>();

        // Create a unique connection ID and a channel for future communication
        let connection_id = format!("mcp-over-acp-connection:{}", uuid::Uuid::new_v4());
        let (mcp_server_tx, mut mcp_server_rx) = mpsc::channel(128);
        self.insert_connection(&connection_id, mcp_server_tx);

        // Generate streams
        let (mcp_server_stream, mcp_client_stream) = tokio::io::duplex(8192);
        let (mcp_server_read, mcp_server_write) = tokio::io::split(mcp_server_stream);
        let (mcp_client_read, mcp_client_write) = tokio::io::split(mcp_client_stream);

        // Create JsonRpcConnection for communicating with the server.
        //
        // Every request/notification that the server sends up, we will package up
        // as an McpOverAcpRequest/McpOverAcpNotification and send to our agent.
        //
        // Every request/notification that is sent over `mcp_server_tx` we will
        // send to the MCP server.
        let spawn_results = request_cx
            .spawn(
                JsonRpcConnection::new(mcp_client_write.compat_write(), mcp_client_read.compat())
                    .on_receive_message({
                        let connection_id = connection_id.clone();
                        let outer_cx = request_cx.connection_cx();
                        async move |message: scp::MessageAndCx| {
                            // Wrap the message in McpOverAcp{Request,Notification} and forward to successor
                            let wrapped = message.map(
                                |request, request_cx| {
                                    (
                                        McpOverAcpRequest {
                                            connection_id: connection_id.clone(),
                                            request,
                                        },
                                        request_cx,
                                    )
                                },
                                |notification, cx| {
                                    (
                                        McpOverAcpNotification {
                                            connection_id: connection_id.clone(),
                                            notification,
                                        },
                                        cx,
                                    )
                                },
                            );
                            outer_cx.send_proxied_message(wrapped)
                        }
                    })
                    .with_client({
                        async move |mcp_cx| {
                            while let Some(msg) = mcp_server_rx.next().await {
                                mcp_cx.send_proxied_message(msg)?;
                            }
                            Ok(())
                        }
                    }),
            )
            .and_then(|()| {
                // Spawn MCP server task
                request_cx.spawn(async move {
                    registered_server
                        .spawn
                        .spawn(Box::pin(mcp_server_write), Box::pin(mcp_server_read))
                        .await
                })
            });

        match spawn_results {
            Ok(()) => {
                request_cx.respond(McpConnectResponse { connection_id })?;
                Ok(Handled::Yes)
            }

            Err(err) => {
                request_cx.respond_with_error(err)?;
                Ok(Handled::Yes)
            }
        }
    }

    async fn handle_mcp_over_acp_request(
        &self,
        result: Result<
            SuccessorRequest<McpOverAcpRequest<UntypedMessage>>,
            agent_client_protocol::Error,
        >,
        request_cx: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, agent_client_protocol::Error> {
        // Check if we parsed this message successfully.
        let SuccessorRequest { request } = match result {
            Ok(request) => request,
            Err(err) => {
                request_cx.respond_with_error(err)?;
                return Ok(Handled::Yes);
            }
        };

        // Check if we have a registered server with the given URL. If not, don't try to handle the request.
        let Some(mut mcp_server_tx) = self.get_connection(&request.connection_id) else {
            return Ok(Handled::No(request_cx));
        };

        mcp_server_tx
            .send(MessageAndCx::Request(request.request, request_cx))
            .await
            .map_err(acp::Error::into_internal_error)?;

        Ok(Handled::Yes)
    }

    async fn handle_mcp_over_acp_notification(
        &self,
        result: Result<
            SuccessorNotification<McpOverAcpNotification<UntypedMessage>>,
            agent_client_protocol::Error,
        >,
        notification_cx: JsonRpcConnectionCx,
    ) -> Result<Handled<JsonRpcConnectionCx>, agent_client_protocol::Error> {
        // Check if we parsed this message successfully.
        let SuccessorNotification { notification } = match result {
            Ok(request) => request,
            Err(err) => {
                notification_cx.send_error_notification(err)?;
                return Ok(Handled::Yes);
            }
        };

        // Check if we have a registered server with the given URL. If not, don't try to handle the request.
        let Some(mut mcp_server_tx) = self.get_connection(&notification.connection_id) else {
            return Ok(Handled::No(notification_cx));
        };

        mcp_server_tx
            .send(MessageAndCx::Notification(
                notification.notification,
                notification_cx.clone(),
            ))
            .await
            .map_err(acp::Error::into_internal_error)?;

        Ok(Handled::Yes)
    }

    async fn handle_mcp_disconnect_notification(
        &self,
        result: Result<
            SuccessorNotification<McpDisconnectNotification>,
            agent_client_protocol::Error,
        >,
        notification_cx: JsonRpcConnectionCx,
    ) -> Result<Handled<JsonRpcConnectionCx>, agent_client_protocol::Error> {
        // Check if we parsed this message successfully.
        let SuccessorNotification { notification } = match result {
            Ok(request) => request,
            Err(err) => {
                notification_cx.send_error_notification(err)?;
                return Ok(Handled::Yes);
            }
        };

        // Remove connection if we have it. Otherwise, do not handle the notification.
        if self.remove_connection(&notification.connection_id) {
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(notification_cx))
        }
    }

    async fn handle_new_session_request(
        &self,
        result: Result<NewSessionRequest, agent_client_protocol::Error>,
        request_cx: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, agent_client_protocol::Error> {
        // Check if we parsed this message successfully.
        let mut request = match result {
            Ok(request) => request,
            Err(err) => {
                request_cx.send_error_notification(err)?;
                return Ok(Handled::Yes);
            }
        };

        // Add the MCP servers into the session/new request.
        //
        // Q: Do we care if there are already servers with that name?
        {
            let data = self.data.lock().expect("not poisoned");
            for server in data.registered_by_url.values() {
                request.mcp_servers.push(server.acp_mcp_server());
            }
        }

        // Forward it to the successor.
        request_cx
            .send_request_to_successor(request)
            .forward_to_request_cx(request_cx.cast())?;

        Ok(Handled::Yes)
    }
}

impl JsonRpcHandler for McpServiceRegistry {
    fn describe_chain(&self) -> impl std::fmt::Debug {
        "McpServiceRegistry"
    }

    async fn handle_message(
        &mut self,
        message: scp::MessageAndCx,
    ) -> Result<scp::Handled<scp::MessageAndCx>, agent_client_protocol::Error> {
        match message {
            scp::MessageAndCx::Request(msg, mut cx) => {
                let params = msg.params();

                if let Some(result) =
                    <SuccessorRequest<McpConnectRequest>>::parse_request(cx.method(), params)
                {
                    cx = match self.handle_connect_request(result, cx).await? {
                        Handled::Yes => return Ok(Handled::Yes),
                        Handled::No(cx) => cx,
                    };
                }

                if let Some(result) =
                    <SuccessorRequest<McpOverAcpRequest<UntypedMessage>>>::parse_request(
                        cx.method(),
                        params,
                    )
                {
                    cx = match self.handle_mcp_over_acp_request(result, cx).await? {
                        Handled::Yes => return Ok(Handled::Yes),
                        Handled::No(cx) => cx,
                    };
                }

                if let Some(result) = <NewSessionRequest>::parse_request(cx.method(), params) {
                    cx = match self.handle_new_session_request(result, cx).await? {
                        Handled::Yes => return Ok(Handled::Yes),
                        Handled::No(cx) => cx,
                    };
                }

                Ok(Handled::No(scp::MessageAndCx::Request(msg, cx)))
            }
            scp::MessageAndCx::Notification(msg, mut cx) => {
                let params = msg.params();

                if let Some(result) =
                    <SuccessorNotification<McpOverAcpNotification<UntypedMessage>>>::parse_notification(
                        msg.method(),
                        params,
                    )
                {
                    cx = match self.handle_mcp_over_acp_notification(result, cx).await? {
                        Handled::Yes => return Ok(Handled::Yes),
                        Handled::No(cx) => cx,
                    };
                }

                if let Some(result) =
                    <SuccessorNotification<McpDisconnectNotification>>::parse_notification(
                        msg.method(),
                        params,
                    )
                {
                    cx = match self.handle_mcp_disconnect_notification(result, cx).await? {
                        Handled::Yes => return Ok(Handled::Yes),
                        Handled::No(cx) => cx,
                    };
                }

                Ok(scp::Handled::No(scp::MessageAndCx::Notification(msg, cx)))
            }
        }
    }
}

#[derive(Clone)]
struct RegisteredMcpServer {
    name: String,
    url: String,
    spawn: Arc<dyn DynSpawnMcpServer>,
}

impl RegisteredMcpServer {
    fn acp_mcp_server(&self) -> acp::McpServer {
        acp::McpServer::Http {
            name: self.name.clone(),
            url: self.url.clone(),
            headers: vec![],
        }
    }
}

impl std::fmt::Debug for RegisteredMcpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisteredMcpServer")
            .field("name", &self.name)
            .field("url", &self.url)
            .finish()
    }
}

trait DynSpawnMcpServer: 'static + Send + Sync {
    fn spawn(
        &self,
        outgoing_bytes: Pin<Box<dyn tokio::io::AsyncWrite + Send>>,
        incoming_bytes: Pin<Box<dyn tokio::io::AsyncRead + Send>>,
    ) -> BoxFuture<'static, Result<(), acp::Error>>;
}

impl AsRef<McpServiceRegistry> for McpServiceRegistry {
    fn as_ref(&self) -> &McpServiceRegistry {
        self
    }
}
