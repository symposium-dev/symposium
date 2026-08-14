//! Deciding which backing servers a workspace makes available.
//!
//! The same predicate filtering that decides which skills install decides
//! which MCP servers are in scope, so a workspace only ever sees tools
//! belonging to crates it actually depends on. That conditionality is the
//! thing no MCP primitive can express, and it is why the meta-server exists.
//!
//! Nothing is started here. Resolution is a read of the plugin registry;
//! processes begin on first use.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::plugins::{McpTransport, StdioCommand};

use crate::config::Symposium;
use crate::mcp::client::{SpawnKind, SpawnSpec};
use crate::mcp::server::{EXECUTE, LIST_TOOLS};
use crate::plugins::McpServerOverrides;

/// Where a server's executable comes from.
///
/// An installation is carried as its definition rather than a path: acquiring
/// it means downloading or installing, and startup has to stay side-effect
/// free. The same shape the hook layer uses.
#[derive(Debug, Clone)]
pub enum ServerCommand {
    Path(PathBuf),
    Installation(Box<crate::plugins::Installation>),
}

/// How a resolved server is reached.
#[derive(Debug, Clone)]
pub enum ServerTransport {
    Stdio {
        command: ServerCommand,
        args: Vec<String>,
        env: Vec<(String, String)>,
        cwd: Option<PathBuf>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
    },
}

/// A backing server, ready to be started on demand.
#[derive(Debug, Clone)]
pub struct ResolvedServer {
    pub name: String,
    pub transport: ServerTransport,
    pub startup_timeout: Duration,
    /// Ceiling on one call to this server, already reconciled with the
    /// user's script deadline.
    pub tool_call_timeout: Duration,
    pub enabled_tools: Option<Vec<String>>,
    pub disabled_tools: Option<Vec<String>>,
    /// Acquired before the server is first started, so a package-runner
    /// download does not land on the first tool call.
    pub requirements: Vec<crate::plugins::Installation>,
}

impl ResolvedServer {
    /// Acquire what this server needs and produce its spawn spec.
    ///
    /// Deferred to first use rather than done at resolve time: a client may
    /// spawn a throwaway copy of the meta-server to probe it, and startup must
    /// not download anything.
    pub async fn spawn_spec(&self, sym: &Symposium) -> anyhow::Result<SpawnSpec> {
        // Dispatch-time acquisition serves the cache; the SessionStart prewarm
        // is what forces a freshness check once per session.
        let update = symposium_install::UpdateLevel::None;

        // Requirements first, so a warmed cache is in place before the command
        // runs. A failure here only means the cost lands later.
        for requirement in &self.requirements {
            if let Err(e) =
                crate::installation::acquire_installation(sym, requirement, None, None, update)
                    .await
            {
                tracing::warn!(
                    server = %self.name,
                    requirement = %requirement.name,
                    error = %e,
                    "failed to acquire mcp server requirement"
                );
            }
        }

        let (command, args, env, cwd) = match &self.transport {
            ServerTransport::Http { url, headers } => {
                return Ok(SpawnSpec {
                    name: self.name.clone(),
                    startup_timeout: self.startup_timeout,
                    kind: SpawnKind::Http {
                        url: url.clone(),
                        headers: headers.clone(),
                        config_dir: sym.config_dir().to_path_buf(),
                    },
                });
            }
            ServerTransport::Stdio {
                command,
                args,
                env,
                cwd,
            } => (command, args, env, cwd),
        };
        let args_from_entry = args;

        let (command, mut args) = match command {
            ServerCommand::Path(path) => (path.clone(), Vec::new()),
            ServerCommand::Installation(installation) => {
                let acquired = crate::installation::acquire_installation(
                    sym,
                    installation,
                    None,
                    None,
                    update,
                )
                .await?;
                let label = format!("mcp server `{}`", self.name);
                match crate::installation::resolve_runnable(acquired, &label)? {
                    symposium_install::Runnable::Exec(path) => (path, Vec::new()),
                    // A script is run through a shell, as hooks are.
                    symposium_install::Runnable::Script(path) => (
                        PathBuf::from("sh"),
                        vec![path.to_string_lossy().into_owned()],
                    ),
                }
            }
        };
        args.extend(args_from_entry.iter().cloned());

        Ok(SpawnSpec {
            name: self.name.clone(),
            startup_timeout: self.startup_timeout,
            kind: SpawnKind::Child {
                command,
                args,
                env: env.clone(),
                cwd: cwd.clone(),
            },
        })
    }

