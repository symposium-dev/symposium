//! Dispatch for plugin-vended `cargo agents <name>` subcommands.
//!
//! The top-level CLI declares `allow_external_subcommands = true`, any
//! unknown subcommand is routed here. This module looks up the name across configured plugins
//! (filtered by workspace crates at the plugin and subcommand levels), resolves its `Installation` to a `Runnable`, and
//! spawns it with inherited stdio, propagating the child's exit code.
//!

use std::{ffi::OsString, path::Path, process::ExitStatus};

use symposium_sdk::workspace::WorkspaceCrate;

use crate::{
    config::Symposium,
    installation::{acquire_installation, resolve_runnable},
    plugins::{self, ParsedPlugin, Plugin, PluginRegistry, Subcommand},
};
use anyhow::{Context, Result, bail};
use semver::Version;
use symposium_install::Runnable;
use tokio::process::Command;

/// Collect every plugin subcommand whose plugin-level and subcommand-level predicates
/// apply to `deps`. Shared between dispatch (name lookup) and help rendering (audience grouping).
pub fn applicable_subcommands<'a>(
    registry: &'a PluginRegistry,
    deps: &[(String, Version)],
) -> Vec<(&'a Plugin, &'a str, &'a Subcommand)> {
    let mut ctx = crate::predicate::PredicateContext::new(deps);
    let mut results = Vec::new();
    for ParsedPlugin { plugin, .. } in &registry.plugins {
        if !plugin.applies(&mut ctx) {
            continue;
        }
        for (name, subcommand) in &plugin.subcommands {
            if subcommand.predicates.evaluate(&mut ctx) {
                results.push((plugin, name.as_str(), subcommand));
            }
        }
    }
    results
}

