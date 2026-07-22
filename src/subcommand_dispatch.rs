//! Dispatch for plugin-vended `cargo agents <name>` subcommands.
//!
//! The top-level CLI declares `allow_external_subcommands = true`, any
//! unknown subcommand is routed here. This module looks up the name across configured plugins
//! (filtered by workspace crates at the plugin and subcommand levels), resolves its `Installation` to a `Runnable`, and
//! spawns it with inherited stdio, propagating the child's exit code.
//!

use std::{ffi::OsString, path::Path, process::ExitStatus};

use crate::{
    config::Symposium,
    installation::{acquire_installation, resolve_runnable},
    plugins::{self, Plugin, PluginRegistry, Subcommand},
    pm::PackageId,
};
use anyhow::{Context, Result, bail};
use symposium_install::{Runnable, UpdateLevel};
use tokio::process::Command;

/// Collect every plugin subcommand whose plugin-level and subcommand-level predicates
/// apply to `deps`. `used` names the plugins the applicable `[plugins] use`
/// entries enable, which is what wakes a dormant plugin. Shared between
/// dispatch (name lookup) and help rendering (audience grouping).
pub fn applicable_subcommands<'a>(
    registry: &'a PluginRegistry,
    deps: &[PackageId],
    used: &[&str],
) -> Vec<(&'a Plugin, &'a str, &'a Subcommand)> {
    let mut ctx = crate::predicate::PredicateContext::new(deps).with_used_names(used);
    let mut results = Vec::new();
    for parsed in &registry.plugins {
        let plugin = &parsed.plugin;
        if !parsed.applies(&mut ctx) {
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
    deps: &[PackageId],
    used: &[&str],
) -> Result<Option<(&'a Plugin, &'a Subcommand)>> {
    let matches: Vec<_> = applicable_subcommands(registry, deps, used)
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

    let mut deps = sym.workspace_deps(cwd);
    let workspace = deps.load().cloned();
    let registry = plugins::load_registry_with_workspace(sym, workspace.as_deref()).await;

    let dep_ids = crate::pm::workspace_dep_ids(sym, deps.crates()).await;
    let used = workspace
        .as_ref()
        .map(|ws| sym.config.plugins.used_names_in(&ws.root))
        .unwrap_or_default();
    let (plugin, subcommand) = find_subcommand(&registry, name, &dep_ids, &used)?
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
        acquire_installation(sym, installation, None, None, UpdateLevel::None).await?,
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
    use crate::plugins::ParsedPlugin;
    use crate::pm::ANY_VERSION;
    use crate::{plugins::Audience, predicate::PredicateSet};
    use std::{collections::BTreeMap, path::PathBuf};

    fn ws_crate(name: &str, version: &str) -> PackageId {
        PackageId::new(crate::pm::CARGO_PM, name, version)
    }

    fn crate_set(spec: &str) -> PredicateSet {
        PredicateSet::from_depends_on(spec).unwrap()
    }

    fn plugin_with(
        name: &str,
        depends_on: &str,
        subcommands: BTreeMap<String, Subcommand>,
    ) -> ParsedPlugin {
        ParsedPlugin {
            canonical: PackageId::new("test", name, ANY_VERSION),
            path: PathBuf::from(format!("/test/{name}.toml")),
            plugin: Plugin {
                name: name.into(),
                predicates: crate_set(depends_on),
                installations: vec![],
                hooks: vec![],
                skills: vec![],
                mcp_servers: vec![],
                subcommands,
                custom_predicates: vec![],
                chained: vec![],
                requires_use: false,
            },
            source_dir: PathBuf::from("/test"),
            workspace_member: false,
        }
    }

    fn subcommand(command: &str, depends_on: Option<&str>) -> Subcommand {
        Subcommand {
            description: "test".into(),
            audience: Audience::default(),
            command: command.into(),
            predicates: depends_on.map(crate_set).unwrap_or_default(),
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

        let (plugin, sub) = find_subcommand(&reg, "greet", &ws, &[]).unwrap().unwrap();
        assert_eq!(plugin.name, "example-plugin");
        assert_eq!(sub.command, "greet-install");
    }

    #[test]
    fn unknown_name_returns_none() {
        let mut subs = BTreeMap::new();
        subs.insert("greet".into(), subcommand("greet-install", None));
        let reg = registry(vec![plugin_with("example-plugin", "*", subs)]);
        let ws = [ws_crate("skill-tree", "1.0.0")];

        assert!(find_subcommand(&reg, "nope", &ws, &[]).unwrap().is_none());
    }

    #[test]
    fn plugin_predicate_excludes_plugin() {
        let mut subs = BTreeMap::new();
        subs.insert("greet".into(), subcommand("greet-install", Some("rayon")));
        let reg = registry(vec![plugin_with("example-plugin", "*", subs)]);
        let ws = [ws_crate("skill-tree", "1.0.0")];

        assert!(find_subcommand(&reg, "greet", &ws, &[]).unwrap().is_none());
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

        let err = find_subcommand(&reg, "greet", &ws, &[])
            .unwrap_err()
            .to_string();

        assert!(err.contains("plugin-a"), "expected `plugin-a` in {err}");
        assert!(err.contains("plugin-b"), "expected `plugin-b` in {err}");
    }
}
