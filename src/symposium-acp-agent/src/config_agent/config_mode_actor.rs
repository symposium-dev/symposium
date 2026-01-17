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
    /// If `config` is None, this is initial setup - the actor will go straight
    /// to agent selection and create a default config after selection.
    ///
    /// The `resume_tx` is an optional oneshot sender that, when dropped, will
    /// signal the conductor to resume processing. If provided, it will be
    /// dropped when the actor exits (either save or cancel).
    #[allow(dead_code)] // Convenience wrapper, kept for future non-test use
    pub fn spawn(
        config: Option<SymposiumUserConfig>,
        session_id: SessionId,
        config_agent_tx: UnboundedSender<ConfigAgentMessage>,
        resume_tx: Option<oneshot::Sender<()>>,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<Self, sacp::Error> {
        Self::spawn_with_agents(config, session_id, config_agent_tx, resume_tx, cx, None)
    }

    /// Spawn a new config mode actor with a pre-populated agent list.
    ///
    /// If `config` is None, this is initial setup - the actor will go straight
    /// to agent selection and create a default config after selection.
    ///
    /// If `agents` is Some, skips the registry fetch and uses the provided list.
    /// This is useful for testing.
    pub fn spawn_with_agents(
        config: Option<SymposiumUserConfig>,
        session_id: SessionId,
        config_agent_tx: UnboundedSender<ConfigAgentMessage>,
        resume_tx: Option<oneshot::Sender<()>>,
        cx: &JrConnectionCx<AgentToClient>,
        agents: Option<Vec<AgentListEntry>>,
    ) -> Result<Self, sacp::Error> {
        let (tx, rx) = mpsc::channel(32);
        let handle = Self { tx };

        let actor = ConfigModeActor {
            config,
            session_id,
            config_agent_tx,
            rx,
            available_agents: Vec::new(),
            injected_agents: agents,
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
    /// Current configuration. None means initial setup (no config exists yet).
    /// When None, we go straight to agent selection and create a default config.
    config: Option<SymposiumUserConfig>,
    session_id: SessionId,
    config_agent_tx: UnboundedSender<ConfigAgentMessage>,
    rx: mpsc::Receiver<ConfigModeInput>,
    available_agents: Vec<AgentListEntry>,
    /// If Some, use these agents instead of fetching from registry.
    injected_agents: Option<Vec<AgentListEntry>>,
    /// When dropped, signals the conductor to resume. We never send to this,
    /// just hold it until the actor exits.
    _resume_tx: Option<oneshot::Sender<()>>,
}

impl ConfigModeActor {
    /// Main entry point - runs the actor.
    async fn run(mut self) -> Result<(), sacp::Error> {
        // Use injected agents if provided, otherwise fetch from registry
        if let Some(agents) = self.injected_agents.take() {
            self.available_agents = agents;
        } else {
            match registry::list_agents().await {
                Ok(agents) => self.available_agents = agents,
                Err(e) => self.send_message(format!("Warning: Failed to fetch registry: {}", e)),
            }
        }

        // If no config exists (initial setup), go straight to agent selection
        let mut config = match self.config.take() {
            Some(config) => config,
            None => {
                self.send_message(
                    "Welcome to Symposium!\n\n\
                     No configuration found. Let's set up your AI agent.\n",
                );
                match self.initial_agent_selection().await {
                    Some(config) => config,
                    None => {
                        // User cancelled during initial setup
                        self.cancelled();
                        return Ok(());
                    }
                }
            }
        };

        self.main_menu_loop(&mut config).await;

        Ok(())
    }

    /// Initial agent selection (when no config exists).
    /// Returns the new config if an agent was selected, None if cancelled.
    async fn initial_agent_selection(&mut self) -> Option<SymposiumUserConfig> {
        loop {
            self.show_agent_selection();

            let Some(input) = self.next_input().await else {
                return None;
            };

            let text = input.trim();
            let text_upper = text.to_uppercase();

            // Cancel/back exits without creating config
            if text_upper == "CANCEL" || text_upper == "BACK" {
                return None;
            }

            // Select by index (1-based)
            if let Ok(display_index) = text.parse::<usize>() {
                if display_index >= 1 && display_index <= self.available_agents.len() {
                    let agent = &self.available_agents[display_index - 1];
                    self.send_message(format!("Agent set to `{}`.", agent.name));
                    // Create default config with selected agent
                    return Some(SymposiumUserConfig::with_agent(&agent.id));
                } else if self.available_agents.is_empty() {
                    self.send_message("No agents available.");
                } else {
                    self.send_message(format!(
                        "Invalid index. Please enter 1-{}.",
                        self.available_agents.len()
                    ));
                }
                continue;
            }

            self.send_message(format!(
                "Unknown input: `{}`. Enter a number or `cancel`.",
                text
            ));
        }
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
    fn done(&self, config: &SymposiumUserConfig) {
        self.config_agent_tx
            .unbounded_send(ConfigAgentMessage::ConfigModeOutput(
                self.session_id.clone(),
                ConfigModeOutput::Done {
                    config: config.clone(),
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
    async fn main_menu_loop(&mut self, config: &mut SymposiumUserConfig) {
        self.show_main_menu(config);

        loop {
            let Some(input) = self.next_input().await else {
                return;
            };

            match self.handle_main_menu_input(&input, config).await {
                MenuAction::Done => return,
                MenuAction::Redisplay => self.show_main_menu(config),
                MenuAction::Continue => {}
            }
        }
    }

    /// Handle input in the main menu.
    async fn handle_main_menu_input(
        &mut self,
        text: &str,
        config: &mut SymposiumUserConfig,
    ) -> MenuAction {
        let text = text.trim();
        let text_upper = text.to_uppercase();

        // Save and exit
        if text_upper == "SAVE" {
            self.done(config);
            return MenuAction::Done;
        }

        // Cancel without saving
        if text_upper == "CANCEL" {
            self.cancelled();
            return MenuAction::Done;
        }

        // Agent selection
        if text_upper == "A" || text_upper == "AGENT" {
            self.agent_selection_loop(config).await;
            return MenuAction::Redisplay;
        }

        // Toggle extension by index (1-based)
        if let Ok(display_index) = text.parse::<usize>() {
            if display_index >= 1 && display_index <= config.proxies.len() {
                let index = display_index - 1; // Convert to 0-based
                config.proxies[index].enabled = !config.proxies[index].enabled;
                let proxy = &config.proxies[index];
                let status = if proxy.enabled { "enabled" } else { "disabled" };
                self.send_message(format!("Extension `{}` is now {}.", proxy.name, status));
                return MenuAction::Redisplay;
            } else if config.proxies.is_empty() {
                self.send_message("No extensions configured.");
                return MenuAction::Continue;
            } else {
                self.send_message(format!(
                    "Invalid index. Please enter 1-{}.",
                    config.proxies.len()
                ));
                return MenuAction::Continue;
            }
        }

        // Move command: "move X to Y" or "move X to start/end" (1-based)
        static MOVE_RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?i)^move\s+(\d+)\s+to\s+(\d+|start|end)$").unwrap());

        if let Some(caps) = MOVE_RE.captures(text) {
            let from_display: usize = caps[1].parse().unwrap();
            let to_str = caps[2].to_lowercase();

            // Convert 1-based display index to 0-based
            if from_display < 1 || from_display > config.proxies.len() {
                self.send_message(format!(
                    "Invalid source index. Please enter 1-{}.",
                    config.proxies.len()
                ));
                return MenuAction::Continue;
            }
            let from = from_display - 1;

            // Parse destination: number (1-based), "start", or "end"
            let to = if to_str == "start" {
                0
            } else if to_str == "end" {
                config.proxies.len() - 1
            } else {
                let to_display: usize = to_str.parse().unwrap();
                if to_display < 1 || to_display > config.proxies.len() {
                    self.send_message(format!(
                        "Invalid destination index. Please enter 1-{}, `start`, or `end`.",
                        config.proxies.len()
                    ));
                    return MenuAction::Continue;
                }
                to_display - 1
            };

            let proxy = config.proxies.remove(from);
            let insert_at = if to > from { to } else { to };
            config
                .proxies
                .insert(insert_at.min(config.proxies.len()), proxy.clone());
            self.send_message(format!(
                "Moved `{}` to position {}.",
                proxy.name,
                insert_at + 1
            ));
            return MenuAction::Redisplay;
        }

        // Unknown command
        self.send_message(format!("Unknown command: `{}`", text));
        MenuAction::Continue
    }

    /// Agent selection loop (from main menu, config already exists).
    async fn agent_selection_loop(&mut self, config: &mut SymposiumUserConfig) {
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

            // Select by index (1-based)
            if let Ok(display_index) = text.parse::<usize>() {
                if display_index >= 1 && display_index <= self.available_agents.len() {
                    let agent = &self.available_agents[display_index - 1];
                    config.agent = agent.id.clone();
                    self.send_message(format!("Agent set to `{}`.", agent.name));
                    return;
                } else if self.available_agents.is_empty() {
                    self.send_message("No agents available.");
                } else {
                    self.send_message(format!(
                        "Invalid index. Please enter 1-{}.",
                        self.available_agents.len()
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
    fn show_main_menu(&self, config: &SymposiumUserConfig) {
        let mut msg = String::new();
        msg.push_str("# Configuration\n\n");

        // Current agent
        let agent_name = if config.agent.is_empty() {
            "(not configured)".to_string()
        } else {
            self.available_agents
                .iter()
                .find(|a| a.id == config.agent)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| config.agent.clone())
        };

        msg.push_str(&format!("* **Agent:** {}\n", agent_name));

        // Extensions (formerly proxies)
        msg.push_str("* **Extensions:**\n");
        if config.proxies.is_empty() {
            msg.push_str("    * (none configured)\n");
        } else {
            for (i, proxy) in config.proxies.iter().enumerate() {
                // 1-based indexing for display
                let display_index = i + 1;
                if proxy.enabled {
                    msg.push_str(&format!("    {}. {}\n", display_index, proxy.name));
                } else {
                    msg.push_str(&format!(
                        "    {}. ~~{}~~ (disabled)\n",
                        display_index, proxy.name
                    ));
                }
            }
        }
        msg.push('\n');

        // Commands
        msg.push_str("# Commands\n\n");
        msg.push_str("- `A` or `AGENT` - Select a different agent\n");
        if !config.proxies.is_empty() {
            msg.push_str("- `1`, `2`, ... - Toggle extension enabled/disabled\n");
            msg.push_str("- `move X to Y` - Reorder extensions (or `start`/`end`)\n");
        }
        msg.push_str("- `save` - Save for future sessions\n");
        msg.push_str("- `cancel` - Exit without saving\n");

        self.send_message(msg);
    }

    /// Show the agent selection menu.
    fn show_agent_selection(&self) {
        let mut msg = String::new();
        msg.push_str("# Select Agent\n\n");

        if self.available_agents.is_empty() {
            msg.push_str("No agents available.\n\n");
        } else {
            // Table header
            msg.push_str("| # | Agent | Description |\n");
            msg.push_str("|---|-------|-------------|\n");
            for (i, agent) in self.available_agents.iter().enumerate() {
                // 1-based indexing for display
                let display_index = i + 1;
                let description = agent.description.as_deref().unwrap_or("");
                msg.push_str(&format!(
                    "| {} | {} | {} |\n",
                    display_index, agent.name, description
                ));
            }
            msg.push('\n');
        }

        msg.push_str("Enter a number to select, or `back` to return.\n");

        self.send_message(msg);
    }
}