    /// Whether a plugin's filters let this tool through.
    pub fn exposes(&self, tool: &str) -> bool {
        if let Some(allow) = &self.enabled_tools {
            return allow.iter().any(|t| t == tool);
        }
        if let Some(deny) = &self.disabled_tools {
            return !deny.iter().any(|t| t == tool);
        }
        true
    }
}

/// What resolution produced, including what it had to refuse.
#[derive(Debug, Default)]
pub struct Resolution {
    pub servers: Vec<ResolvedServer>,
    /// Servers that could not be used, and why. Reported rather than
    /// swallowed: a server silently missing looks like a broken workspace.
    pub rejected: Vec<Rejection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub server: String,
    pub reason: String,
}

/// Resolve the servers applicable to the workspace containing `cwd`.
pub async fn resolve(sym: &Symposium, cwd: &Path) -> Resolution {
    let deps = sym.workspace_deps(cwd);
    if deps.load().is_none() {
        // Outside a Rust workspace there is nothing to condition on.
        return Resolution::default();
    }
    resolve_with_deps(sym, &deps).await
}

/// Resolve against a workspace resolver the caller already holds, so the hook
/// pipeline shares its one `cargo metadata` result with this pass.
pub async fn resolve_with_deps(
    sym: &Symposium,
    deps: &Arc<crate::pm::WorkspaceDeps>,
) -> Resolution {
    let Some(loaded) = deps.load().cloned() else {
        return Resolution::default();
    };
    let registry = crate::plugins::load_registry_with_workspace(sym, Some(&loaded)).await;

    let dep_ids = crate::pm::workspace_dep_ids(sym, deps).await;
    let used_names = sym.config.plugins.used_names_in(&loaded.root);
    let mut ctx = crate::predicate::PredicateContext::new(&dep_ids).with_used_names(&used_names);

    // The same active set every other facet resolves over, so a crate-sourced
    // plugin's servers are reachable exactly like a registry plugin's.
    let pms = sym.package_managers(deps);
    let active =
        crate::plugins::active_plugins(sym, &registry, &pms, Some(&loaded.root), &mut ctx).await;

    let mut entries: Vec<Candidate> = Vec::new();
    for plugin in &active {
        if !plugin.applies(&mut ctx) {
            continue;
        }
        for entry in plugin.plugin.applicable_mcp_entries(&mut ctx) {
            entries.push(Candidate {
                entry,
                owner: plugin.plugin.name.clone(),
                plugin: &plugin.plugin,
            });
        }
    }

    build(entries, &loaded.root, sym.config.mcp.script_timeout_secs)
}

/// Acquire what applicable servers need, once per session.
///
/// Two different intents, treated differently:
///
/// * **`requirements`** are acquired eagerly. Declaring one *is* the author
///   saying "warm this up" — it is how a package-runner server avoids paying
///   its download on the first tool call.
/// * **`installation`** commands are only refreshed if already present, as
///   hooks are. Installing every declared server eagerly would fetch tools a
///   session may never touch.
///
/// Best-effort throughout: a failure here only means the cost lands later.
pub async fn prewarm(sym: &Symposium, resolution: &Resolution) {
    let update = symposium_install::UpdateLevel::Check;

    for server in &resolution.servers {
        for requirement in &server.requirements {
            if let Err(e) =
                crate::installation::acquire_installation(sym, requirement, None, None, update)
                    .await
            {
                tracing::debug!(
                    server = %server.name,
                    requirement = %requirement.name,
                    error = %e,
                    "prewarm: requirement acquisition failed"
                );
            }
        }

        if let ServerTransport::Stdio {
            command: ServerCommand::Installation(installation),
            ..
        } = &server.transport
        {
            if let Err(e) =
                crate::installation::refresh_installation_if_present(sym, installation, None).await
            {
                tracing::debug!(
                    server = %server.name,
                    error = %e,
                    "prewarm: command refresh failed"
                );
            }
        }
    }
}

