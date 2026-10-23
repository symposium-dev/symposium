use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use symposium_install::UpdateLevel;

use crate::agents::Agent;
use crate::config::{Symposium, UseEntry};
use crate::crate_sources::normalize_crate_name;
use crate::plugins::{Plugin, PluginSource};
use crate::pm::PluginKind;
use crate::report::ReportEvent;

pub async fn try_plugin(
    sym: &mut Symposium,
    cwd: &Path,
    path: &Path,
    update: UpdateLevel,
) -> Result<()> {
    if !sym.config.defaults.user_plugins {
        bail!("`[defaults] user-plugins` is off, so a tried plugin would never load");
    }

    let dir = path
        .canonicalize()
        .with_context(|| format!("cannot read `{}`", path.display()))?;
    let unvalidated = crate::plugins::entry_plugin(&dir, Path::new(""), "user-plugins")
        .with_context(|| format!("no Symposium.toml or SKILL.md in `{}`", dir.display()))??;
    let plugin = crate::plugins::validate_plugin(unvalidated, PluginKind::Registry)
        .with_context(|| format!("run `cargo agents plugin validate` on `{}`", dir.display()))?;
    let name = plugin.manifest.name.clone();

    if !crate::skills::is_safe_dir_name(&name) {
        bail!("plugin name `{name}` cannot be used as a directory name");
    }
    if sym.config.plugins.is_disabled(&name) {
        bail!("`{name}` is disabled in `[plugins] disable`; remove that entry to try it");
    }

    let plugins_dir = user_plugins_dir(sym);
    let link = plugins_dir.join(&name);
    match slot(&link) {
        Slot::Free => {
            fs::create_dir_all(&plugins_dir)
                .with_context(|| format!("cannot create `{}`", plugins_dir.display()))?;
            symlink_dir(&dir, &link)
                .with_context(|| format!("cannot link `{}`", link.display()))?;
        }
        Slot::Trial(target) if target == dir => {}
        Slot::Trial(target) => bail!(
            "`{name}` is already being tried from `{}`; run `cargo agents plugin try --remove {name}` first",
            target.display()
        ),
        Slot::Taken => bail!(
            "`{}` already exists and is not a trial; move it to try `{name}`",
            link.display()
        ),
    }

    let entry = UseEntry::Global(name.clone());
    if !sym.config.plugins.used.contains(&entry) {
        sym.config.plugins.used.push(entry);
        sym.save_config().context("failed to write user config")?;
    }

    tracing::info!(
        report = %ReportEvent::PluginTried { name: name.clone() },
    );
    if !plugin.manifest.requires_use {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!(
                    "`{name}` has its own activation conditions and applies only where they hold; see `cargo agents status`"
                ),
            },
        );
    }

    if sync_here(sym, cwd, update).await? {
        report_loaded(sym, cwd, &skill_names(&plugin)).await;
    }
    Ok(())
}

pub async fn remove_trial(
    sym: &mut Symposium,
    cwd: &Path,
    target: &Path,
    update: UpdateLevel,
) -> Result<()> {
    let plugins_dir = user_plugins_dir(sym);
    let name = trial_name(&plugins_dir, target)?;
    let link = plugins_dir.join(&name);

    remove_symlink_dir(&link).with_context(|| format!("cannot remove `{}`", link.display()))?;
    let _ = fs::remove_dir(&plugins_dir);

    let normalized = normalize_crate_name(&name);
    let used = &mut sym.config.plugins.used;
    let before = used.len();
    used.retain(
        |entry| !matches!(entry, UseEntry::Global(n) if normalize_crate_name(n) == normalized),
    );
    if used.len() != before {
        sym.save_config().context("failed to write user config")?;
    }

    tracing::info!(report = %ReportEvent::PluginTrialRemoved { name });

    sync_here(sym, cwd, update).await.map(|_| ())
}

async fn sync_here(sym: &Symposium, cwd: &Path, update: UpdateLevel) -> Result<bool> {
    let deps = sym.workspace_deps(cwd);
    if deps.load().is_some() {
        crate::sync::sync(sym, &deps, update).await?;
        return Ok(true);
    }
    tracing::info!(
        report = %ReportEvent::Info {
            message: "not in a Rust workspace; agents pick the change up when a session starts in one".to_string(),
        },
    );
    Ok(false)
}

const LISTING_TIMEOUT: Duration = Duration::from_secs(60);

fn skill_names(plugin: &Plugin) -> Vec<String> {
    let mut names: Vec<String> = plugin
        .manifest
        .skills
        .iter()
        .filter_map(|group| match &group.source {
            PluginSource::Path(dir) => Some(crate::skills::discover_skills(
                dir,
                group.workspace_member,
                &group.predicates,
            )),
            PluginSource::Git(_) => None,
        })
        .flatten()
        .filter_map(Result::ok)
        .map(|skill| skill.name().to_string())
        .collect();
    names.sort();
    names.dedup();
    names
}

