//! Config mode actor - handles the interactive configuration "phone tree" UI.
//!
//! This actor is spawned when a user enters config mode via `/symposium:config`.
//! It owns the configuration state and processes user input through a simple
//! text-based menu system.

use super::ConfigAgentMessage;
use crate::registry::{self, AgentListEntry};
use crate::user_config::SymposiumUserConfig;
use futures::channel::mpsc::{self, UnboundedSender};
use futures::StreamExt;
use sacp::link::AgentToClient;
use sacp::schema::SessionId;
use sacp::JrConnectionCx;

/// Messages sent to the config mode actor.
pub enum ConfigModeInput {
    /// User sent a prompt (the text content).
    UserInput(String),
}

/// Messages sent from the config mode actor back to ConfigAgent.
pub enum ConfigModeOutput {
    /// Send this text to the user.
    SendMessage(String),

    /// Configuration is complete - save and exit.
    Done {
        /// The final configuration to save.
        config: SymposiumUserConfig,
    },

    /// User cancelled - exit without saving.
    Cancelled,
}

/// Handle to communicate with the config mode actor.
#[derive(Clone)]
pub struct ConfigModeHandle {
    tx: mpsc::Sender<ConfigModeInput>,
}

impl ConfigModeHandle {
    /// Spawn a new config mode actor.
    ///
    /// Returns a handle for sending input to the actor.
    pub fn spawn(
        config: SymposiumUserConfig,
        session_id: SessionId,
        config_agent_tx: UnboundedSender<ConfigAgentMessage>,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<Self, sacp::Error> {
        let (tx, rx) = mpsc::channel(32);
        let handle = Self { tx };

        cx.spawn(run_actor(config, session_id, config_agent_tx, rx))?;

        Ok(handle)
    }

    /// Send user input to the actor.
    pub async fn send_input(&self, text: String) -> Result<(), sacp::Error> {
        self.tx
            .clone()
            .try_send(ConfigModeInput::UserInput(text))
            .map_err(|_| sacp::util::internal_error("Config mode actor closed"))
    }
}

/// States in the config mode phone tree.
#[derive(Debug, Clone)]
enum ConfigState {
    /// Main menu - show current config, accept commands.
    MainMenu,

