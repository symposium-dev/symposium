//! Structured report layer.
//!
//! Emits user-facing events during `cargo agents` commands as tracing events
//! carrying a single `report` field whose value is a serialized
//! `ReportEvent`. A custom tracing layer picks these up and either
//! pretty-prints them (`--verbose`) or accumulates JSON (`--json`).

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// The output mode for the report layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportMode {
    /// Pretty-print info-level events to stdout as they arrive.
    Normal,
    /// Pretty-print all events (info + debug) to stderr as they arrive.
    Verbose,
    /// Accumulate events, emit as a JSON array at the end.
    Json,
}

/// A structured event emitted during command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportEvent {
    /// A plugin was considered and either matched or was skipped.
    PluginConsidered {
        plugin: String,
        matched: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// A skill group within a plugin was considered.
    SkillGroupConsidered {
        plugin: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        group_crates: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        matched: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        skills_found: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// A directory was searched for SKILL.md files.
    SkillSourceSearched {
        plugin: String,
        source: String,
        path: String,
        skills_found: usize,
    },

    /// An individual skill was evaluated.
    SkillConsidered {
        skill: String,
        plugin: String,
        matched: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// A skill was installed to an agent's directory.
    SkillInstalled {
        skill: String,
        agent: String,
        dest: String,
    },

    /// A stale skill directory was removed.
    SkillRemoved { path: String },

    /// A hook was registered for an agent.
    HookRegistered { agent: String, hook: String },

    /// A user-authored skill was propagated to an agent.
    SkillPropagated {
        skill: String,
        agent: String,
        dest: String,
    },

    /// An MCP server was registered for an agent.
    McpServerRegistered { agent: String, server: String },

    /// Informational message.
    Info { message: String },

    /// A non-fatal warning.
    Warning { message: String },

    /// An installed source config entry was added, updated, removed, or found unchanged.
    InstalledSourceChanged {
        registry: String,
        source: String,
        status: String,
    },

    // ── Hook dispatch events ─────────────────────────────────────────
    /// A plugin hook was considered for dispatch.
    HookConsidered {
        plugin: String,
        hook: String,
        event: String,
        selected: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// A plugin hook was dispatched (process spawned).
    HookDispatched {
        plugin: String,
        hook: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    // ── Plugin validate/list events ──────────────────────────────────
    /// A plugin or skill was validated.
    Validated {
        path: String,
        item_kind: String,
        valid: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        warning: Option<String>,
    },

    /// A provider was listed with its plugins.
    ProviderListed {
        name: String,
        source_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        plugins: Vec<String>,
    },
}

impl std::fmt::Display for ReportEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&serde_json::to_string(self).unwrap())
    }
}

impl ReportEvent {
    fn format_human(&self) -> String {
        match self {
            Self::PluginConsidered {
                plugin,
                matched,
                reason,
            } => {
                if *matched {
                    format!("  plugin {plugin}: matched")
                } else {
                    let r = reason.as_deref().unwrap_or("predicates not satisfied");
                    format!("  plugin {plugin}: skipped ({r})")
                }
            }
            Self::SkillGroupConsidered {
                plugin,
                group_crates,
                source,
                matched,
                skills_found,
                reason,
            } => {
                let crates_str = group_crates.as_deref().unwrap_or("*");
                let source_str = source.as_deref().unwrap_or("unknown");
                if *matched {
                    let count = skills_found.unwrap_or(0);
                    format!(
                        "    group [{crates_str}] in {plugin}: matched, source={source_str}, {count} skill(s) found"
                    )
                } else {
                    let r = reason.as_deref().unwrap_or("predicates not satisfied");
                    format!("    group [{crates_str}] in {plugin}: skipped ({r})")
                }
            }
            Self::SkillSourceSearched {
                plugin,
                source,
                path,
                skills_found,
            } => {
                format!("      searched {source} ({plugin}): {path} → {skills_found} skill(s)")
            }
            Self::SkillConsidered {
                skill,
                plugin,
                matched,
                reason,
            } => {
                if *matched {
                    format!("      skill {skill} ({plugin}): included")
                } else {
                    let r = reason.as_deref().unwrap_or("predicates not satisfied");
                    format!("      skill {skill} ({plugin}): skipped ({r})")
                }
            }
            Self::SkillInstalled { skill, agent, dest } => {
                format!("✅ installed skill {skill} for {agent} → {dest}")
            }
            Self::SkillRemoved { path } => {
                format!("➖ removed {path}")
            }
            Self::SkillPropagated { skill, agent, dest } => {
                format!("✅ propagated skill {skill} for {agent} → {dest}")
            }
            Self::HookRegistered { agent, hook } => {
                format!("🟢 {hook}: hooks registered for {agent}")
            }
            Self::McpServerRegistered { agent, server } => {
                format!("✅ registered MCP server {server} for {agent}")
            }
            Self::Info { message } => {
                format!("ℹ️  {message}")
            }
            Self::Warning { message } => {
                format!("⚠️  {message}")
            }
            Self::InstalledSourceChanged {
                registry,
                source,
                status,
            } => match status.as_str() {
                "installed" => format!("➕ {registry} source installed: {source}"),
                "updated" => format!("✅ {registry} source updated: {source}"),
                "already_installed" => {
                    format!("🟢 {registry} source already installed: {source}")
                }
                "uninstalled" => format!("➖ {registry} source uninstalled: {source}"),
                "not_installed" => format!("🟢 {registry} source was not installed: {source}"),
                other => format!("ℹ️  {registry} source {other}: {source}"),
            },

            Self::HookConsidered {
                plugin,
                hook,
                event,
                selected,
                format,
                reason,
            } => {
                let fmt = format.as_deref().unwrap_or("symposium");
                if *selected {
                    format!("  hook {hook} ({plugin}): selected for {event} [format={fmt}]")
                } else {
                    let r = reason.as_deref().unwrap_or("not matched");
                    format!("  hook {hook} ({plugin}): skipped for {event} ({r})")
                }
            }
            Self::HookDispatched {
                plugin,
                hook,
                exit_code,
                error,
            } => {
                if let Some(err) = error {
                    format!("  hook {hook} ({plugin}): error — {err}")
                } else {
                    let code = exit_code.unwrap_or(0);
                    format!("  hook {hook} ({plugin}): exited {code}")
                }
            }

            Self::Validated {
                path,
                item_kind,
                valid,
                error,
                warning,
            } => {
                if *valid {
                    if let Some(w) = warning {
                        format!("  ⚠️  {path} ({item_kind}): {w}")
                    } else {
                        format!("  ✅ {path} ({item_kind})")
                    }
                } else {
                    let e = error.as_deref().unwrap_or("unknown error");
                    format!("  ✗ {path} ({item_kind}): {e}")
                }
            }
            Self::ProviderListed {
                name,
                source_type,
                url,
                path,
                plugins,
            } => {
                let location = url.as_deref().or(path.as_deref()).unwrap_or("(local)");
                let mut lines = format!("Provider: {name}\n  Type: {source_type}\n  {location}");
                if plugins.is_empty() {
                    lines.push_str("\n  (no plugins)");
                } else {
                    for p in plugins {
                        lines.push_str(&format!("\n  - {p}"));
                    }
                }
                lines
            }
        }
    }
}

/// Handle returned when creating a report layer, allowing the caller
/// to drain accumulated JSON after the operation completes.
#[derive(Clone)]
pub struct ReportHandle {
    buffer: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl ReportHandle {
    /// Drain accumulated JSON events. Only meaningful in `Json` mode.
    pub fn drain(&self) -> Vec<serde_json::Value> {
        std::mem::take(&mut *self.buffer.lock().unwrap())
    }
}

/// Tracing layer that captures events with a `report` field.
pub struct ReportLayer {
    mode: ReportMode,
    buffer: Arc<Mutex<Vec<serde_json::Value>>>,
    max_level: tracing::Level,
}

impl ReportLayer {
    pub fn new(mode: ReportMode, max_level: tracing::Level) -> (Self, ReportHandle) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let handle = ReportHandle {
            buffer: buffer.clone(),
        };
        (
            Self {
                mode,
                buffer,
                max_level,
            },
            handle,
        )
    }
}

struct ReportVisitor {
    report_json: Option<String>,
}

impl Visit for ReportVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "report" {
            self.report_json = Some(value.to_string());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "report" {
            self.report_json = Some(format!("{value:?}"));
        }
    }
}

impl<S> Layer<S> for ReportLayer
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().level() > &self.max_level {
            return;
        }

        let mut visitor = ReportVisitor { report_json: None };
        event.record(&mut visitor);

        let Some(json_str) = visitor.report_json else {
            return;
        };

        match self.mode {
            ReportMode::Normal => {
                if let Ok(evt) = serde_json::from_str::<ReportEvent>(&json_str) {
                    println!("{}", evt.format_human());
                }
            }
            ReportMode::Verbose => {
                if let Ok(evt) = serde_json::from_str::<ReportEvent>(&json_str) {
                    eprintln!("{}", evt.format_human());
                }
            }
            ReportMode::Json => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    self.buffer.lock().unwrap().push(val);
                }
            }
        }
    }
}