fn listing(agent: Agent) -> Option<(&'static str, &'static [&'static str])> {
    match agent {
        Agent::Codex => Some(("codex", &["debug", "prompt-input", "hi"])),
        Agent::Copilot => Some(("copilot", &["skill", "list", "--json"])),
        Agent::Goose => Some(("goose", &["skills", "list"])),
        Agent::OpenCode => Some(("opencode", &["debug", "skill"])),
        Agent::Claude | Agent::Antigravity | Agent::Kiro => None,
    }
}

fn listed(agent: Agent, stdout: &str, skill: &str) -> bool {
    let json_names = |items: Vec<serde_json::Value>| {
        items
            .iter()
            .any(|item| item["name"].as_str() == Some(skill))
    };
    match agent {
        Agent::Copilot | Agent::OpenCode => serde_json::from_str(stdout)
            .map(json_names)
            .unwrap_or(false),
        Agent::Codex => {
            let entry = format!("- {skill}: ");
            serde_json::from_str::<Vec<serde_json::Value>>(stdout)
                .unwrap_or_default()
                .iter()
                .flat_map(|item| item["content"].as_array().cloned().unwrap_or_default())
                .filter_map(|content| content["text"].as_str().map(str::to_string))
                .any(|text| text.lines().any(|line| line.starts_with(&entry)))
        }
        Agent::Goose => stdout
            .lines()
            .any(|line| line.split('|').next().map(str::trim) == Some(skill)),
        Agent::Claude | Agent::Antigravity | Agent::Kiro => false,
    }
}

async fn run_listing(
    sym: &Symposium,
    cwd: &Path,
    program: &str,
    args: &[&str],
) -> std::io::Result<std::process::Output> {
    let mut cmd = sym.agent_command(program);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    tokio::time::timeout(LISTING_TIMEOUT, cmd.output())
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out"))?
}

async fn report_loaded(sym: &Symposium, cwd: &Path, skills: &[String]) {
    if skills.is_empty() {
        return;
    }
    for entry in &sym.config.agents {
        let Ok(agent) = Agent::from_config_name(&entry.name) else {
            continue;
        };
        let agent_name = agent.display_name();
        let Some((program, args)) = listing(agent) else {
            info(format!(
                "{agent_name} has no listing command; start a new session here and run `/skills`"
            ));
            continue;
        };
        let command = format!("{program} {}", args.join(" "));
        match run_listing(sym, cwd, program, args).await {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info(format!(
                    "{agent_name}: `{program}` is not on PATH, not checked"
                ));
            }
            Err(e) => warn(format!("{agent_name}: `{command}` failed: {e}")),
            Ok(output) if !output.status.success() => {
                warn(format!(
                    "{agent_name}: `{command}` exited with {}",
                    output.status
                ));
            }
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let missing: Vec<&str> = skills
                    .iter()
                    .filter(|skill| !listed(agent, &stdout, skill))
                    .map(String::as_str)
                    .collect();
                if missing.is_empty() {
                    info(format!("{agent_name} lists {}", skills.join(", ")));
                } else {
                    warn(format!(
                        "{agent_name} does not list {} (checked with `{command}`)",
                        missing.join(", ")
                    ));
                }
            }
        }
    }
}

fn info(message: String) {
    tracing::info!(report = %ReportEvent::Info { message });
}

fn warn(message: String) {
    tracing::info!(report = %ReportEvent::Warning { message });
}

fn user_plugins_dir(sym: &Symposium) -> PathBuf {
    sym.config_dir().join("plugins")
}

fn trial_name(plugins_dir: &Path, target: &Path) -> Result<String> {
    if let Ok(dir) = target.canonicalize()
        && dir.is_dir()
    {
        let entries = fs::read_dir(plugins_dir).into_iter().flatten().flatten();
        for entry in entries {
            if matches!(slot(&entry.path()), Slot::Trial(t) if t == dir) {
                return Ok(entry.file_name().to_string_lossy().into_owned());
            }
        }
        bail!("`{}` is not being tried", target.display());
    }

    let Some(name) = target.file_name().and_then(|n| n.to_str()) else {
        bail!("`{}` is not being tried", target.display());
    };
    match slot(&plugins_dir.join(name)) {
        Slot::Trial(_) => Ok(name.to_string()),
        Slot::Free => bail!("`{name}` is not being tried"),
        Slot::Taken => bail!("`{name}` is a user plugin, not a trial; it was left alone"),
    }
}

enum Slot {
    Free,
    Trial(PathBuf),
    Taken,
}

fn slot(link: &Path) -> Slot {
    match fs::symlink_metadata(link) {
        Err(_) => Slot::Free,
        Ok(meta) if meta.file_type().is_symlink() => {
            fs::read_link(link).map_or(Slot::Taken, Slot::Trial)
        }
        Ok(_) => Slot::Taken,
    }
}

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(unix)]
fn remove_symlink_dir(link: &Path) -> std::io::Result<()> {
    fs::remove_file(link)
}

#[cfg(windows)]
fn remove_symlink_dir(link: &Path) -> std::io::Result<()> {
    fs::remove_dir(link)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_reads_each_agent_listing() {
        let codex = r#"[{"content":[{"text":"<skills_instructions>\n### Available skills\n- kit-guidance: Guidance (file: r0/kit-guidance/SKILL.md)\n- other: x"}]}]"#;
        let copilot = r#"[{"name":"kit-guidance","source":"project","path":"/w/.agents/skills/kit-guidance"}]"#;
        let goose =
            "Name         | Description\nkit-guidance | Guidance...  \nweb-search   | Search";
        let opencode =
            r#"[{"name":"kit-guidance","location":"/w/.agents/skills/kit-guidance/SKILL.md"}]"#;
        let cases = [
            (Agent::Codex, codex, "kit-guidance", true),
            (Agent::Codex, codex, "kit", false),
            (Agent::Codex, "not json", "kit-guidance", false),
            (Agent::Copilot, copilot, "kit-guidance", true),
            (Agent::Copilot, copilot, "kit", false),
            (Agent::Goose, goose, "kit-guidance", true),
            (Agent::Goose, goose, "Name", true),
            (Agent::Goose, goose, "guidance", false),
            (Agent::OpenCode, opencode, "kit-guidance", true),
            (Agent::OpenCode, "[]", "kit-guidance", false),
            (Agent::Claude, copilot, "kit-guidance", false),
        ];
        for (agent, stdout, skill, expected) in cases {
            assert_eq!(listed(agent, stdout, skill), expected, "{agent:?} {skill}");
        }
    }
}