/// An applicable entry together with the plugin that declared it, whose
/// `[[installations]]` its `installation` and `requirements` name.
struct Candidate<'a> {
    entry: &'a crate::plugins::PluginMcpServer,
    owner: String,
    plugin: &'a crate::plugins::Plugin,
}

/// Turn applicable manifest entries into runnable servers.
///
/// `root` anchors a relative `cwd`: a plugin author cannot know what
/// directory the agent was launched from.
fn build(entries: Vec<Candidate<'_>>, root: &Path, script_timeout_secs: u64) -> Resolution {
    let mut resolution = Resolution::default();
    // Which plugin claimed each name, so a clash can name both sides.
    let mut claimed: Vec<(String, String)> = Vec::new();

    for Candidate {
        entry,
        owner,
        plugin,
    } in entries
    {
        let name = entry.name.clone();

        // The meta-server's own tools live in the same namespace as the
        // servers it exposes; a backing server taking one would shadow it.
        if name == LIST_TOOLS || name == EXECUTE {
            resolution.rejected.push(Rejection {
                server: name,
                reason: format!("`{owner}` uses a name reserved by the meta-server"),
            });
            continue;
        }

        // First-wins would silently drop one plugin's server, and a warning
        // on a stdio server's stderr is invisible. Refusing names both.
        if let Some((_, first)) = claimed.iter().find(|(n, _)| *n == name) {
            resolution.rejected.push(Rejection {
                server: name.clone(),
                reason: format!("declared by both `{first}` and `{owner}`"),
            });
            continue;
        }

        let transport = match &entry.transport {
            McpTransport::Sse(_) => {
                resolution.rejected.push(Rejection {
                    server: name,
                    reason: "the SSE transport is deprecated; declare a streamable HTTP server \
                             with `url` instead"
                        .to_string(),
                });
                continue;
            }
            McpTransport::Http(remote) => match expand_remote(remote) {
                Ok((url, headers)) => ServerTransport::Http { url, headers },
                Err(unset) => {
                    resolution.rejected.push(Rejection {
                        server: name,
                        reason: format!(
                            "`{owner}` references environment variable `{unset}`, which is not set"
                        ),
                    });
                    continue;
                }
            },
            McpTransport::Stdio(stdio) => {
                let command = match &stdio.command {
                    StdioCommand::Path(path) => ServerCommand::Path(path.clone()),
                    StdioCommand::Installation(installation) => {
                        match plugin.get_installation(installation) {
                            Some(found) => ServerCommand::Installation(Box::new(found.clone())),
                            None => {
                                resolution.rejected.push(Rejection {
                                    server: name,
                                    reason: format!(
                                        "`{owner}` names installation `{installation}`, which it does not declare"
                                    ),
                                });
                                continue;
                            }
                        }
                    }
                };
                ServerTransport::Stdio {
                    command,
                    args: stdio.args.clone(),
                    env: stdio
                        .env
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                    cwd: stdio.cwd.as_ref().map(|dir| root.join(dir)),
                }
            }
        };

        // Named requirements have to exist too, or a warmup silently does
        // nothing.
        let mut requirements = Vec::new();
        let mut missing = None;
        for requirement in &entry.requirements {
            match plugin.get_installation(requirement) {
                Some(found) => requirements.push(found.clone()),
                None => {
                    missing = Some(requirement.clone());
                    break;
                }
            }
        }
        if let Some(requirement) = missing {
            resolution.rejected.push(Rejection {
                server: name,
                reason: format!(
                    "`{owner}` names requirement `{requirement}`, which it does not declare"
                ),
            });
            continue;
        }

        claimed.push((name.clone(), owner));
        resolution.servers.push(ResolvedServer {
            name: name.clone(),
            transport,
            startup_timeout: Duration::from_secs(
                entry.overrides.startup_timeout_secs.unwrap_or(30),
            ),
            tool_call_timeout: call_timeout(&entry.overrides, script_timeout_secs),
            enabled_tools: entry.overrides.enabled_tools.clone(),
            disabled_tools: entry.overrides.disabled_tools.clone(),
            requirements,
        });
    }

    resolution.servers.sort_by(|a, b| a.name.cmp(&b.name));
    resolution
}

