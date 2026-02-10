//! Config mode actor - handles the interactive configuration "phone tree" UI.
//!
//! This actor is spawned when a user enters config mode via `/symposium:config`.
//! It owns the configuration state and processes user input through a simple
//! text-based menu system.

use super::ConfigAgentMessage;
use crate::recommendations::{RecommendationDiff, WorkspaceRecommendations};
use crate::registry::list_agents_with_sources;
use crate::remote_recommendations::{self, save_local_recommendations};
use crate::user_config::{ConfigPaths, GlobalAgentConfig, WorkspaceModsConfig};
use futures::StreamExt;
use futures::channel::mpsc::{self, UnboundedSender};
use regex::Regex;
use sacp::JrConnectionCx;
use sacp::link::AgentToClient;
use sacp::schema::SessionId;
use std::path::PathBuf;
use std::sync::LazyLock;
use symposium_recommendations::{
    ComponentSource, HttpDistribution, LocalDistribution, ModKind, Recommendation,
};
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
#[derive(Debug)]
pub enum ConfigModeOutput {
    /// Send this text to the user.
    SendMessage(String),

    /// Configuration is complete - save and exit.
    Done {
        /// The agent to save globally.
        agent: ComponentSource,
        /// The mods to save per-workspace.
        mods: WorkspaceModsConfig,
    },

    /// User cancelled - exit without saving.
    Cancelled,
}

/// Handle to communicate with the config mode actor.
#[derive(Clone)]
pub struct ConfigModeHandle {
    tx: mpsc::Sender<ConfigModeInput>,
}

/// The starting configuration
enum StartingConfiguration {
    /// An existing configuration with agent and mods
    ExistingConfig {
        agent: ComponentSource,
        mods: WorkspaceModsConfig,
    },

    /// A new workspace - needs agent selection
    NewWorkspace(WorkspaceRecommendations),
}

