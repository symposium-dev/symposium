//! Uberconductor actor - manages conductor lifecycle.
//!
//! This actor:
//! - Receives requests to create/get conductors for configurations
//! - Maintains a map of config -> conductor handle
//! - Spawns new conductors as needed
//! - Forwards new session requests to the appropriate conductor

use super::conductor_actor::ConductorHandle;
use super::ConfigAgentMessage;
use crate::registry::ComponentSource;
use crate::user_config::ModConfig;
use futures::channel::mpsc::UnboundedSender;
use fxhash::FxHashMap;
use sacp::link::AgentToClient;
use sacp::schema::{NewSessionRequest, NewSessionResponse};
use sacp::{JrConnectionCx, JrRequestCx};
use std::path::PathBuf;
use tokio::sync::mpsc;

/// Messages that can be sent to the UberconductorActor.
pub enum UberconductorMessage {
    /// Create/get a conductor for this config and forward the session request to it.
    NewSession {
        workspace_path: PathBuf,
        agent: ComponentSource,
        mods: Vec<ModConfig>,
        request: NewSessionRequest,
        request_cx: JrRequestCx<NewSessionResponse>,
    },
}

/// Handle for communicating with the UberconductorActor.
#[derive(Clone)]
pub struct UberconductorHandle {
    tx: mpsc::Sender<UberconductorMessage>,
}

impl UberconductorHandle {
    /// Spawn a new uberconductor actor.
    pub fn spawn(
        trace_dir: Option<PathBuf>,
        config_agent_tx: UnboundedSender<ConfigAgentMessage>,
        client_cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<Self, sacp::Error> {
        let (tx, rx) = mpsc::channel(32);

        client_cx.spawn(run_actor(trace_dir, config_agent_tx, client_cx.clone(), rx))?;

        Ok(Self { tx })
    }

    /// Request a new session with the given agent and mods.
    pub async fn new_session(
        &self,
        workspace_path: PathBuf,
        agent: ComponentSource,
        mods: Vec<ModConfig>,
        request: NewSessionRequest,
        request_cx: JrRequestCx<NewSessionResponse>,
    ) -> Result<(), sacp::Error> {
        self.tx
            .send(UberconductorMessage::NewSession {
                workspace_path,
                agent,
                mods,
                request,
                request_cx,
            })
            .await
            .map_err(|_| sacp::util::internal_error("Uberconductor actor closed"))
    }
}

/// The main actor loop.
async fn run_actor(
    trace_dir: Option<PathBuf>,
    config_agent_tx: UnboundedSender<ConfigAgentMessage>,
    client_cx: JrConnectionCx<AgentToClient>,
    mut rx: mpsc::Receiver<UberconductorMessage>,
) -> Result<(), sacp::Error> {
    // Key conductors by workspace path - each workspace gets its own conductor
    let mut conductors: FxHashMap<PathBuf, ConductorHandle> = FxHashMap::default();

    while let Some(message) = rx.recv().await {
        match message {
            UberconductorMessage::NewSession {
                workspace_path,
                agent,
                mods,
                request,
                request_cx,
            } => {
                // Get or create conductor for this workspace
                let handle = match conductors.get(&workspace_path) {
                    Some(handle) => handle.clone(),
                    None => {
                        let handle = ConductorHandle::spawn(
                            workspace_path.clone(),
                            agent,
                            mods,
                            trace_dir.as_ref(),
                            config_agent_tx.clone(),
                            &client_cx,
                        )
                        .await?;
                        conductors.insert(workspace_path.clone(), handle.clone());
                        handle
                    }
                };

                // Forward the session request to the conductor
                // The conductor will send NewSessionCreated back to ConfigAgent
                handle.send_new_session(request, request_cx).await?;
            }
        }
    }

    tracing::debug!("Uberconductor actor shutting down");
    Ok(())
}