/// Expand `${VAR}` references in a remote server's url and headers.
///
/// Returns the name of the first variable that is unset and has no default, so
/// the caller can refuse the server by name. Sending a literal `${TOKEN}` to a
/// remote endpoint would instead look like an authentication failure.
fn expand_remote(
    remote: &crate::plugins::RemoteServer,
) -> Result<(String, Vec<(String, String)>), String> {
    let url = expand_vars(&remote.url)?;
    let mut headers = Vec::with_capacity(remote.headers.len());
    for (key, value) in &remote.headers {
        headers.push((key.clone(), expand_vars(value)?));
    }
    Ok((url, headers))
}

/// Substitute `${VAR}` and `${VAR:-default}` from the environment.
fn expand_vars(input: &str) -> Result<String, String> {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return Ok(out);
        };
        let reference = &after[..end];
        let (name, default) = match reference.split_once(":-") {
            Some((name, default)) => (name, Some(default)),
            None => (reference, None),
        };
        match std::env::var(name) {
            Ok(value) => out.push_str(&value),
            Err(_) => match default {
                Some(default) => out.push_str(default),
                None => return Err(name.to_string()),
            },
        }
        rest = &after[end + 1..];
    }

    out.push_str(rest);
    Ok(out)
}

/// Reconcile a plugin's call timeout with the user's script deadline.
///
/// A plugin author cannot see the user's configuration, so an override
/// longer than the whole script budget is clamped rather than rejected —
/// refusing to load a server because a user lowered their own limit would
/// punish the wrong person.
fn call_timeout(overrides: &McpServerOverrides, script_timeout_secs: u64) -> Duration {
    let requested = overrides.tool_call_timeout_secs.unwrap_or(60);
    // Leave the script deadline strictly larger, or the call timeout could
    // never fire.
    let ceiling = script_timeout_secs.saturating_sub(1).max(1);
    Duration::from_secs(requested.min(ceiling))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::PluginMcpServer;

    fn stdio(name: &str) -> PluginMcpServer {
        PluginMcpServer {
            name: name.to_string(),
            predicates: Default::default(),
            overrides: McpServerOverrides::default(),
            transport: McpTransport::Stdio(crate::plugins::StdioServer {
                command: StdioCommand::Path("/usr/bin/true".into()),
                args: Vec::new(),
                env: Default::default(),
                cwd: None,
            }),
            requirements: Vec::new(),
        }
    }

    /// A plugin carrying the given entries, so `installation` and
    /// `requirements` have somewhere to resolve against.
    fn owner_plugin(installations: Vec<crate::plugins::Installation>) -> crate::plugins::Plugin {
        crate::plugins::Plugin {
            name: "db-plugin".to_string(),
            predicates: Default::default(),
            installations,
            hooks: Vec::new(),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            subcommands: Default::default(),
            custom_predicates: Vec::new(),
            chained: Vec::new(),
            requires_use: false,
        }
    }

    fn resolve_with(
        entries: Vec<&PluginMcpServer>,
        plugin: &crate::plugins::Plugin,
        script_secs: u64,
    ) -> Resolution {
        build(
            entries
                .into_iter()
                .map(|entry| Candidate {
                    entry,
                    owner: plugin.name.clone(),
                    plugin,
                })
                .collect(),
            Path::new("/ws"),
            script_secs,
        )
    }

    /// Entries paired with the plugin name that declared each, against a
    /// plugin declaring no installations.
    fn resolve_all(entries: Vec<(&PluginMcpServer, &str)>, script_secs: u64) -> Resolution {
        let plugin = owner_plugin(Vec::new());
        build(
            entries
                .into_iter()
                .map(|(entry, owner)| Candidate {
                    entry,
                    owner: owner.to_string(),
                    plugin: &plugin,
                })
                .collect(),
            Path::new("/ws"),
            script_secs,
        )
    }

    fn stdio_cwd(server: &ResolvedServer) -> Option<&Path> {
        match &server.transport {
            ServerTransport::Stdio { cwd, .. } => cwd.as_deref(),
            ServerTransport::Http { .. } => None,
        }
    }

    #[test]
    fn relative_cwd_resolves_against_the_workspace_root() {
        let mut entry = stdio("sqlx");
        if let McpTransport::Stdio(s) = &mut entry.transport {
            s.cwd = Some("crates/db".into());
        }
        let out = resolve_all(vec![(&entry, "db-plugin")], 120);
        assert_eq!(
            stdio_cwd(&out.servers[0]),
            Some(Path::new("/ws/crates/db")),
            "got: {:?}",
            stdio_cwd(&out.servers[0])
        );
    }

    #[test]
    fn absolute_cwd_is_left_as_written() {
        let mut entry = stdio("sqlx");
        if let McpTransport::Stdio(s) = &mut entry.transport {
            s.cwd = Some("/opt/db".into());
        }
        let out = resolve_all(vec![(&entry, "db-plugin")], 120);
        assert_eq!(stdio_cwd(&out.servers[0]), Some(Path::new("/opt/db")));
    }

    #[test]
    fn stdio_servers_become_spawnable() {
        let entry = stdio("sqlx");
        let out = resolve_all(vec![(&entry, "db-plugin")], 120);

        assert_eq!(out.servers.len(), 1);
        assert_eq!(out.servers[0].name, "sqlx");
        assert!(out.rejected.is_empty());
    }

    /// A silently missing server looks like a broken workspace, so refusals
    /// are reported.
    fn remote(name: &str, url: &str, headers: &[(&str, &str)]) -> PluginMcpServer {
        PluginMcpServer {
            name: name.to_string(),
            predicates: Default::default(),
            overrides: McpServerOverrides::default(),
            transport: McpTransport::Http(crate::plugins::RemoteServer {
                url: url.to_string(),
                headers: headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            }),
            requirements: Vec::new(),
        }
    }

    #[test]
    fn http_servers_resolve_with_their_headers() {
        let entry = remote(
            "sqlx",
            "https://mcp.example.com/mcp",
            &[("Authorization", "Bearer t")],
        );
        let out = resolve_all(vec![(&entry, "p")], 120);

        assert_eq!(out.servers.len(), 1, "rejected: {:?}", out.rejected);
        match &out.servers[0].transport {
            ServerTransport::Http { url, headers } => {
                assert_eq!(url, "https://mcp.example.com/mcp");
                assert_eq!(
                    headers,
                    &vec![("Authorization".to_string(), "Bearer t".to_string())]
                );
            }
            other => panic!("expected an http transport, got {other:?}"),
        }
    }

    /// The SSE transport is deprecated in the protocol and has no client in the
    /// SDK, so it is refused by name rather than half-supported.
    #[test]
    fn sse_servers_are_refused_pointing_at_streamable_http() {
        let mut entry = remote("sqlx", "https://mcp.example.com/sse", &[]);
        if let McpTransport::Http(remote) = entry.transport.clone() {
            entry.transport = McpTransport::Sse(remote);
        }
        let out = resolve_all(vec![(&entry, "p")], 120);

        assert!(out.servers.is_empty());
        assert_eq!(out.rejected.len(), 1);
        assert!(
            out.rejected[0].reason.contains("url"),
            "got: {}",
            out.rejected[0].reason
        );
    }

    /// A literal `${TOKEN}` reaching a remote endpoint would look like an
    /// authentication failure, so an unset variable refuses the server instead.
    #[test]
    fn an_unset_variable_refuses_the_server_naming_it() {
        let entry = remote(
            "sqlx",
            "https://mcp.example.com/mcp",
            &[("Authorization", "Bearer ${SYMPOSIUM_TEST_UNSET_TOKEN}")],
        );
        let out = resolve_all(vec![(&entry, "p")], 120);

        assert!(out.servers.is_empty());
        assert_eq!(out.rejected.len(), 1);
        assert!(
            out.rejected[0]
                .reason
                .contains("SYMPOSIUM_TEST_UNSET_TOKEN"),
            "got: {}",
            out.rejected[0].reason
        );
    }

    #[test]
    fn a_default_stands_in_for_an_unset_variable() {
        let entry = remote(
            "sqlx",
            "${SYMPOSIUM_TEST_UNSET_BASE:-https://fallback.example.com}/mcp",
            &[],
        );
        let out = resolve_all(vec![(&entry, "p")], 120);

        assert_eq!(out.servers.len(), 1, "rejected: {:?}", out.rejected);
        match &out.servers[0].transport {
            ServerTransport::Http { url, .. } => {
                assert_eq!(url, "https://fallback.example.com/mcp")
            }
            other => panic!("expected an http transport, got {other:?}"),
        }
    }

    /// First-wins would drop one plugin's server silently, and a warning on
    /// a stdio server's stderr is invisible.
    #[test]
    fn duplicate_names_are_refused_naming_both_plugins() {
        let a = stdio("sqlx");
        let b = stdio("sqlx");
        let out = resolve_all(vec![(&a, "first-plugin"), (&b, "second-plugin")], 120);

        assert_eq!(out.servers.len(), 1, "the first still works");
        assert_eq!(out.rejected.len(), 1);
        let reason = &out.rejected[0].reason;
        assert!(
            reason.contains("first-plugin") && reason.contains("second-plugin"),
            "both sides should be named, got: {reason}"
        );
    }

    /// A backing server called `execute` would shadow the meta-server's own
    /// tool.
    #[test]
    fn reserved_names_are_refused() {
        for name in [LIST_TOOLS, EXECUTE] {
            let entry = stdio(name);
            let out = resolve_all(vec![(&entry, "p")], 120);
            assert!(out.servers.is_empty(), "{name} should be refused");
            assert!(out.rejected[0].reason.contains("reserved"));
        }
    }

    #[test]
    fn per_server_timeouts_are_honored() {
        let mut entry = stdio("slow");
        entry.overrides.startup_timeout_secs = Some(45);
        entry.overrides.tool_call_timeout_secs = Some(90);
        let out = resolve_all(vec![(&entry, "p")], 300);

        assert_eq!(out.servers[0].startup_timeout, Duration::from_secs(45));
        assert_eq!(out.servers[0].tool_call_timeout, Duration::from_secs(90));
    }

    /// A plugin author cannot see the user's configuration, so an override
    /// beyond the script budget is clamped rather than refused.
    #[test]
    fn call_timeout_is_clamped_below_the_script_deadline() {
        let mut entry = stdio("slow");
        entry.overrides.tool_call_timeout_secs = Some(600);
        let out = resolve_all(vec![(&entry, "p")], 30);

        assert_eq!(
            out.servers[0].tool_call_timeout,
            Duration::from_secs(29),
            "must stay strictly under the script deadline or it can never fire"
        );
    }

    // -- installations --

    fn installation(name: &str, install_commands: Vec<String>) -> crate::plugins::Installation {
        crate::plugins::Installation {
            name: name.to_string(),
            requirements: Vec::new(),
            install_commands,
            source: None,
            executable: Some("/usr/bin/true".to_string()),
            script: None,
            args: Vec::new(),
        }
    }

    fn installation_backed(
        name: &str,
        installation: &str,
        requirements: &[&str],
    ) -> PluginMcpServer {
        PluginMcpServer {
            name: name.to_string(),
            predicates: Default::default(),
            overrides: McpServerOverrides::default(),
            transport: McpTransport::Stdio(crate::plugins::StdioServer {
                command: StdioCommand::Installation(installation.to_string()),
                args: Vec::new(),
                env: Default::default(),
                cwd: None,
            }),
            requirements: requirements.iter().map(|r| r.to_string()).collect(),
        }
    }

    /// The definition is carried, not resolved: acquiring means downloading,
    /// and that must not happen while resolving.
    #[test]
    fn an_installation_backed_server_carries_its_definition() {
        let entry = installation_backed("sqlx", "sqlx-mcp", &[]);
        let plugin = owner_plugin(vec![installation("sqlx-mcp", vec![])]);
        let out = resolve_with(vec![&entry], &plugin, 120);

        assert_eq!(out.servers.len(), 1, "rejected: {:?}", out.rejected);
        assert!(matches!(
            &out.servers[0].transport,
            ServerTransport::Stdio {
                command: ServerCommand::Installation(_),
                ..
            }
        ));
    }

    #[test]
    fn an_unknown_installation_is_refused_naming_it() {
        let entry = installation_backed("sqlx", "missing", &[]);
        let plugin = owner_plugin(Vec::new());
        let out = resolve_with(vec![&entry], &plugin, 120);

        assert!(out.servers.is_empty());
        assert!(
            out.rejected[0].reason.contains("missing"),
            "got: {:?}",
            out.rejected
        );
    }

    /// A warmup that names nothing would silently do nothing, so the
    /// reference has to be checked.
    #[test]
    fn an_unknown_requirement_is_refused_naming_it() {
        let entry = installation_backed("sqlx", "sqlx-mcp", &["not-declared"]);
        let plugin = owner_plugin(vec![installation("sqlx-mcp", vec![])]);
        let out = resolve_with(vec![&entry], &plugin, 120);

        assert!(out.servers.is_empty());
        assert!(
            out.rejected[0].reason.contains("not-declared"),
            "got: {:?}",
            out.rejected
        );
    }

    #[test]
    fn requirements_are_carried_for_acquisition() {
        let entry = installation_backed("sqlx", "sqlx-mcp", &["warmup"]);
        let plugin = owner_plugin(vec![
            installation("sqlx-mcp", vec![]),
            installation("warmup", vec!["true".to_string()]),
        ]);
        let out = resolve_with(vec![&entry], &plugin, 120);

        assert_eq!(out.servers.len(), 1, "rejected: {:?}", out.rejected);
        assert_eq!(out.servers[0].requirements.len(), 1);
        assert_eq!(out.servers[0].requirements[0].name, "warmup");
    }

    /// A declared requirement is acquired eagerly: that is the author asking
    /// for a warm cache, and the alternative is the download landing on the
    /// agent's first tool call.
    #[tokio::test]
    async fn prewarm_runs_declared_requirements() {
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join("warmed");
        let sym = Symposium::from_dir(tmp.path());

        let entry = installation_backed("sqlx", "sqlx-mcp", &["warmup"]);
        let plugin = owner_plugin(vec![
            installation("sqlx-mcp", vec![]),
            installation("warmup", vec![format!("touch {}", marker.display())]),
        ]);
        let resolution = resolve_with(vec![&entry], &plugin, 120);

        assert!(!marker.exists(), "resolving must not run anything");
        prewarm(&sym, &resolution).await;
        assert!(
            marker.exists(),
            "the warmup should have run at prewarm time"
        );
    }

    /// A warmup that fails only means the cost lands later, so it must not
    /// take the session with it.
    #[tokio::test]
    async fn prewarm_survives_a_failing_requirement() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());

        let entry = installation_backed("sqlx", "sqlx-mcp", &["warmup"]);
        let plugin = owner_plugin(vec![
            installation("sqlx-mcp", vec![]),
            installation("warmup", vec!["exit 1".to_string()]),
        ]);
        let resolution = resolve_with(vec![&entry], &plugin, 120);

        prewarm(&sym, &resolution).await;
    }

    // -- tool filters --

    #[test]
    fn an_allow_list_hides_everything_else() {
        let mut entry = stdio("sqlx");
        entry.overrides.enabled_tools = Some(vec!["query".into()]);
        let out = resolve_all(vec![(&entry, "p")], 120);

        assert!(out.servers[0].exposes("query"));
        assert!(!out.servers[0].exposes("drop_table"));
    }

    #[test]
    fn a_deny_list_hides_only_what_it_names() {
        let mut entry = stdio("sqlx");
        entry.overrides.disabled_tools = Some(vec!["drop_table".into()]);
        let out = resolve_all(vec![(&entry, "p")], 120);

        assert!(out.servers[0].exposes("query"));
        assert!(!out.servers[0].exposes("drop_table"));
    }

    /// An empty allow-list means nothing, which is different from declaring
    /// no filter at all.
    #[test]
    fn an_empty_allow_list_exposes_nothing() {
        let mut entry = stdio("sqlx");
        entry.overrides.enabled_tools = Some(vec![]);
        let out = resolve_all(vec![(&entry, "p")], 120);

        assert!(!out.servers[0].exposes("query"));
    }

    #[test]
    fn without_filters_every_tool_is_exposed() {
        let entry = stdio("sqlx");
        let out = resolve_all(vec![(&entry, "p")], 120);
        assert!(out.servers[0].exposes("anything"));
    }

    /// Order must not depend on registry iteration, or the inventory shown
    /// to the model would shift between sessions.
    #[test]
    fn servers_are_ordered_by_name() {
        let b = stdio("b-server");
        let a = stdio("a-server");
        let out = resolve_all(vec![(&b, "p"), (&a, "p")], 120);

        let names: Vec<&str> = out.servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["a-server", "b-server"]);
    }
}