impl ConfigModeHandle {
    /// Spawn a new config mode actor for reconfiguration.
    ///
    /// Returns a handle for sending input to the actor.
    ///
    /// The `resume_tx` is a oneshot sender that, when dropped, will
    /// signal the conductor to resume processing. It will be dropped
    /// when the actor exits (either save or cancel).
    pub fn spawn_reconfig(
        agent: ComponentSource,
        mods: WorkspaceModsConfig,
        workspace_path: PathBuf,
        config_paths: ConfigPaths,
        session_id: SessionId,
        config_agent_tx: UnboundedSender<ConfigAgentMessage>,
        resume_tx: oneshot::Sender<()>,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<Self, sacp::Error> {
        Self::spawn_inner(
            StartingConfiguration::ExistingConfig { agent, mods },
            workspace_path,
            config_paths,
            None,
            session_id,
            config_agent_tx,
            Some(resume_tx),
            cx,
        )
    }

    /// Spawn a new config mode actor for initial configuration.
    ///
    /// Returns a handle for sending input to the actor.
    ///
    /// This is for initial setup - the actor will use recommendations
    /// to create the initial configuration.
    ///
    /// The `resume_tx` is an optional oneshot sender that, when dropped, will
    /// signal the conductor to resume processing. If provided, it will be
    /// dropped when the actor exits (either save or cancel).
    pub fn spawn_initial_config(
        workspace_path: PathBuf,
        config_paths: ConfigPaths,
        recommendations: WorkspaceRecommendations,
        session_id: SessionId,
        config_agent_tx: UnboundedSender<ConfigAgentMessage>,
        resume_tx: Option<oneshot::Sender<()>>,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<Self, sacp::Error> {
        Self::spawn_inner(
            StartingConfiguration::NewWorkspace(recommendations),
            workspace_path,
            config_paths,
            None,
            session_id,
            config_agent_tx,
            resume_tx,
            cx,
        )
    }

    /// Spawn a config mode actor in diff-only mode.
    ///
    /// This is used when starting a new session with an existing config.
    /// The actor will only handle the recommendation diff prompt, then send
    /// `DiffCompleted` or `DiffCancelled` instead of showing the main menu.
    pub fn spawn_with_recommendations(
        agent: ComponentSource,
        mut mods: WorkspaceModsConfig,
        workspace_path: PathBuf,
        config_paths: ConfigPaths,
        diff: RecommendationDiff,
        session_id: SessionId,
        config_agent_tx: UnboundedSender<ConfigAgentMessage>,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<Self, sacp::Error> {
        diff.apply(&mut mods);
        Self::spawn_inner(
            StartingConfiguration::ExistingConfig { agent, mods },
            workspace_path,
            config_paths,
            Some(diff),
            session_id,
            config_agent_tx,
            None, // No resume_tx for diff-only mode
            cx,
        )
    }

    fn spawn_inner(
        config: StartingConfiguration,
        workspace_path: PathBuf,
        config_paths: ConfigPaths,
        diff: Option<RecommendationDiff>,
        session_id: SessionId,
        config_agent_tx: UnboundedSender<ConfigAgentMessage>,
        resume_tx: Option<oneshot::Sender<()>>,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<Self, sacp::Error> {
        let (tx, rx) = mpsc::channel(32);
        let handle = Self { tx };

        let actor = ConfigModeActor {
            workspace_path,
            config_paths,
            diff: diff.unwrap_or_default(),
            session_id,
            config_agent_tx,
            rx,
            _resume_tx: resume_tx,
        };

        cx.spawn(actor.run(config))?;

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

/// Result of handling the recommendation diff prompt.
enum DiffResult {
    /// Save the updated configuration and exit.
    Save,
    /// Go to the main menu
    Config,
}

/// The config mode actor state.
struct ConfigModeActor {
    /// The workspace this configuration is for.
    workspace_path: PathBuf,
    /// Configuration paths (where to read/write config files).
    config_paths: ConfigPaths,
    /// Diff of the current config vs recommendations.
    diff: RecommendationDiff,
    session_id: SessionId,
    config_agent_tx: UnboundedSender<ConfigAgentMessage>,
    rx: mpsc::Receiver<ConfigModeInput>,
    /// When dropped, signals the conductor to resume. We never send to this,
    /// just hold it until the actor exits.
    _resume_tx: Option<oneshot::Sender<()>>,
}

impl ConfigModeActor {
    /// Main entry point - runs the actor.
    async fn run(mut self, config: StartingConfiguration) -> Result<(), sacp::Error> {
        // Extract or create agent and mods
        let (mut agent, mut mods) = match config {
            StartingConfiguration::ExistingConfig { agent, mods } => (agent, mods),
            StartingConfiguration::NewWorkspace(recommendations) => {
                self.send_message("Welcome to Symposium!\n\n");

                // Check for global agent config
                let global_agent = match GlobalAgentConfig::load(&self.config_paths) {
                    Ok(Some(global)) => Some(global.agent),
                    Ok(None) => None,
                    Err(e) => {
                        tracing::warn!("Failed to load global agent config: {}", e);
                        None
                    }
                };

                let agent = match global_agent {
                    Some(agent) => {
                        self.send_message(&format!(
                            "Using your selected agent: **{}**\n\n",
                            agent.display_name()
                        ));
                        agent
                    }
                    None => {
                        // No global agent - need to select one
                        self.send_message("No agent configured. Let's choose one.\n\n");
                        match self.select_agent().await {
                            Some(agent) => {
                                // Save as global agent
                                let global_config = GlobalAgentConfig::new(agent.clone());
                                if let Err(e) = global_config.save(&self.config_paths).await {
                                    tracing::warn!("Failed to save global agent config: {}", e);
                                }
                                agent
                            }
                            None => {
                                self.send_message("Agent selection cancelled.\n");
                                self.cancelled();
                                return Ok(());
                            }
                        }
                    }
                };

                // Create mods from recommendations
                let mods = WorkspaceModsConfig::from_recommendations(recommendations.mods);

                self.send_message("Configuration created with recommended mods.\n\n");
                (agent, mods)
            }
        };

        // If there is an active diff, present it first
        if !self.diff.is_empty() {
            match self.present_diff(&mut mods).await {
                DiffResult::Save => {
                    self.done(&agent, &mods);
                    return Ok(());
                }
                DiffResult::Config => { /* continue to main menu */ }
            }
        }

        // Enter main menu loop
        self.main_menu_loop(&mut agent, &mut mods).await;

        Ok(())
    }

    /// Handle the recommendation diff prompt.
    /// Returns the result of the interaction.
    async fn present_diff(&mut self, mods: &mut WorkspaceModsConfig) -> DiffResult {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        tracing::debug!(diff = ?self.diff);

        self.send_message("# Recommendations have changed\n\n");

        if !self.diff.to_add.is_empty() {
            self.send_message("The following mods are now recommended:\n");
            for m in &self.diff.to_add {
                self.send_message(&format!(
                    "- {} [{}]\n",
                    m.source.display_name(),
                    m.when.explain_why_added().join(", ")
                ));
            }
            self.send_message("\n");
        }

        if !self.diff.to_remove.is_empty() {
            self.send_message(
                "The following mods were removed as they are no longer recommended:\n",
            );
            for m in &self.diff.to_remove {
                self.send_message(&format!(
                    "- {} [{}]\n",
                    m.source.display_name(),
                    m.when.explain_why_stale().join(", ")
                ));
            }
            self.send_message("\n");
        }

        loop {
            self.send_message("Options:\n");
            self.send_message("* `SAVE` - Accept the new recommendations\n");
            self.send_message("* `IGNORE` - Disable all new recommendations\n");
            self.send_message("* `CONFIG` - Select which mods to enable or make other changes\n");

            let Some(input) = self.next_input().await else {
                return DiffResult::Config;
            };

            let input = input.trim();
            let input_upper = input.to_uppercase();

            match &input_upper[..] {
                "SAVE" => return DiffResult::Save,

                "IGNORE" => {
                    // Disable all the new recommended mods
                    for to_add in &self.diff.to_add {
                        for m in &mut mods.mods {
                            if m.source == to_add.source {
                                m.enabled = false;
                                break;
                            }
                        }
                    }

                    return DiffResult::Save;
                }

                "CONFIG" => {
                    return DiffResult::Config;
                }

                _ => {
                    self.send_message(&format!("Unknown command: `{}`\n", input));
                }
            }

            // Unknown input
        }
    }

    /// Prompt user to select an agent from the registry.
    /// Returns None if cancelled or an error occurred.
    async fn select_agent(&mut self) -> Option<ComponentSource> {
        self.send_message("Fetching available agents...\n");

        let agents = match list_agents_with_sources().await {
            Ok(agents) => agents,
            Err(e) => {
                self.send_message(&format!("Failed to fetch agents: {}\n", e));
                return None;
            }
        };

        if agents.is_empty() {
            self.send_message("No agents available.\n");
            return None;
        }

        // Show the list
        let mut msg = String::new();
        msg.push_str("# Select an Agent\n\n");
        for (i, (entry, _)) in agents.iter().enumerate() {
            msg.push_str(&format!("{}. {}\n", i + 1, entry.name));
        }
        msg.push_str("\nEnter a number to select, or `cancel` to abort:\n");
        self.send_message(msg);

        // Wait for selection
        loop {
            let Some(input) = self.next_input().await else {
                return None;
            };
            let input = input.trim();

            if input.eq_ignore_ascii_case("cancel") {
                return None;
            }

            if let Ok(idx) = input.parse::<usize>() {
                if idx >= 1 && idx <= agents.len() {
                    let (entry, source) = &agents[idx - 1];
                    self.send_message(&format!("Selected: **{}**\n\n", entry.name));
                    return Some(source.clone());
                }
            }

            self.send_message(&format!(
                "Invalid selection. Please enter 1-{} or `cancel`.\n",
                agents.len()
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
    fn done(&self, agent: &ComponentSource, mods: &WorkspaceModsConfig) {
        self.config_agent_tx
            .unbounded_send(ConfigAgentMessage::ConfigModeOutput(
                self.session_id.clone(),
                ConfigModeOutput::Done {
                    agent: agent.clone(),
                    mods: mods.clone(),
                },
            ))
            .ok();
    }

    /// Signal that configuration was cancelled.
    fn cancelled(&mut self) {
        // Regular config mode cancellation
        self.config_agent_tx
            .unbounded_send(ConfigAgentMessage::ConfigModeOutput(
                self.session_id.clone(),
                ConfigModeOutput::Cancelled,
            ))
            .ok();
    }

    /// Main menu loop.
    async fn main_menu_loop(
        &mut self,
        agent: &mut ComponentSource,
        mods: &mut WorkspaceModsConfig,
    ) {
        self.show_main_menu(agent, mods);

        loop {
            let Some(input) = self.next_input().await else {
                return;
            };

            match self.handle_main_menu_input(&input, agent, mods).await {
                MenuAction::Done => return,
                MenuAction::Redisplay => self.show_main_menu(agent, mods),
                MenuAction::Continue => {}
            }
        }
    }

    /// Handle input in the main menu.
    async fn handle_main_menu_input(
        &mut self,
        text: &str,
        agent: &mut ComponentSource,
        mods: &mut WorkspaceModsConfig,
    ) -> MenuAction {
        let text = text.trim();
        let text_upper = text.to_uppercase();

        // Save and exit
        if text_upper == "SAVE" {
            self.done(agent, mods);
            return MenuAction::Done;
        }

        // Cancel without saving
        if text_upper == "CANCEL" {
            self.cancelled();
            return MenuAction::Done;
        }

        // Change agent
        if text_upper == "A" || text_upper == "AGENT" {
            if let Some(new_agent) = self.select_agent().await {
                *agent = new_agent.clone();
                return MenuAction::Redisplay;
            }
            // Selection was cancelled, just redisplay menu
            return MenuAction::Redisplay;
        }

        // Manage local recommendations
        if text_upper == "R" || text_upper == "RECS" || text_upper == "RECOMMENDATIONS" {
            return self.manage_local_recommendations().await;
        }

        // Toggle mod by index (1-based)
        if let Ok(display_index) = text.parse::<usize>() {
            if display_index >= 1 && display_index <= mods.mods.len() {
                let m = &mut mods.mods[display_index - 1];
                m.enabled = !m.enabled;
                self.send_message(format!(
                    "Mod `{}` is now {}.",
                    m.source.display_name(),
                    if m.enabled { "enabled" } else { "disabled" },
                ));
                return MenuAction::Redisplay;
            } else if mods.mods.is_empty() {
                self.send_message("No mods configured.");
                return MenuAction::Continue;
            } else {
                self.send_message(format!(
                    "Invalid index. Please enter 1-{}.",
                    mods.mods.len()
                ));
                return MenuAction::Continue;
            }
        }

        // Move command: "move X to Y" or "move X to start/end" (1-based)
        // Note: Since we use BTreeMap, ordering is by key, not insertion order.
        // For now, we don't support reordering - could add a priority field later.
        static MOVE_RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?i)^move\s+(\d+)\s+to\s+(\d+|start|end)$").unwrap());

        if MOVE_RE.captures(text).is_some() {
            self.send_message("Mod reordering is not yet supported with the new config format.");
            return MenuAction::Continue;
        }

        // Unknown command
        self.send_message(format!("Unknown command: `{}`", text));
        MenuAction::Continue
    }

    /// Manage local recommendations file (`<config>/config/recommendations.toml`).
    /// Allows listing, adding, and removing single recommendation entries.
    async fn manage_local_recommendations(&mut self) -> MenuAction {
        loop {
            let mut msg = String::new();
            msg.push_str("# Local Recommendations\n\n");

            let local_path = self.config_paths.local_reccomendations_path();
            let local_recs = match remote_recommendations::load_local_recommendations(
                &self.config_paths,
            )
            .await
            {
                Ok(recs) => recs,
                Err(e) => {
                    msg.push_str(&format!(
                        "Failed to read {}: {}\n\n",
                        local_path.display(),
                        e
                    ));
                    return MenuAction::Redisplay;
                }
            };
            let mut recs = match local_recs {
                Some(recs) if !recs.mods.is_empty() => {
                    for (m, display_index) in recs.mods.iter().zip(1..) {
                        let name = m.display_name();
                        let mcp = matches!(m.kind, ModKind::MCP)
                            .then_some(" (MCP)")
                            .unwrap_or("");
                        let condition = m.when.is_some().then_some(" (conditional)").unwrap_or("");
                        msg.push_str(&format!(
                            "  {}. {}{}{}\n",
                            display_index, name, mcp, condition
                        ));
                    }
                    recs.mods
                }
                Some(_) | None => {
                    msg.push_str("  * (none configured)\n\n");
                    vec![]
                }
            };

            msg.push_str("Commands:\n");
            msg.push_str("- `ADD` - Add a new recommendation (interactive)\n");
            msg.push_str("- `REMOVE N` - Remove recommendation N\n");
            msg.push_str("- `BACK` - Return to main menu\n");
            self.send_message(msg);

            let Some(input) = self.next_input().await else {
                return MenuAction::Redisplay;
            };
            let input = input.trim();
            let input_upper = input.to_uppercase();

            if input_upper == "BACK" {
                return MenuAction::Redisplay;
            }

            if input_upper == "ADD" {
                self.send_message("Enter kind (`proxy` or `mcp`):");
                let kind = loop {
                    let Some(kind) = self.next_input().await else {
                        return MenuAction::Done;
                    };
                    break match &*kind {
                        "proxy" => ModKind::Proxy,
                        "mcp" => ModKind::MCP,
                        _ => {
                            self.send_message(&format!(
                                "Invalid kind {}. Expected one of `proxy` or `mcp`:",
                                kind
                            ));
                            continue;
                        }
                    };
                };

                // Ask for source type and details and build a ComponentSource directly
                let source = loop {
                    self.send_message(
                        "Enter source type (`local`, `cargo`, `registry`, `http`, `sse`):",
                    );
                    let Some(src) = self.next_input().await else {
                        return MenuAction::Redisplay;
                    };
                    let src = src.trim().to_lowercase();

                    match src.as_str() {
                        "local" => {
                            self.send_message("Enter binary path:");
                            let Some(command) = self.next_input().await else {
                                return MenuAction::Redisplay;
                            };
                            self.send_message("Enter args (space-delimited, or leave blank):");
                            let Some(args_line) = self.next_input().await else {
                                return MenuAction::Redisplay;
                            };
                            let args: Vec<String> = args_line
                                .split_ascii_whitespace()
                                .map(|s| s.to_string())
                                .collect();

                            break ComponentSource::Local(LocalDistribution {
                                command,
                                args,
                                name: None,
                                env: Default::default(),
                            });
                        }

                        "cargo" => {
                            self.send_message("Crate name:");
                            let Some(crate_name) = self.next_input().await else {
                                return MenuAction::Redisplay;
                            };
                            self.send_message("Version (optional, or blank):");
                            let version = match self.next_input().await {
                                Some(v) if !v.trim().is_empty() => Some(v.trim().to_string()),
                                _ => None,
                            };
                            self.send_message("Binary name (optional, or blank):");
                            let binary = match self.next_input().await {
                                Some(b) if !b.trim().is_empty() => Some(b.trim().to_string()),
                                _ => None,
                            };
                            self.send_message("Args (space-delimited, or blank):");
                            let args = match self.next_input().await {
                                Some(a) if !a.trim().is_empty() => a
                                    .split_ascii_whitespace()
                                    .map(|s| s.to_string())
                                    .collect::<Vec<_>>(),
                                _ => Vec::new(),
                            };

                            break ComponentSource::Cargo(
                                symposium_recommendations::CargoDistribution {
                                    crate_name: crate_name.trim().to_string(),
                                    version,
                                    binary,
                                    args,
                                },
                            );
                        }

                        "registry" => {
                            self.send_message("Registry mod id:");
                            let Some(id) = self.next_input().await else {
                                return MenuAction::Redisplay;
                            };
                            break ComponentSource::Registry(id.trim().to_string());
                        }

                        "http" | "sse" => {
                            self.send_message("Name for server:");
                            let Some(name) = self.next_input().await else {
                                return MenuAction::Redisplay;
                            };
                            self.send_message("URL:");
                            let Some(url) = self.next_input().await else {
                                return MenuAction::Redisplay;
                            };
                            let dist = HttpDistribution {
                                name: name.trim().to_string(),
                                url: url.trim().to_string(),
                                headers: vec![],
                            };
                            if src == "sse" {
                                break ComponentSource::Sse(dist);
                            } else {
                                break ComponentSource::Http(dist);
                            }
                        }

                        _ => {
                            self.send_message(&format!("Unknown source type: `{}`.", src));
                            continue;
                        }
                    }
                };

                // Build the Recommendation directly; interactive `when` config is not supported here yet.
                let rec = Recommendation {
                    kind,
                    source,
                    when: None,
                };

                recs.push(rec);

                match save_local_recommendations(&self.config_paths, recs).await {
                    Ok(()) => {
                        self.send_message("Added recommendation to local recommendations file.\n");
                    }
                    Err(e) => {
                        self.send_message(&format!("Failed to save recommendations ({}).\n", e));
                    }
                }

                return MenuAction::Redisplay;
            }

            if input_upper.starts_with("REMOVE") {
                let parts: Vec<_> = input.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(idx) = parts[1].parse::<usize>()
                        && idx >= 1
                        && idx <= recs.len()
                    {
                        let removed = recs.remove(idx);

                        match save_local_recommendations(&self.config_paths, recs).await {
                            Ok(()) => {
                                self.send_message(&format!(
                                    "Removed MCP server `{}`.",
                                    removed.source.display_name()
                                ));
                            }
                            Err(e) => {
                                self.send_message(&format!(
                                    "Failed to remove recommendation ({}).\n",
                                    e
                                ));
                            }
                        }
                    } else {
                        self.send_message("Invalid index for REMOVE.");
                    }
                } else {
                    self.send_message("Usage: REMOVE <N>");
                }

                return MenuAction::Redisplay;
            }

            // Unknown
            self.send_message(&format!("Unknown command: `{}`", input));
        }
    }

    /// Show the main menu.
    fn show_main_menu(&self, agent: &ComponentSource, mods: &WorkspaceModsConfig) {
        let mut msg = String::new();
        msg.push_str("# Configuration\n\n");

        // Current agent (global)
        msg.push_str(&format!("**Agent:** {}\n\n", agent.display_name()));

        // Mods (per-workspace)
        msg.push_str(&format!(
            "**Mods for workspace `{}`:**\n",
            self.workspace_path.display()
        ));
        if mods.mods.is_empty() {
            msg.push_str("  * (none configured)\n");
        } else {
            for (m, display_index) in mods.mods.iter().zip(1..) {
                let name = m.source.display_name();
                let mcp = matches!(m.kind, ModKind::MCP)
                    .then_some(" (MCP)")
                    .unwrap_or("");
                if m.enabled {
                    msg.push_str(&format!("  {}. {}{}\n", display_index, name, mcp));
                } else {
                    msg.push_str(&format!(
                        "  {}. ~~{}{}~~ (disabled)\n",
                        display_index, name, mcp
                    ));
                }
            }
        }
        msg.push('\n');

        // Commands
        msg.push_str("# Commands\n\n");
        msg.push_str("- `AGENT` - Change agent (affects all workspaces)\n");
        msg.push_str("- `RECS` - Update user-defined recommendations\n");
        match mods.mods.len() {
            0 => {}
            1 => msg.push_str("- `1` - Toggle mod enabled/disabled in this workspace\n"),
            n => msg.push_str(&format!(
                "- `1` through `{n}` - Toggle mod enabled/disabled in this workspace\n"
            )),
        }
        msg.push_str("- `SAVE` - Save for future sessions\n");
        msg.push_str("- `CANCEL` - Exit without saving\n");

        self.send_message(msg);
    }
}