    /// Agent selection - show available agents, pick one.
    SelectAgent,
}

/// The main actor loop.
async fn run_actor(
    mut config: SymposiumUserConfig,
    session_id: SessionId,
    config_agent_tx: UnboundedSender<ConfigAgentMessage>,
    mut rx: mpsc::Receiver<ConfigModeInput>,
) -> Result<(), sacp::Error> {
    // Fetch available agents and extensions
    let (available_agents, available_extensions) = match fetch_registry_data().await {
        Ok(data) => data,
        Err(e) => {
            send_message(
                &config_agent_tx,
                &session_id,
                format!("Warning: Failed to fetch registry: {}", e),
            );
            (Vec::new(), Vec::new())
        }
    };

    let mut state = ConfigState::MainMenu;

    // Show initial menu
    show_main_menu(&config_agent_tx, &session_id, &config, &available_agents);

    // Process input
    while let Some(input) = rx.next().await {
        match input {
            ConfigModeInput::UserInput(text) => {
                let should_continue = handle_input(
                    &text,
                    &mut state,
                    &mut config,
                    &available_agents,
                    &available_extensions,
                    &config_agent_tx,
                    &session_id,
                );

                if !should_continue {
                    break;
                }
            }
        }
    }

    Ok(())
}

/// Fetch available agents and extensions from the registry.
async fn fetch_registry_data(
) -> anyhow::Result<(Vec<AgentListEntry>, Vec<registry::ExtensionListEntry>)> {
    let agents = registry::list_agents().await?;
    let extensions = registry::list_extensions().await?;
    Ok((agents, extensions))
}

/// Handle user input. Returns false if we should exit.
fn handle_input(
    text: &str,
    state: &mut ConfigState,
    config: &mut SymposiumUserConfig,
    available_agents: &[AgentListEntry],
    _available_extensions: &[registry::ExtensionListEntry],
    config_agent_tx: &UnboundedSender<ConfigAgentMessage>,
    session_id: &SessionId,
) -> bool {
    let text = text.trim();

    match state {
        ConfigState::MainMenu => handle_main_menu(
            text,
            state,
            config,
            available_agents,
            config_agent_tx,
            session_id,
        ),
        ConfigState::SelectAgent => handle_select_agent(
            text,
            state,
            config,
            available_agents,
            config_agent_tx,
            session_id,
        ),
    }
}

/// Handle input in the main menu state.
fn handle_main_menu(
    text: &str,
    state: &mut ConfigState,
    config: &mut SymposiumUserConfig,
    available_agents: &[AgentListEntry],
    config_agent_tx: &UnboundedSender<ConfigAgentMessage>,
    session_id: &SessionId,
) -> bool {
    let text_upper = text.to_uppercase();

    // Exit commands
    if text_upper == "EXIT" || text_upper == "DONE" || text_upper == "QUIT" {
        config_agent_tx
            .unbounded_send(ConfigAgentMessage::ConfigModeOutput(
                session_id.clone(),
                ConfigModeOutput::Done {
                    config: config.clone(),
                },
            ))
            .ok();
        return false;
    }

    // Cancel without saving
    if text_upper == "CANCEL" {
        config_agent_tx
            .unbounded_send(ConfigAgentMessage::ConfigModeOutput(
                session_id.clone(),
                ConfigModeOutput::Cancelled,
            ))
            .ok();
        return false;
    }

    // Agent selection
    if text_upper == "A" || text_upper == "AGENT" {
        *state = ConfigState::SelectAgent;
        show_agent_selection(config_agent_tx, session_id, available_agents);
        return true;
    }

    // Toggle proxy by index
    if let Ok(index) = text.parse::<usize>() {
        if index < config.proxies.len() {
            config.proxies[index].enabled = !config.proxies[index].enabled;
            let proxy = &config.proxies[index];
            let status = if proxy.enabled { "enabled" } else { "disabled" };
            send_message(
                config_agent_tx,
                session_id,
                format!("Proxy `{}` is now {}.", proxy.name, status),
            );
            show_main_menu(config_agent_tx, session_id, config, available_agents);
        } else {
            send_message(
                config_agent_tx,
                session_id,
                format!(
                    "Invalid index. Please enter 0-{}.",
                    config.proxies.len().saturating_sub(1)
                ),
            );
        }
        return true;
    }

    // Move command: "move X to Y"
    if let Some(rest) = text_upper.strip_prefix("MOVE ") {
        if let Some((from, to)) = parse_move_command(rest) {
            if from < config.proxies.len() && to <= config.proxies.len() {
                let proxy = config.proxies.remove(from);
                let insert_at = if to > from { to - 1 } else { to };
                send_message(
                    config_agent_tx,
                    session_id,
                    format!("Moved `{}` from {} to {}.", proxy.name, from, to),
                );
                config
                    .proxies
                    .insert(insert_at.min(config.proxies.len()), proxy);
                show_main_menu(config_agent_tx, session_id, config, available_agents);
            } else {
                send_message(config_agent_tx, session_id, "Invalid indices for move.");
            }
        } else {
            send_message(
                config_agent_tx,
                session_id,
                "Usage: `move X to Y` where X and Y are proxy indices.",
            );
        }
        return true;
    }

    // Unknown command
    send_message(
        config_agent_tx,
        session_id,
        format!("Unknown command: `{}`", text),
    );
    show_main_menu(config_agent_tx, session_id, config, available_agents);
    true
}

/// Parse "X to Y" from a move command.
fn parse_move_command(rest: &str) -> Option<(usize, usize)> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() == 3 && parts[1].to_uppercase() == "TO" {
        let from = parts[0].parse().ok()?;
        let to = parts[2].parse().ok()?;
        Some((from, to))
    } else {
        None
    }
}

