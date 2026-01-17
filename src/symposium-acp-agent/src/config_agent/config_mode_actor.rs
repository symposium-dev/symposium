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
use regex::Regex;
use sacp::link::AgentToClient;
use sacp::schema::SessionId;
use sacp::JrConnectionCx;
use std::sync::LazyLock;
use tokio::sync::oneshot;

/// Result of handling menu input.
enum MenuAction {
    /// Exit the menu loop (save or cancel was chosen).
    Done,
    /// Redisplay the menu (state changed).
    Redisplay,
    /// Just wait for more input (invalid command, no state change).
    Continue,
}

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
    ///
    /// The `resume_tx` is an optional oneshot sender that, when dropped, will
    /// signal the conductor to resume processing. If provided, it will be
    /// dropped when the actor exits (either save or cancel).
    pub fn spawn(
        config: SymposiumUserConfig,
        session_id: SessionId,
        config_agent_tx: UnboundedSender<ConfigAgentMessage>,
        resume_tx: Option<oneshot::Sender<()>>,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<Self, sacp::Error> {
        let (tx, rx) = mpsc::channel(32);
        let handle = Self { tx };

        let actor = ConfigModeActor {
            config,
            session_id,
            config_agent_tx,
            rx,
            available_agents: Vec::new(),
            _resume_tx: resume_tx,
        };

        cx.spawn(actor.run())?;

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

/// The config mode actor state.
struct ConfigModeActor {
    config: SymposiumUserConfig,
    session_id: SessionId,
    config_agent_tx: UnboundedSender<ConfigAgentMessage>,
    rx: mpsc::Receiver<ConfigModeInput>,
    available_agents: Vec<AgentListEntry>,
    /// When dropped, signals the conductor to resume. We never send to this,
    /// just hold it until the actor exits.
    _resume_tx: Option<oneshot::Sender<()>>,
}

impl ConfigModeActor {
    /// Main entry point - runs the actor.
    async fn run(mut self) -> Result<(), sacp::Error> {
        // Fetch available agents
        match registry::list_agents().await {
            Ok(agents) => self.available_agents = agents,
            Err(e) => self.send_message(format!("Warning: Failed to fetch registry: {}", e)),
        }

        self.main_menu_loop().await;

        Ok(())
    }

    /// Wait for the next user input.
    async fn next_input(&mut self) -> Option<String> {
        match self.rx.next().await {
            Some(ConfigModeInput::UserInput(text)) => Some(text),
            None => None,
        }
    }

    /// Send a message to the user.
    fn send_message(&self, text: impl Into<String>) {
        self.config_agent_tx
            .unbounded_send(ConfigAgentMessage::ConfigModeOutput(
                self.session_id.clone(),
                ConfigModeOutput::SendMessage(text.into()),
            ))
            .ok();
    }

    /// Signal that configuration is done (save and exit).
    fn done(&self) {
        self.config_agent_tx
            .unbounded_send(ConfigAgentMessage::ConfigModeOutput(
                self.session_id.clone(),
                ConfigModeOutput::Done {
                    config: self.config.clone(),
                },
            ))
            .ok();
    }

    /// Signal that configuration was cancelled.
    fn cancelled(&self) {
        self.config_agent_tx
            .unbounded_send(ConfigAgentMessage::ConfigModeOutput(
                self.session_id.clone(),
                ConfigModeOutput::Cancelled,
            ))
            .ok();
    }

    /// Main menu loop.
    async fn main_menu_loop(&mut self) {
        self.show_main_menu();

        loop {
            let Some(input) = self.next_input().await else {
                return;
            };

            match self.handle_main_menu_input(&input).await {
                MenuAction::Done => return,
                MenuAction::Redisplay => self.show_main_menu(),
                MenuAction::Continue => {}
            }
        }
    }

    /// Handle input in the main menu.
    async fn handle_main_menu_input(&mut self, text: &str) -> MenuAction {
        let text = text.trim();
        let text_upper = text.to_uppercase();

        // Save and exit
        if text_upper == "SAVE" {
            self.done();
            return MenuAction::Done;
        }

        // Cancel without saving
        if text_upper == "CANCEL" {
            self.cancelled();
            return MenuAction::Done;
        }

        // Agent selection
        if text_upper == "A" || text_upper == "AGENT" {
            self.agent_selection_loop().await;
            return MenuAction::Redisplay;
        }

        // Toggle proxy by index
        if let Ok(index) = text.parse::<usize>() {
            if index < self.config.proxies.len() {
                self.config.proxies[index].enabled = !self.config.proxies[index].enabled;
                let proxy = &self.config.proxies[index];
                let status = if proxy.enabled { "enabled" } else { "disabled" };
                self.send_message(format!("Proxy `{}` is now {}.", proxy.name, status));
                return MenuAction::Redisplay;
            } else {
                self.send_message(format!(
                    "Invalid index. Please enter 0-{}.",
                    self.config.proxies.len().saturating_sub(1)
                ));
                return MenuAction::Continue;
            }
        }

        // Move command: "move X to Y"
        static MOVE_RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?i)^move\s+(\d+)\s+to\s+(\d+)$").unwrap());

        if let Some(caps) = MOVE_RE.captures(text) {
            let from: usize = caps[1].parse().unwrap();
            let to: usize = caps[2].parse().unwrap();

            if from < self.config.proxies.len() && to <= self.config.proxies.len() {
                let proxy = self.config.proxies.remove(from);
                let insert_at = if to > from { to - 1 } else { to };
                self.send_message(format!("Moved `{}` from {} to {}.", proxy.name, from, to));
                self.config
                    .proxies
                    .insert(insert_at.min(self.config.proxies.len()), proxy);
                return MenuAction::Redisplay;
            } else {
                self.send_message("Invalid indices for move.");
                return MenuAction::Continue;
            }
        }

        // Unknown command
        self.send_message(format!("Unknown command: `{}`", text));
        MenuAction::Continue
    }

    /// Agent selection loop.
    async fn agent_selection_loop(&mut self) {
        loop {
            self.show_agent_selection();

            let Some(input) = self.next_input().await else {
                return;
            };

            let text = input.trim();
            let text_upper = text.to_uppercase();

            // Back to main menu
            if text_upper == "BACK" || text_upper == "CANCEL" {
                return;
            }

            // Select by index
            if let Ok(index) = text.parse::<usize>() {
                if index < self.available_agents.len() {
                    let agent = &self.available_agents[index];
                    self.config.agent = agent.id.clone();
                    self.send_message(format!("Agent set to `{}`.", agent.name));
                    return;
                } else {
                    self.send_message(format!(
                        "Invalid index. Please enter 0-{}.",
                        self.available_agents.len().saturating_sub(1)
                    ));
                }
                continue;
            }

            self.send_message(format!(
                "Unknown input: `{}`. Enter a number or `back`.",
                text
            ));
        }
    }

    /// Show the main menu.
    fn show_main_menu(&self) {
        let mut msg = String::new();
        msg.push_str("# Symposium Configuration\n\n");

        // Current agent
        msg.push_str("**Agent:** ");
        if self.config.agent.is_empty() {
            msg.push_str("(not configured)\n\n");
        } else {
            // Try to find the agent name
            let agent_name = self
                .available_agents
                .iter()
                .find(|a| a.id == self.config.agent)
                .map(|a| a.name.as_str())
                .unwrap_or(&self.config.agent);
            msg.push_str(&format!("`{}`\n\n", agent_name));
        }

        // Proxies
        msg.push_str("**Proxies:**\n");
        if self.config.proxies.is_empty() {
            msg.push_str("  (none configured)\n");
        } else {
            for (i, proxy) in self.config.proxies.iter().enumerate() {
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
        msg.push_str("  `save` - Save for future sessions\n");
        msg.push_str("  `cancel` - Exit without saving\n");

        self.send_message(msg);
    }

    /// Show the agent selection menu.
    fn show_agent_selection(&self) {
        let mut msg = String::new();
        msg.push_str("# Select Agent\n\n");

        if self.available_agents.is_empty() {
            msg.push_str("No agents available.\n\n");
        } else {
            for (i, agent) in self.available_agents.iter().enumerate() {
                msg.push_str(&format!("`{}` **{}**", i, agent.name));
                if let Some(desc) = &agent.description {
                    msg.push_str(&format!(" - {}", desc));
                }
                msg.push('\n');
            }
            msg.push('\n');
        }

        msg.push_str("Enter a number to select, or `back` to return.\n");

        self.send_message(msg);
    }
}
