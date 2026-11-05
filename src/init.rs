//! Init command: `cargo agents init`.

use std::path::Path;

use anyhow::{Context, Result, bail};
use dialoguer::MultiSelect;
use symposium_install::UpdateLevel;

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
/// 2. With agents configured: interactive multi-select (if `should_prompt`),
///    else keep them. Deselecting all uninstalls.
/// 3. With none configured: interactive multi-select insisting on at least one
///    (if `should_prompt`), else every detected agent, and an error when none
///    is detected
///
/// Explicit choices stay selected even when detection fails. Other choices
/// follow detection history, preserving previous deselections.
fn resolve_agents(
    opts: &InitOpts,
    existing: &[AgentEntry],
    detected: &[Agent],
    previously_detected: &[Agent],
    explicit: &[String],
    should_prompt: bool,
    out: &Output,
) -> Result<Vec<String>> {
    if !opts.agents.is_empty() || !opts.remove_agents.is_empty() {
        let mut names: Vec<String> = existing.iter().map(|e| e.name.clone()).collect();
        for name in &opts.agents {
            Agent::from_config_name(name)?;
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        for name in &opts.remove_agents {
            if !names.contains(name) {
                Agent::from_config_name(name)?;
            }
            names.retain(|n| n != name);
        }
        return Ok(names);
    }
    if !existing.is_empty() {
        if should_prompt {
            report_changes_since_last_setup(existing, detected, previously_detected, explicit, out);
            return Ok(prompt_for_agents(&preselected(
                existing,
                detected,
                previously_detected,
                explicit,
            ))?
            .iter()
            .map(|a| a.config_name().to_string())
            .collect());
        }
        return Ok(existing.iter().map(|e| e.name.clone()).collect());
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
        return Ok(prompt_until_chosen(
            &preselected(existing, detected, previously_detected, explicit),
            out,
        )?
        .iter()
        .map(|a| a.config_name().to_string())
        .collect());
    }
    Ok(detected
        .iter()
        .map(|a| a.config_name().to_string())
        .collect())
}

fn preselected(
    existing: &[AgentEntry],
    detected: &[Agent],
    previously_detected: &[Agent],
    explicit: &[String],
) -> Vec<Agent> {
    Agent::all()
        .iter()
        .copied()
        .filter(|a| {
            let configured = existing.iter().any(|e| e.name == a.config_name());
            (configured && explicit.iter().any(|name| name == a.config_name()))
                || (detected.contains(a) && (configured || !previously_detected.contains(a)))
        })
        .collect()
}

fn report_changes_since_last_setup(
    existing: &[AgentEntry],
    detected: &[Agent],
    previously_detected: &[Agent],
    explicit: &[String],
    out: &Output,
) {
    let configured = |a: &Agent| existing.iter().any(|e| e.name == a.config_name());
    let added: Vec<_> = detected
        .iter()
        .filter(|a| !configured(a) && !previously_detected.contains(a))
        .map(|a| a.display_name())
        .collect();
    let gone: Vec<_> = Agent::all()
        .iter()
        .filter(|a| {
            configured(a)
                && previously_detected.contains(a)
                && !detected.contains(a)
                && !explicit.iter().any(|name| name == a.config_name())
        })
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

/// Keep explicit choices while selected; flags and newly checked boxes express intent.
fn explicit_after_selection(
    previous: &[String],
    selected: &[String],
    defaults: &[Agent],
    opts: &InitOpts,
    prompted: bool,
) -> Vec<String> {
    selected
        .iter()
        .filter(|name| {
            previous.contains(name)
                || opts.agents.contains(name)
                || (prompted && !defaults.iter().any(|a| a.config_name() == name.as_str()))
        })
        .cloned()
        .collect()
}

/// Run user-wide initialization.
///
/// Prompts for agents (unless provided), writes
/// `~/.symposium/config.toml`, and registers global hooks.
pub async fn init(
    sym: &mut Symposium,
    cwd: &Path,
    out: &Output,
    opts: &InitOpts,
    update: UpdateLevel,
) -> Result<()> {
    tracing::info!("init started");
    out.println("Setting up symposium for your user account.\n");

    // CLI flags signal non-interactive intent — skip all prompts.
    let cli_driven = !opts.agents.is_empty() || !opts.remove_agents.is_empty();
    let should_prompt = !cli_driven && interactive(out);

    // Resolve each setting: CLI flag > interactive prompt > keep existing.
    let first_run = sym.config.agents.is_empty();
    let detected = Agent::detect_installed(sym.home_dir(), |name| std::env::var_os(name));
    let state = crate::state::load(sym.config_dir()).unwrap_or_default();
    let explicit = state.explicit_agents.clone().unwrap_or_else(|| {
        sym.config
            .agents
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    });
    let previously_detected: Vec<Agent> = state
        .detected_agents
        .iter()
        .filter_map(|name| Agent::from_config_name(name).ok())
        .collect();
    let defaults = preselected(
        &sym.config.agents,
        &detected,
        &previously_detected,
        &explicit,
    );
    let names = resolve_agents(
        opts,
        &sym.config.agents,
        &detected,
        &previously_detected,
        &explicit,
        should_prompt,
        out,
    )?;
    let removed_agent = sym.config.agents.iter().any(|e| !names.contains(&e.name));

    let agents: Vec<Agent> = names
        .iter()
        .filter_map(|n| Agent::from_config_name(n).ok())
        .collect();
    let explicit = explicit_after_selection(&explicit, &names, &defaults, opts, should_prompt);
    sym.config.agents = names
        .iter()
        .map(|name| AgentEntry { name: name.clone() })
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
        agents = ?names,
        scope = ?sym.config.hook_scope,
        auto_update = ?sym.config.auto_update,
        "resolved config"
    );

    // Persist and apply.
    sym.save_config().context("failed to write user config")?;
    let mut detection_history = state.detected_agents;
    if should_prompt || (!cli_driven && first_run) {
        detection_history = detected
            .iter()
            .map(|a| a.config_name().to_string())
            .collect();
    } else {
        // An explicit removal must not be re-added as a newly detected agent.
        for name in &opts.remove_agents {
            if detected.iter().any(|a| a.config_name() == name) && !detection_history.contains(name)
            {
                detection_history.push(name.clone());
            }
        }
    }
    crate::state::record_agent_choices(sym.config_dir(), explicit, detection_history);

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
        return reap_removed_agent_skills(sym, cwd, out, update).await;
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

    if removed_agent {
        reap_removed_agent_skills(sym, cwd, out, update).await?;
    }
    Ok(())
}

async fn reap_removed_agent_skills(
    sym: &Symposium,
    cwd: &Path,
    out: &Output,
    update: UpdateLevel,
) -> Result<()> {
    let ws = sym.workspace(cwd);
    if ws.info().await.is_some() {
        crate::sync::sync(sym, &ws, update).await?;
    }
    out.info("skills installed in other workspaces stay until `cargo agents sync` runs there");
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
        resolve_agents(
            opts,
            &entries(existing),
            detected,
            &[],
            &[],
            false,
            &Output::quiet(),
        )
        .unwrap()
        .iter()
        .map(|n| Agent::from_config_name(n).unwrap())
        .collect()
    }

    #[test]
    fn explicit_choices_survive_disappearance_but_detected_choices_do_not() {
        let configured = entries(&["claude", "codex"]);
        assert_eq!(
            preselected(
                &configured,
                &[],
                &[Agent::Claude, Agent::Codex],
                &["claude".into()]
            ),
            vec![Agent::Claude]
        );
        assert_eq!(
            preselected(&configured, &[], &[], &["claude".into(), "codex".into()]),
            vec![Agent::Claude, Agent::Codex]
        );
    }

    #[test]
    fn accepting_detection_does_not_pin_but_checking_a_box_does() {
        let selected = vec!["claude".into(), "codex".into()];
        assert_eq!(
            explicit_after_selection(&[], &selected, &[Agent::Codex], &InitOpts::default(), true),
            vec!["claude".to_string()]
        );
        assert!(
            explicit_after_selection(
                &[],
                &["codex".into()],
                &[Agent::Codex],
                &InitOpts::default(),
                true
            )
            .is_empty()
        );
    }

    #[test]
    fn adding_an_already_detected_agent_pins_it_and_removing_clears_it() {
        let opts = InitOpts {
            agents: vec!["codex".into()],
            ..InitOpts::default()
        };
        assert_eq!(
            explicit_after_selection(&[], &["codex".into()], &[Agent::Codex], &opts, false),
            vec!["codex".to_string()]
        );
        assert!(
            explicit_after_selection(
                &["codex".into()],
                &[],
                &[Agent::Codex],
                &InitOpts::default(),
                true
            )
            .is_empty()
        );
    }

    #[test]
    fn fresh_config_without_a_terminal_takes_detected_agents() {
        let detected = [Agent::Codex, Agent::Kiro];
        assert_eq!(resolve(&InitOpts::default(), &[], &detected), detected);
    }

    #[test]
    fn fresh_config_without_detection_or_terminal_is_an_error() {
        let result = resolve_agents(
            &InitOpts::default(),
            &[],
            &[],
            &[],
            &[],
            false,
            &Output::quiet(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn first_run_preselects_every_detected_agent() {
        let detected = [Agent::Claude, Agent::Codex];
        assert_eq!(preselected(&[], &detected, &[], &[]), detected);
    }

    #[test]
    fn rerun_preselects_configured_and_new_agents_but_not_deselected_ones() {
        assert_eq!(
            preselected(
                &entries(&["codex", "kiro"]),
                &[Agent::Claude, Agent::Codex, Agent::Copilot],
                &[Agent::Claude, Agent::Codex, Agent::Kiro],
                &[],
            ),
            vec![Agent::Codex, Agent::Copilot]
        );
    }

    #[test]
    fn rerun_reports_agents_detected_and_no_longer_detected() {
        let out = Output::capturing();
        report_changes_since_last_setup(
            &entries(&["claude", "codex"]),
            &[Agent::Claude, Agent::Copilot],
            &[Agent::Claude, Agent::Codex],
            &[],
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
    fn rerun_does_not_report_deselected_agents_as_new() {
        let out = Output::capturing();
        report_changes_since_last_setup(
            &entries(&["codex"]),
            &[Agent::Claude, Agent::Codex],
            &[Agent::Claude, Agent::Codex],
            &[],
            &out,
        );
        assert!(out.captured().is_empty(), "{:?}", out.captured());
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