/// Handle input in the agent selection state.
fn handle_select_agent(
    text: &str,
    state: &mut ConfigState,
    config: &mut SymposiumUserConfig,
    available_agents: &[AgentListEntry],
    config_agent_tx: &UnboundedSender<ConfigAgentMessage>,
    session_id: &SessionId,
) -> bool {
    let text_upper = text.to_uppercase();

    // Back to main menu
    if text_upper == "BACK" || text_upper == "CANCEL" {
        *state = ConfigState::MainMenu;
        show_main_menu(config_agent_tx, session_id, config, available_agents);
        return true;
    }

    // Select by index
    if let Ok(index) = text.parse::<usize>() {
        if index < available_agents.len() {
            let agent = &available_agents[index];
            config.agent = agent.id.clone();
            send_message(
                config_agent_tx,
                session_id,
                format!("Agent set to `{}`.", agent.name),
            );
            *state = ConfigState::MainMenu;
            show_main_menu(config_agent_tx, session_id, config, available_agents);
        } else {
            send_message(
                config_agent_tx,
                session_id,
                format!(
                    "Invalid index. Please enter 0-{}.",
                    available_agents.len().saturating_sub(1)
                ),
            );
        }
        return true;
    }

    // Unknown
    send_message(
        config_agent_tx,
        session_id,
        format!("Unknown input: `{}`. Enter a number or `back`.", text),
    );
    true
}

/// Show the main menu.
fn show_main_menu(
    config_agent_tx: &UnboundedSender<ConfigAgentMessage>,
    session_id: &SessionId,
    config: &SymposiumUserConfig,
    available_agents: &[AgentListEntry],
) {
    let mut msg = String::new();
    msg.push_str("# Symposium Configuration\n\n");

    // Current agent
    msg.push_str("**Agent:** ");
    if config.agent.is_empty() {
        msg.push_str("(not configured)\n\n");
    } else {
        // Try to find the agent name
        let agent_name = available_agents
            .iter()
            .find(|a| a.id == config.agent)
            .map(|a| a.name.as_str())
            .unwrap_or(&config.agent);
        msg.push_str(&format!("`{}`\n\n", agent_name));
    }

    // Proxies
    msg.push_str("**Proxies:**\n");
    if config.proxies.is_empty() {
        msg.push_str("  (none configured)\n");
    } else {
        for (i, proxy) in config.proxies.iter().enumerate() {
            let status = if proxy.enabled { "✓" } else { "✗" };
            msg.push_str(&format!("  `{}` [{}] {}\n", i, status, proxy.name));
        }
    }
    msg.push('\n');

    // Commands
    msg.push_str("**Commands:**\n");
    msg.push_str("  `A` or `AGENT` - Select a different agent\n");
    msg.push_str("  `0`, `1`, ... - Toggle proxy enabled/disabled\n");
    msg.push_str("  `move X to Y` - Reorder proxies\n");
    msg.push_str("  `done` - Save and exit\n");
    msg.push_str("  `cancel` - Exit without saving\n");

    send_message(config_agent_tx, session_id, msg);
}

/// Show the agent selection menu.
fn show_agent_selection(
    config_agent_tx: &UnboundedSender<ConfigAgentMessage>,
    session_id: &SessionId,
    available_agents: &[AgentListEntry],
) {
    let mut msg = String::new();
    msg.push_str("# Select Agent\n\n");

    if available_agents.is_empty() {
        msg.push_str("No agents available.\n\n");
    } else {
        for (i, agent) in available_agents.iter().enumerate() {
            msg.push_str(&format!("`{}` **{}**", i, agent.name));
            if let Some(desc) = &agent.description {
                msg.push_str(&format!(" - {}", desc));
            }
            msg.push('\n');
        }
        msg.push('\n');
    }

    msg.push_str("Enter a number to select, or `back` to return.\n");

    send_message(config_agent_tx, session_id, msg);
}

/// Send a message to the user via ConfigAgent.
fn send_message(
    config_agent_tx: &UnboundedSender<ConfigAgentMessage>,
    session_id: &SessionId,
    text: impl Into<String>,
) {
    config_agent_tx
        .unbounded_send(ConfigAgentMessage::ConfigModeOutput(
            session_id.clone(),
            ConfigModeOutput::SendMessage(text.into()),
        ))
        .ok();
}
