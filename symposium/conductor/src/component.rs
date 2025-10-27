use agent_client_protocol as acp;
use std::pin::Pin;
use tokio_util::compat::{FuturesAsyncReadCompatExt as _, FuturesAsyncWriteCompatExt};

use futures::{AsyncRead, AsyncWrite};

use scp::JsonRpcConnectionCx;
use tokio::process::Child;
use tracing::debug;

/// "Provider" used to spawn components. The [`CommandProvider`] is used from the CLI, but for testing
/// or internal purposes, other comment providers may be created.
pub trait ComponentProvider: Send {
    /// Create a component that will read/write ACP messages from the given streams.
    /// The `cx` can be used to spawn tasks running in the JSON RPC connection.
    fn create(
        &self,
        cx: &JsonRpcConnectionCx,
        outgoing_bytes: Pin<Box<dyn AsyncWrite + Send>>,
        incoming_bytes: Pin<Box<dyn AsyncRead + Send>>,
    ) -> Result<Cleanup, acp::Error>;
}

/// Cleanup enum returned by component provider.
/// Will be dropped when server executed.
#[non_exhaustive]
pub enum Cleanup {
    /// No cleanup required.
    None,
    /// Send "kill" signal to the process.
    KillProcess(Child),
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        match self {
            Cleanup::None => {}
            Cleanup::KillProcess(child) => {
                let _: Result<_, _> = child.start_kill();
            }
        }
    }
}

/// A spawned component in the proxy chain.
///
/// This represents a component that has been launched and is connected
/// to the conductor via JSON-RPC.
pub struct Component {
    /// The child process, if this component was spawned via Command.
    /// This is used to kill the child process when the component is dropped.
    /// None for mock components used in tests.
    pub cleanup: Cleanup,

    /// The connection context to the component. This is called `agent_cx` because the
    /// component is acting as the conductor's agent.
    pub agent_cx: JsonRpcConnectionCx,
}

/// A "command provider" provides a component by running a command and sending ACP messages to/from stdio.
pub struct CommandComponentProvider {
    command: String,
}

impl CommandComponentProvider {
    pub fn new(command: String) -> Box<dyn ComponentProvider> {
        Box::new(CommandComponentProvider { command })
    }
}

impl ComponentProvider for CommandComponentProvider {
    fn create(
        &self,
        cx: &JsonRpcConnectionCx,
        outgoing_bytes: Pin<Box<dyn AsyncWrite + Send>>,
        incoming_bytes: Pin<Box<dyn AsyncRead + Send>>,
    ) -> Result<Cleanup, acp::Error> {
        debug!(command = self.command, "Spawning command");

        let mut child = tokio::process::Command::new(&self.command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(acp::Error::into_internal_error)?;

        // Take ownership of the streams (can only do this once!)
        let mut child_stdin = child.stdin.take().expect("Failed to open stdin");
        let mut child_stdout = child.stdout.take().expect("Failed to open stdout");

        cx.spawn(async move {
            tokio::io::copy(&mut incoming_bytes.compat(), &mut child_stdin)
                .await
                .map_err(acp::Error::into_internal_error)?;
            Ok(())
        });

        cx.spawn(async move {
            tokio::io::copy(&mut child_stdout, &mut outgoing_bytes.compat_write())
                .await
                .map_err(acp::Error::into_internal_error)?;
            Ok(())
        });

        Ok(Cleanup::KillProcess(child))
    }
}
