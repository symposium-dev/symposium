//! Init command: `cargo agents init`.

use anyhow::{Context, Result, bail};
use dialoguer::MultiSelect;

use crate::agents::Agent;
use crate::config::{AgentEntry, Symposium};
use crate::output::{Output, display_path};

/// Options that can be provided on the command line to skip interactive prompts.
#[derive(Debug, Default)]
pub struct InitOpts {
    /// Agent names provided via `--add-agent`. If non-empty, skips the agent prompt.
    pub agents: Vec<String>,
    /// Agent names to remove via `--remove-agent`.
    pub remove_agents: Vec<String>,
    /// Explicit hook scope override.
    pub hook_scope: Option<crate::config::HookScope>,
}

/// Whether we can prompt the user interactively.
fn interactive(out: &Output) -> bool {
    out.is_interactive()
}

/// Resolve which agents to configure. Priority:
/// 1. Explicit `--add-agent` / `--remove-agent` flags (applied to existing set)
/// 2. With agents configured: interactive multi-select pre-selecting the
///    detected agents and reporting which appeared or disappeared since the last
///    setup (if `should_prompt`), else keep them. Deselecting all uninstalls.
/// 3. With none configured: interactive multi-select pre-selecting the
///    detected agents and insisting on at least one (if `should_prompt`), else
///    every detected agent, and an error when none is detected
fn resolve_agents(
    opts: &InitOpts,
    existing: &[AgentEntry],
    detected: &[Agent],
    should_prompt: bool,
    out: &Output,
) -> Result<Vec<Agent>> {
    if !opts.agents.is_empty() || !opts.remove_agents.is_empty() {
        let mut names: Vec<String> = existing.iter().map(|e| e.name.clone()).collect();
        for name in &opts.agents {
            Agent::from_config_name(name)?;
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        for name in &opts.remove_agents {
            Agent::from_config_name(name)?;
            names.retain(|n| n != name);
        }
        return names.iter().map(|n| Agent::from_config_name(n)).collect();
    }
    if !existing.is_empty() {
        if should_prompt {
            report_changes_since_last_setup(existing, detected, out);
            return prompt_for_agents(detected);
        }
        return existing
            .iter()
            .map(|e| Agent::from_config_name(&e.name))
            .collect();
    }
    if detected.is_empty() {
        if !should_prompt {
            let names: Vec<_> = Agent::all().iter().map(|a| a.config_name()).collect();
            bail!(
                "no agents detected; pass `--add-agent <name>` ({})",
                names.join(", ")
            );
        }
        out.info("No agents detected");
    } else {
        let names: Vec<_> = detected.iter().map(|a| a.display_name()).collect();
        out.info(format!("Detected: {}", names.join(", ")));
    }
    if should_prompt {
        return prompt_until_chosen(detected, out);
    }
    Ok(detected.to_vec())
}

fn report_changes_since_last_setup(existing: &[AgentEntry], detected: &[Agent], out: &Output) {
    let configured = |a: &Agent| existing.iter().any(|e| e.name == a.config_name());
    let added: Vec<_> = detected
        .iter()
        .filter(|a| !configured(a))
        .map(|a| a.display_name())
        .collect();
    let gone: Vec<_> = Agent::all()
        .iter()
        .filter(|a| configured(a) && !detected.contains(a))
        .map(|a| a.display_name())
        .collect();
    if !added.is_empty() {
        out.info(format!("Detected since last setup: {}", added.join(", ")));
    }
    if !gone.is_empty() {
        out.info(format!("No longer detected: {}", gone.join(", ")));
    }
}

fn prompt_until_chosen(preselected: &[Agent], out: &Output) -> Result<Vec<Agent>> {
    loop {
        let agents = prompt_for_agents(preselected)?;
        if !agents.is_empty() {
            return Ok(agents);
        }
        out.warn("Select at least one agent (space to select, enter to confirm, Ctrl-C to cancel)");
    }
}

/// Run user-wide initialization.
///
/// Prompts for agents (unless provided), writes
/// `~/.symposium/config.toml`, and registers global hooks.
pub async fn init(sym: &mut Symposium, out: &Output, opts: &InitOpts) -> Result<()> {
    tracing::info!("init started");
    out.println("Setting up symposium for your user account.\n");

    // CLI flags signal non-interactive intent — skip all prompts.
    let cli_driven = !opts.agents.is_empty() || !opts.remove_agents.is_empty();
    let should_prompt = !cli_driven && interactive(out);

    // Resolve each setting: CLI flag > interactive prompt > keep existing.
    let detected = Agent::detect_installed(sym.home_dir(), |name| std::env::var_os(name));
    let agents = resolve_agents(opts, &sym.config.agents, &detected, should_prompt, out)?;

    sym.config.agents = agents
        .iter()
        .map(|a| AgentEntry {
            name: a.config_name().to_string(),
        })
        .collect();

    sym.config.hook_scope = match opts.hook_scope {
        Some(scope) => scope,
        None if should_prompt && !agents.is_empty() => {
            prompt_for_hook_scope(sym.config.hook_scope)?
        }
        None => sym.config.hook_scope,
    };

    if should_prompt && !agents.is_empty() {
        sym.config.auto_update = prompt_for_auto_update(sym.config.auto_update)?;
        sym.config.telemetry.enabled = prompt_for_telemetry(sym.config.telemetry.enabled)?;
    }

    tracing::debug!(
        agents = ?agents.iter().map(|a| a.config_name()).collect::<Vec<_>>(),
        scope = ?sym.config.hook_scope,
        auto_update = ?sym.config.auto_update,
        "resolved config"
    );

    // Persist and apply.
    sym.save_config().context("failed to write user config")?;

    let config_path = sym.config_dir().join("config.toml");

    if agents.is_empty() {
        // Uninstall: unregister all hooks and MCP servers for every agent.
        crate::sync::register_hooks(sym, out)
            .await
            .context("failed to unregister hooks")?;
        out.done(format!(
            "{}: wrote user config (no agents — symposium uninstalled)",
            display_path(&config_path),
        ));
        return Ok(());
    }

    let agent_names: Vec<_> = agents.iter().map(|a| a.display_name()).collect();
    out.done(format!(
        "{}: wrote user config (agents: {})",
        display_path(&config_path),
        agent_names.join(", ")
    ));

    if sym.config.hook_scope == crate::config::HookScope::Global {
        crate::sync::register_hooks(sym, out)
            .await
            .context("failed to register global hooks")?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Interactive prompts
// ---------------------------------------------------------------------------

fn prompt_for_hook_scope(current: crate::config::HookScope) -> Result<crate::config::HookScope> {
    use crate::config::HookScope;

    let items = ["Globally (recommended)", "Per project"];
    let default = match current {
        HookScope::Global => 0,
        HookScope::Project => 1,
    };

    let selection = dialoguer::Select::new()
        .with_prompt("Install hooks and agent configuration")
        .items(items)
        .default(default)
        .interact()?;

    Ok(match selection {
        0 => HookScope::Global,
        _ => HookScope::Project,
    })
}

fn prompt_for_auto_update(current: crate::config::AutoUpdate) -> Result<crate::config::AutoUpdate> {
    use crate::config::AutoUpdate;

    let items = [
        "Auto-update (recommended)",
        "Warn when updates are available",
        "Off",
    ];
    let default = match current {
        AutoUpdate::On => 0,
        AutoUpdate::Warn => 1,
        AutoUpdate::Off => 2,
    };

    let selection = dialoguer::Select::new()
        .with_prompt("Automatic updates")
        .items(items)
        .default(default)
        .interact()?;

    Ok(match selection {
        0 => AutoUpdate::On,
        1 => AutoUpdate::Warn,
        _ => AutoUpdate::Off,
    })
}

fn prompt_for_telemetry(current: bool) -> Result<bool> {
    Ok(dialoguer::Confirm::new()
        .with_prompt(
            "Enable anonymous usage telemetry? It is stored locally under \
             ~/.symposium/telemetry/ and never uploaded automatically — you can review \
             it with `cargo agents telemetry show` and share it yourself",
        )
        .default(current)
        .interact()?)
}

fn prompt_for_agents(preselected: &[Agent]) -> Result<Vec<Agent>> {
    let agents = Agent::all();
    let items: Vec<&str> = agents.iter().map(|a| a.display_name()).collect();

    let defaults: Vec<bool> = agents.iter().map(|a| preselected.contains(a)).collect();

    let selections = MultiSelect::new()
        .with_prompt("Which agents do you use? (space to select, enter to confirm)")
        .items(items)
        .defaults(&defaults)
        .interact()?;

    Ok(selections.into_iter().map(|i| agents[i]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(names: &[&str]) -> Vec<AgentEntry> {
        names
            .iter()
            .map(|name| AgentEntry {
                name: name.to_string(),
            })
            .collect()
    }

    fn resolve(opts: &InitOpts, existing: &[&str], detected: &[Agent]) -> Vec<Agent> {
        resolve_agents(opts, &entries(existing), detected, false, &Output::quiet()).unwrap()
    }

    #[test]
    fn fresh_config_without_a_terminal_takes_detected_agents() {
        let detected = [Agent::Codex, Agent::Kiro];
        assert_eq!(resolve(&InitOpts::default(), &[], &detected), detected);
    }

    #[test]
    fn fresh_config_without_detection_or_terminal_is_an_error() {
        let result = resolve_agents(&InitOpts::default(), &[], &[], false, &Output::quiet());
        assert!(result.is_err());
    }

    #[test]
    fn rerun_reports_agents_detected_and_no_longer_detected() {
        let out = Output::capturing();
        report_changes_since_last_setup(
            &entries(&["claude", "codex"]),
            &[Agent::Claude, Agent::Copilot],
            &out,
        );
        let lines = out.captured().join("\n");
        assert!(
            lines.contains("Detected since last setup: GitHub Copilot"),
            "{lines}"
        );
        assert!(lines.contains("No longer detected: Codex CLI"), "{lines}");
    }

    #[test]
    fn existing_config_without_a_terminal_ignores_detection() {
        assert_eq!(
            resolve(&InitOpts::default(), &["codex"], &[Agent::Claude]),
            vec![Agent::Codex]
        );
    }

    #[test]
    fn flags_ignore_detection() {
        let opts = InitOpts {
            agents: vec!["goose".to_string()],
            ..InitOpts::default()
        };
        assert_eq!(resolve(&opts, &[], &[Agent::Claude]), vec![Agent::Goose]);
    }
}
