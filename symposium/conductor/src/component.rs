use std::{future::Future, pin::Pin};

use futures::{AsyncRead, AsyncWrite};

use scp::JsonRpcConnectionCx;
use tokio::process::Child;

/// A spawned component in the proxy chain.
///
/// This represents a component that has been launched and is connected
/// to the conductor via JSON-RPC.
pub struct Component {
    /// The child process, if this component was spawned via Command.
    /// This is used to kill the child process when the component is dropped.
    /// None for mock components used in tests.
    pub child: Option<Child>,

    /// The connection context to the component. This is called `agent_cx` because the
    /// component is acting as the conductor's agent.
    pub agent_cx: JsonRpcConnectionCx,
}

impl Drop for Component {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            child.start_kill();
        }
    }
}

/// Specifies how to create a component in the proxy chain.
pub enum ComponentProvider {
    /// Spawn a component by running a shell command.
    Command(String),

    /// Create a mock component for testing (provides byte streams directly).
    #[cfg(any(test, feature = "test-support"))]
    Mock(Box<dyn MockComponent>),
}

/// Trait for creating mock components in tests.
///
/// Mock components provide bidirectional byte streams that the conductor
/// can use to communicate via JSON-RPC, without spawning actual subprocesses.
#[cfg_attr(not(any(test, feature = "test-support")), expect(dead_code))]
pub trait MockComponent: Send {
    /// Create the byte streams for this mock component.
    ///
    /// Returns a pair of streams (outgoing, incoming) from the conductor's perspective:
    /// - outgoing: conductor writes to component
    /// - incoming: conductor reads from component
    fn create(
        self: Box<Self>,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = anyhow::Result<(
                        Pin<Box<dyn AsyncWrite + Send>>,
                        Pin<Box<dyn AsyncRead + Send>>,
                    )>,
                > + Send,
        >,
    >;
}

/// Type alias for the handler function used by MockComponentImpl
pub type MockComponentHandler = Box<
    dyn FnOnce(
            scp::JsonRpcConnection<
                Pin<Box<dyn AsyncWrite + Send>>,
                Pin<Box<dyn AsyncRead + Send>>,
                scp::NullHandler,
            >,
        ) -> Pin<Box<dyn Future<Output = ()>>>
        + Send,
>;

/// Generic mock component implementation that takes a handler function.
///
/// This provides default boilerplate for setting up JSON-RPC connections,
/// allowing tests to focus on the component's behavior.
#[cfg_attr(not(any(test, feature = "test-support")), expect(dead_code))]
pub struct MockComponentImpl {
    handler: MockComponentHandler,
}

#[cfg_attr(not(any(test, feature = "test-support")), expect(dead_code))]
impl MockComponentImpl {
    pub fn new<F, Fut>(handler: F) -> Self
    where
        F: FnOnce(
                scp::JsonRpcConnection<
                    Pin<Box<dyn AsyncWrite + Send>>,
                    Pin<Box<dyn AsyncRead + Send>>,
                    scp::NullHandler,
                >,
            ) -> Fut
            + Send
            + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        Self {
            handler: Box::new(move |conn| Box::pin(handler(conn))),
        }
    }
}

impl MockComponent for MockComponentImpl {
    fn create(
        mut self: Box<Self>,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = anyhow::Result<(
                        Pin<Box<dyn AsyncWrite + Send>>,
                        Pin<Box<dyn AsyncRead + Send>>,
                    )>,
                > + Send,
        >,
    > {
        use tokio::io::duplex;
        use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

        Box::pin(async move {
            // Create two duplex pairs for bidirectional communication
            let (conductor_out, component_in) = duplex(1024);
            let (component_out, conductor_in) = duplex(1024);

            let connection = scp::JsonRpcConnection::new(
                Box::pin(component_out.compat_write()) as Pin<Box<dyn AsyncWrite + Send>>,
                Box::pin(component_in.compat()) as Pin<Box<dyn AsyncRead + Send>>,
            );

            // Spawn local task to run the handler
            let handler = std::mem::replace(&mut self.handler, Box::new(|_| Box::pin(async {})));
            tokio::task::spawn_local(handler(connection));

            // Return conductor's ends of the streams
            Ok((
                Box::pin(conductor_out.compat_write()) as Pin<Box<dyn AsyncWrite + Send>>,
                Box::pin(conductor_in.compat()) as Pin<Box<dyn AsyncRead + Send>>,
            ))
        })
    }
}
