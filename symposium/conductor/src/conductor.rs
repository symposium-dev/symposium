use std::pin::Pin;

use futures::{SinkExt, channel::mpsc};
use jsonrpcmsg::Params;
use scp::{
    acp::AcpEditorMessages,
    jsonrpc::{JsonRpcConnection, JsonRpcRequestCx},
    proxy::{AcpConductorMessages, JsonRpcConnectionExt},
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::component::Component;

pub struct Conductor {
    conductor_rx: mpsc::Receiver<ConductorMessage>,
    components: Vec<Component>,
}

impl Conductor {
    pub async fn run(mut proxies: Vec<String>) -> anyhow::Result<()> {
        proxies.reverse();
        let (conductor_tx, conductor_rx) = mpsc::channel(128 /* chosen arbitrarily */);
        Conductor {
            components: Default::default(),
            conductor_rx,
        }
        .launch_proxy(proxies, conductor_tx)
        .await
    }

    fn launch_proxy(
        mut self,
        mut proxies: Vec<String>,
        conductor_tx: mpsc::UnboundedSender<ConductorMessage>,
    ) -> Pin<Box<impl Future<Output = anyhow::Result<()>>>> {
        Box::pin(async move {
            let Some(next_proxy) = proxies.pop() else {
                return self.serve().await;
            };

            let mut child = tokio::process::Command::new(next_proxy)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()?;

            // Take ownership of the streams (can only do this once!)
            let stdin = child.stdin.take().expect("Failed to open stdin");
            let stdout = child.stdout.take().expect("Failed to open stdout");

            JsonRpcConnection::new(stdin.compat_write(), stdout.compat())
                // The proxy can send *editor* messages to us
                .on_receive(AcpEditorMessages(xxx))
                .on_receive(AcpConductorMessages(xxx))
                .with_client(async move |jsonrpccx| {
                    self.components.push(Component { child, jsonrpccx });
                    self.launch_proxy(proxies)
                        .await
                        .map_err(scp::util::internal_error)
                })
                .await
                .map_err(|err| anyhow::anyhow!("{err:?}"))
        })
    }

    async fn serve(self) -> anyhow::Result<()> {
        Ok(())
    }
}

struct EditorCallbacks {
    conductor_tx: mpsc::Sender<ConductorMessage>,
}

struct SuccessorSendCallbacks {
    index: usize,
    conductor_tx: mpsc::Sender<ConductorMessage>,
}

impl scp::proxy::ConductorCallbacks for SuccessorSendCallbacks {
    async fn successor_send_request(
        &mut self,
        args: scp::proxy::ToSuccessorRequest<serde_json::Value>,
        response: JsonRpcRequestCx<scp::proxy::ToSuccessorResponse<serde_json::Value>>,
    ) -> Result<(), agent_client_protocol::Error> {
        self.conductor_tx.send(ConductorMessage::ProxyToSuccessor {
            index: self.index,
            args,
            response,
        });
    }

    async fn successor_send_notification(
        &mut self,
        args: scp::proxy::ToSuccessorNotification<serde_json::Value>,
        cx: &scp::jsonrpc::JsonRpcCx,
    ) -> Result<(), agent_client_protocol::Error> {
        self.conductor_tx.send(ConductorMessage::ProxyToSuccessor {
            index: self.index,
            args,
            response,
        });
    }
}

pub enum ConductorMessage {
    Initialize {
        args: agent_client_protocol::InitializeRequest,
        response: JsonRpcRequestCx<agent_client_protocol::InitializeResponse>,
    },

    ToSuccessorSendRequest {
        index: usize,
        args: scp::proxy::ToSuccessorRequest<serde_json::Value>,
        response: JsonRpcRequestCx<serde_json::Value>,
    },

    ToSuccessorSendNotification {
        index: usize,
        args: scp::proxy::ToSuccessorNotification<serde_json::Value>,
    },
}
