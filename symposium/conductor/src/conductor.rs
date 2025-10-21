use std::pin::Pin;

use futures::{SinkExt, StreamExt, channel::mpsc};
use scp::{
    AcpAgentToClientMessages, JsonRpcConnection, JsonRpcCx, JsonRpcRequestCx, ProxyToConductorMessages,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::component::Component;

pub struct Conductor<OB: AsyncWrite, IB: AsyncRead> {
    outgoing_bytes: OB,
    incoming_bytes: IB,
    conductor_rx: mpsc::Receiver<ConductorMessage>,
    components: Vec<Component>,
}

impl<OB: AsyncWrite, IB: AsyncRead> Conductor<OB, IB> {
    pub async fn run(
        outgoing_bytes: OB,
        incoming_bytes: IB,
        mut proxies: Vec<String>,
    ) -> anyhow::Result<()> {
        proxies.reverse();
        let (conductor_tx, conductor_rx) = mpsc::channel(128 /* chosen arbitrarily */);
        Conductor {
            outgoing_bytes,
            incoming_bytes,
            components: Default::default(),
            conductor_rx,
        }
        .launch_proxy(proxies, conductor_tx)
        .await
    }

    fn launch_proxy(
        mut self,
        mut proxies: Vec<String>,
        conductor_tx: mpsc::Sender<ConductorMessage>,
    ) -> Pin<Box<impl Future<Output = anyhow::Result<()>>>> {
        Box::pin(async move {
            let Some(next_proxy) = proxies.pop() else {
                drop(conductor_tx);
                return self.serve().await;
            };

            let mut child = tokio::process::Command::new(next_proxy)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()?;

            // Take ownership of the streams (can only do this once!)
            let stdin = child.stdin.take().expect("Failed to open stdin");
            let stdout = child.stdout.take().expect("Failed to open stdout");

            let component_index = self.components.len();

            JsonRpcConnection::new(stdin.compat_write(), stdout.compat())
                // The proxy can send *editor* messages to us
                .on_receive(AcpAgentToClientMessages::send_to({
                    let mut conductor_tx = conductor_tx.clone();
                    async move |message| {
                        conductor_tx
                            .send(ConductorMessage::ToEditorMessage {
                                component_index,
                                message,
                            })
                            .await
                    }
                }))
                .on_receive(ProxyToConductorMessages::callback(SuccessorSendCallbacks {
                    component_index,
                    conductor_tx: conductor_tx.clone(),
                }))
                .with_client(async move |jsonrpccx| {
                    self.components.push(Component { child, jsonrpccx });
                    self.launch_proxy(proxies, conductor_tx)
                        .await
                        .map_err(scp::util::internal_error)
                })
                .await
                .map_err(|err| anyhow::anyhow!("{err:?}"))
        })
    }

    async fn serve(self) -> anyhow::Result<()> {
        JsonRpcConnection::new(self.outgoing_bytes, self.incoming_bytes)
            .on_receive(handler)
        while let Some(message) = self.conductor_rx.next().await {
            match message {
                ConductorMessage::Initialize { args, response } => todo!(),
                ConductorMessage::ToEditorMessage {
                    component_index,
                    message,
                } => todo!(),
                ConductorMessage::ToSuccessorSendRequest {
                    component_index,
                    args,
                    response,
                } => todo!(),
                ConductorMessage::ToSuccessorSendNotification { index, args, cx } => todo!(),
            }
        }
    }
}

struct SuccessorSendCallbacks {
    component_index: usize,
    conductor_tx: mpsc::Sender<ConductorMessage>,
}

impl scp::ConductorCallbacks for SuccessorSendCallbacks {
    async fn successor_send_request(
        &mut self,
        args: scp::ToSuccessorRequest<serde_json::Value>,
        response: JsonRpcRequestCx<scp::ToSuccessorResponse<serde_json::Value>>,
    ) -> Result<(), agent_client_protocol::Error> {
        self.conductor_tx
            .send(ConductorMessage::ToSuccessorSendRequest {
                component_index: self.component_index,
                args,
                response,
            })
            .await
            .map_err(agent_client_protocol::Error::into_internal_error)
    }

    async fn successor_send_notification(
        &mut self,
        args: scp::ToSuccessorNotification<serde_json::Value>,
        cx: &scp::JsonRpcCx,
    ) -> Result<(), agent_client_protocol::Error> {
        self.conductor_tx
            .send(ConductorMessage::ToSuccessorSendNotification {
                index: self.component_index,
                args,
                cx: cx.clone(),
            })
            .await
            .map_err(agent_client_protocol::Error::into_internal_error)
    }
}

pub enum ConductorMessage {
    Initialize {
        args: agent_client_protocol::InitializeRequest,
        response: JsonRpcRequestCx<agent_client_protocol::InitializeResponse>,
    },

    ToEditorMessage {
        component_index: usize,
        message: scp::AcpAgentToClientMessage,
    },

    ToSuccessorSendRequest {
        component_index: usize,
        args: scp::ToSuccessorRequest<serde_json::Value>,
        response: JsonRpcRequestCx<scp::ToSuccessorResponse<serde_json::Value>>,
    },

    ToSuccessorSendNotification {
        index: usize,
        args: scp::ToSuccessorNotification<serde_json::Value>,
        cx: JsonRpcCx,
    },
}