/// Look up a subcommand by name across all plugins, filtered by workspace crates at a plugin
///  and subcommand levels.
///
/// - `Ok(None)` - no plugin claims the name, or every claim was filtered out.
/// - `Ok(Some(..))` - exactly one plugin claims the name and applies.
/// - `Err(..)` - two or more plugins claim the name and all apply.
pub fn find_subcommand<'a>(
    registry: &'a PluginRegistry,
    name: &str,
    workspace: &[WorkspaceCrate],
) -> Result<Option<(&'a Plugin, &'a Subcommand)>> {
    let deps = crate::crate_sources::crate_pairs(workspace);

    let matches: Vec<_> = applicable_subcommands(registry, &deps)
        .into_iter()
        .filter(|(_, n, _)| *n == name)
        .map(|(plugin, _, subcmd)| (plugin, subcmd))
        .collect();

    match matches.as_slice() {
        [] => Ok(None),
        [single] => Ok(Some(*single)),
        many => {
            let plugins = many
                .iter()
                .map(|(p, _)| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");

            bail!("subcommand `{name}` is defined by multiple plugins: {plugins}");
        }
    }
}

/// Result of running an external subcommand.
pub struct ExternalOutput {
    pub exit_code: u8,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub async fn dispatch_external(
    sym: &Symposium,
    cwd: &Path,
    argv: Vec<OsString>,
) -> Result<ExternalOutput> {
    let mut argv = argv.into_iter();
    let raw_name = argv.next().context("missing subcommand name")?;

    let name = raw_name
        .to_str()
        .context("subcommand name must be valid UTF-8")?;
    let forwarded = argv.collect::<Vec<_>>();

    let registry = plugins::load_registry(sym);
    let mut deps = sym.workspace_deps(cwd);
    let workspace = deps.crates();

    let (plugin, subcommand) = find_subcommand(&registry, name, workspace)?
        .with_context(|| format!("no plugin defines subcommand `{name}`"))?;

    let installation = plugin
        .get_installation(&subcommand.command)
        .with_context(|| {
            format!(
                "plugin `{}` references unknown installation `{}`",
                plugin.name, subcommand.command
            )
        })?;

    let runnable = resolve_runnable(
        acquire_installation(sym, installation, None, None).await?,
        &format!("subcommand `{name}`"),
    )?;

    spawn(runnable, &installation.args, &forwarded).await
}

async fn spawn(
    runnable: Runnable,
    install_args: &[String],
    forwarded: &[OsString],
) -> Result<ExternalOutput> {
    let mut cmd = match runnable {
        Runnable::Exec(path_buf) => Command::new(path_buf),
        Runnable::Script(path_buf) => {
            let mut cmd = Command::new("sh");
            cmd.arg(path_buf);
            cmd
        }
    };

    cmd.args(install_args).args(forwarded);

    let output = cmd
        .output()
        .await
        .context("failed to spawn subcommand process")?;

    Ok(ExternalOutput {
        exit_code: exit_byte_from(output.status),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

fn exit_byte_from(status: ExitStatus) -> u8 {
    match status.code() {
        Some(0) => 0,
        Some(n) => u8::try_from(n).unwrap_or(1),
        None => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{plugins::Audience, predicate::PredicateSet};
    use std::{collections::BTreeMap, path::PathBuf};

    fn ws_crate(name: &str, version: &str) -> WorkspaceCrate {
        WorkspaceCrate {
            name: name.into(),
            version: semver::Version::parse(version).unwrap(),
            path: None,
        }
    }

    fn crate_set(spec: &str) -> PredicateSet {
        PredicateSet::from_crates(spec).unwrap()
    }

    fn plugin_with(
        name: &str,
        crates: &str,
        subcommands: BTreeMap<String, Subcommand>,
    ) -> ParsedPlugin {
        ParsedPlugin {
            path: PathBuf::from(format!("/test/{name}.toml")),
            plugin: Plugin {
                name: name.into(),
                predicates: crate_set(crates),
                installations: vec![],
                hooks: vec![],
                skills: vec![],
                mcp_servers: vec![],
                subcommands,
                custom_predicates: vec![],
            },
            source_name: "test".into(),
            source_dir: PathBuf::from("/test"),
        }
    }

    fn subcommand(command: &str, crates: Option<&str>) -> Subcommand {
        Subcommand {
            description: "test".into(),
            audience: Audience::default(),
            command: command.into(),
            predicates: crates.map(crate_set).unwrap_or_default(),
        }
    }

    fn registry(plugins: Vec<ParsedPlugin>) -> PluginRegistry {
        PluginRegistry {
            plugins,
            standalone_skills: vec![],
            warnings: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        }
    }

    #[test]
    fn returns_single_match() {
        let mut subs = BTreeMap::new();
        subs.insert("greet".into(), subcommand("greet-install", None));
        let reg = registry(vec![plugin_with("example-plugin", "*", subs)]);

        let ws = [ws_crate("skill-tree", "1.0.0")];

        let (plugin, sub) = find_subcommand(&reg, "greet", &ws).unwrap().unwrap();
        assert_eq!(plugin.name, "example-plugin");
        assert_eq!(sub.command, "greet-install");
    }

    #[test]
    fn unknown_name_returns_none() {
        let mut subs = BTreeMap::new();
        subs.insert("greet".into(), subcommand("greet-install", None));
        let reg = registry(vec![plugin_with("example-plugin", "*", subs)]);
        let ws = [ws_crate("skill-tree", "1.0.0")];

        assert!(find_subcommand(&reg, "nope", &ws).unwrap().is_none());
    }

    #[test]
    fn plugin_predicate_excludes_plugin() {
        let mut subs = BTreeMap::new();
        subs.insert("greet".into(), subcommand("greet-install", Some("rayon")));
        let reg = registry(vec![plugin_with("example-plugin", "*", subs)]);
        let ws = [ws_crate("skill-tree", "1.0.0")];

        assert!(find_subcommand(&reg, "greet", &ws).unwrap().is_none());
    }

    #[test]
    fn two_matching_plugins_conflict() {
        let mut a = BTreeMap::new();
        a.insert("greet".into(), subcommand("a-install", None));

        let mut b = BTreeMap::new();
        b.insert("greet".into(), subcommand("b-install", None));

        let reg = registry(vec![
            plugin_with("plugin-a", "*", a),
            plugin_with("plugin-b", "*", b),
        ]);
        let ws = [ws_crate("skill-tree", "1.0.0")];

        let err = find_subcommand(&reg, "greet", &ws).unwrap_err().to_string();

        assert!(err.contains("plugin-a"), "expected `plugin-a` in {err}");
        assert!(err.contains("plugin-b"), "expected `plugin-b` in {err}");
    }
}
