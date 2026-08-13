//! What the workspace's tools look like to the model.
//!
//! `list_tools` answers with an index by default — one line per tool — and
//! full TypeScript declarations only when asked. Truncating a full dump would
//! be lossy and would vary with the workspace; an index is lossless and
//! strictly smaller. A single mainstream server's schemas can fill a whole
//! declaration budget on their own, so this is the common case, not a
//! precaution.
//!
//! Descriptions require a live `tools/list`, so the first call starts the
//! servers it needs. Cold start is paid at first disclosure rather than at
//! session start.

use std::sync::Arc;
use std::time::Duration;

use crate::config::Symposium;

use rmcp::model::Tool;
use serde_json::Value;
use tokio::sync::Mutex;

use super::declarations::{
    KeyMatch, ToolBinding, ToolDecl, binding_table, render_server, resolve_key,
};
use super::dispatch::Namespace;
use super::resolve::{Rejection, Resolution, ResolvedServer, ServerCommand};
use super::supervisor::{RestartPolicy, Supervisor};

/// How much to say about each tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Detail {
    /// Name and description. Enough to choose; not enough to call.
    #[default]
    Names,
    /// One TypeScript signature per tool.
    Signatures,
    /// Full declarations, including the types parameters refer to.
    Full,
}

impl Detail {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("signatures") => Self::Signatures,
            Some("full") => Self::Full,
            _ => Self::Names,
        }
    }
}

/// Which tools to describe.
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub servers: Option<Vec<String>>,
    pub tools: Option<Vec<String>>,
    pub pattern: Option<String>,
    pub detail: Detail,
}

impl Query {
    /// Read a query from the tool's arguments, ignoring anything unrecognized.
    pub fn from_arguments(args: &Value) -> Self {
        Self {
            servers: string_list(args.get("servers")),
            tools: string_list(args.get("tools")),
            pattern: args
                .get("pattern")
                .and_then(Value::as_str)
                .map(str::to_string),
            detail: Detail::parse(args.get("detail").and_then(Value::as_str)),
        }
    }

    /// Naming specific servers or tools implies wanting their details.
    fn effective_detail(&self) -> Detail {
        if self.detail == Detail::Names && (self.servers.is_some() || self.tools.is_some()) {
            Detail::Full
        } else {
            self.detail
        }
    }

    fn wants_server(&self, name: &str) -> bool {
        self.servers
            .as_ref()
            .is_none_or(|only| only.iter().any(|s| s == name))
    }

    fn wants_tool(&self, name: &str) -> bool {
        if let Some(only) = &self.tools {
            if !only.iter().any(|t| t == name) {
                return false;
            }
        }
        match &self.pattern {
            Some(pattern) => glob_matches(pattern, name),
            None => true,
        }
    }
}

/// What the workspace resolved to, swapped as a whole so a refresh is never
/// observed half-applied.
struct CatalogState {
    entries: Vec<Entry>,
    /// Servers that could not be used at all. Reported to the model rather
    /// than only logged: a server silently missing looks like a workspace
    /// that never declared it.
    rejected: Vec<Rejection>,
    /// Filter entries the caller named that match no server.
    known_names: Vec<String>,
}

/// The workspace's backing servers, described on demand.
pub struct Catalog {
    state: std::sync::RwLock<Arc<CatalogState>>,
    read_only: bool,
    policy: RestartPolicy,
    /// Needed to acquire an installation-backed server on first use.
    sym: Arc<Symposium>,
    /// Where the session is running, so the workspace can be resolved again.
    cwd: std::path::PathBuf,
    /// Modification time of `Cargo.lock` when the state was last built.
    resolved_at: std::sync::Mutex<Option<std::time::SystemTime>>,
}

/// What one tool call produced.
///
/// The notice is carried beside the value rather than folded into it: wrapping
/// the result would change the shape the script sees, and the script is not who
/// the notice is for.
pub struct CallOutcome {
    pub value: Value,
    /// Set when the server's answer did not match the output schema it
    /// declared, so the caller can tell the model its type did not hold.
    pub notice: Option<String>,
}

struct Entry {
    resolved: ResolvedServer,
    /// Absent until first use. Building it may acquire an installation, which
    /// must not happen at startup — a client may spawn a throwaway copy of the
    /// meta-server just to probe it.
    ///
    /// Shared so a refresh can carry a running server into the new state.
    supervisor: Arc<Mutex<Option<Supervisor>>>,
}

impl Catalog {
    pub fn new(
        sym: Arc<Symposium>,
        resolution: Resolution,
        policy: RestartPolicy,
        read_only: bool,
        cwd: std::path::PathBuf,
    ) -> Self {
        let resolved_at = cargo_lock_mtime(&cwd);
        Self {
            state: std::sync::RwLock::new(Arc::new(CatalogState::new(resolution, &[]))),
            read_only,
            policy,
            sym,
            cwd,
            resolved_at: std::sync::Mutex::new(resolved_at),
        }
    }

    fn state(&self) -> Arc<CatalogState> {
        Arc::clone(&self.state.read().expect("catalog state lock"))
    }

    /// Rebuild the server set when the workspace changed under us, so a
    /// dependency added mid-session exposes its tools without a restart.
    ///
    /// Servers whose spawn is unchanged are carried across still running.
    async fn refresh_if_stale(&self) {
        if !self.sym.config.auto_sync {
            return;
        }
        let Some(mtime) = cargo_lock_mtime(&self.cwd) else {
            return;
        };
        {
            let seen = self.resolved_at.lock().expect("catalog mtime lock");
            if *seen == Some(mtime) {
                return;
            }
        }

        let resolution = crate::mcp::resolve::resolve(&self.sym, &self.cwd).await;
        let previous = self.state();
        let next = Arc::new(CatalogState::new(resolution, &previous.entries));

        // Anything the new state did not adopt is no longer applicable.
        let dropped: Vec<Arc<Mutex<Option<Supervisor>>>> = previous
            .entries
            .iter()
            .filter(|old| {
                !next
                    .entries
                    .iter()
                    .any(|new| Arc::ptr_eq(&new.supervisor, &old.supervisor))
            })
            .map(|old| Arc::clone(&old.supervisor))
            .collect();

        *self.state.write().expect("catalog state lock") = next;
        *self.resolved_at.lock().expect("catalog mtime lock") = Some(mtime);

        for supervisor in dropped {
            if let Some(running) = supervisor.lock().await.as_mut() {
                running.shutdown().await;
            }
        }
    }

    /// The supervisor for an entry, acquiring what it needs the first time.
    async fn supervisor_for<'a>(
        &self,
        entry: &'a Entry,
    ) -> Result<tokio::sync::MutexGuard<'a, Option<Supervisor>>, String> {
        let mut guard = entry.supervisor.lock().await;
        if guard.is_none() {
            let spec = entry
                .resolved
                .spawn_spec(&self.sym)
                .await
                .map_err(|e| format!("{e:#}"))?;
            *guard = Some(Supervisor::new(spec, self.policy));
        }
        Ok(guard)
    }

    pub fn server_names(&self) -> Vec<String> {
        self.state().known_names.clone()
    }

    pub fn is_empty(&self) -> bool {
        self.state().entries.is_empty()
    }

    /// Describe the matching tools.
    pub async fn describe(&self, query: &Query) -> String {
        self.refresh_if_stale().await;
        let state = self.state();
        if state.entries.is_empty() && state.rejected.is_empty() {
            return "No MCP servers apply to this workspace.".to_string();
        }

        let mut sections = Vec::new();
        // Refusals first: they explain an absence the model would otherwise
        // have to infer.
        let mut problems: Vec<String> = state
            .rejected
            .iter()
            .map(|r| format!("{}: {}", r.server, r.reason))
            .collect();

        for entry in &state.entries {
            if !query.wants_server(entry.resolved.name.as_str()) {
                continue;
            }

            let tools = match self.supervisor_for(entry).await {
                Ok(mut guard) => match guard.as_mut() {
                    Some(supervisor) => supervisor.list_tools().await.map_err(|e| e.to_string()),
                    None => unreachable!("supervisor_for leaves it present"),
                },
                Err(e) => Err(e),
            };
            let tools = match tools {
                Ok(tools) => tools,
                Err(e) => {
                    // A server that will not start is reported in place, so
                    // the absence of its tools has a visible reason.
                    problems.push(format!("{}: {e}", entry.resolved.name.as_str()));
                    continue;
                }
            };

            let visible: Vec<&Tool> = tools
                .iter()
                .filter(|t| entry.resolved.exposes(t.name.as_ref()))
                .filter(|t| !self.read_only || is_read_only(t))
                .filter(|t| query.wants_tool(t.name.as_ref()))
                .collect();

            if visible.is_empty() {
                // Silence here reads as a broken connection, so say why.
                if !tools.is_empty() && !query_narrows(query) {
                    problems.push(format!(
                        "{}: no tools visible ({} hidden by filters)",
                        entry.resolved.name.as_str(),
                        tools.len()
                    ));
                }
                continue;
            }

            sections.push(render(
                entry.resolved.name.as_str(),
                &visible,
                query.effective_detail(),
            ));
        }

        // Naming a server that does not exist is a mistake worth surfacing
        // rather than answering with silence.
        if let Some(requested) = &query.servers {
            for name in requested {
                if !state.known_names.iter().any(|k| k == name) {
                    problems.push(format!(
                        "no server named `{name}`. Available: {}",
                        state.known_names.join(", ")
                    ));
                }
            }
        }

        if sections.is_empty() && problems.is_empty() {
            return "No tools matched.".to_string();
        }

        let mut out = sections.join("\n");
        if !problems.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str("Problems:\n");
            for problem in problems {
                out.push_str(&format!("  {problem}\n"));
            }
        }
        out
    }

    /// The namespaces a script sees, one per server.
    ///
    /// Nothing is started here; a namespace resolves its tools on first call.
    pub async fn namespaces(&self) -> (Vec<Namespace>, Vec<String>) {
        self.refresh_if_stale().await;
        let state = self.state();
        let namespaces = state
            .entries
            .iter()
            .map(|entry| Namespace {
                key: namespace_key(entry.resolved.name.as_str()),
                server: entry.resolved.name.as_str().to_string(),
            })
            .collect();

        // A refused server never becomes a namespace, so its absence needs a
        // reason here.
        let problems = state
            .rejected
            .iter()
            .map(|r| format!("{}: {}", r.server, r.reason))
            .collect();

        (namespaces, problems)
    }

    /// Call a tool on a backing server, honoring its filters.
    /// `key` is the property name the script used, which may be a sanitized
    /// alias. Resolving it needs the tool list, so this is what starts the
    /// server.
    pub async fn call(&self, server: &str, key: &str, args: Value) -> Result<CallOutcome, String> {
        let state = self.state();
        let Some(entry) = state.entries.iter().find(|e| e.resolved.name == server) else {
            return Err(format!(
                "no server named `{server}`. Available: {}",
                state.known_names.join(", ")
            ));
        };

        let timeout = entry.resolved.tool_call_timeout;
        let mut guard = self.supervisor_for(entry).await?;
        let supervisor = guard.as_mut().expect("supervisor_for leaves it present");

        let tools = supervisor.list_tools().await.map_err(|e| e.to_string())?;
        let visible: Vec<&str> = tools
            .iter()
            .filter(|t| entry.resolved.exposes(t.name.as_ref()))
            .filter(|t| !self.read_only || is_read_only(t))
            .map(|t| t.name.as_ref())
            .collect();

        // The same table the declarations are rendered from.
        let table = binding_table(visible);
        let wire_name = match resolve_key(&table, key) {
            KeyMatch::One(binding) => binding.wire_name.clone(),
            KeyMatch::Ambiguous(names) => {
                return Err(format!(
                    "`{server}` has more than one tool spelled like `{key}`: {}. \
                     Use one of those names exactly.",
                    names.join(", ")
                ));
            }
            KeyMatch::None => return Err(unknown_tool(server, key, &table)),
        };

        // Taken before the call so the tool list, which the mutable borrow of
        // the supervisor ends, is still in hand.
        let declared = tools
            .iter()
            .find(|t| t.name.as_ref() == wire_name)
            .and_then(|t| t.output_schema.clone());

        let value = supervisor
            .call(&wire_name, args, timeout)
            .await
            .map_err(|e| e.to_string())?;

        // Checked after unwrapping, since that is the value a script actually
        // receives. A mismatch is reported, never fatal: the value may still be
        // readable, and the model is the one who can decide (see
        // [`crate::mcp::validate`]).
        let notice = declared.and_then(|schema| {
            crate::mcp::validate::check_result(
                server,
                &wire_name,
                &Value::Object((*schema).clone()),
                &value,
            )
        });

        Ok(CallOutcome { value, notice })
    }

    /// Close every running server.
    pub async fn shutdown(&self) {
        for entry in &self.state().entries {
            if let Some(supervisor) = entry.supervisor.lock().await.as_mut() {
                supervisor.shutdown().await;
            }
        }
    }

    /// How long a script may run against this catalog's servers.
    pub fn max_call_timeout(&self) -> Duration {
        self.state()
            .entries
            .iter()
            .map(|e| e.resolved.tool_call_timeout)
            .max()
            .unwrap_or_default()
    }
}

/// A server's name as a JavaScript global.
fn namespace_key(server: &str) -> String {
    let mut out: String = server
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

fn query_narrows(query: &Query) -> bool {
    query.tools.is_some() || query.pattern.is_some()
}

impl CatalogState {
    /// Adopts any still-matching server from `previous`, so a running child
    /// survives a refresh.
    fn new(resolution: Resolution, previous: &[Entry]) -> Self {
        let Resolution { servers, rejected } = resolution;
        let known_names = servers.iter().map(|s| s.name.clone()).collect();
        let entries = servers
            .into_iter()
            .map(|resolved| {
                let supervisor = previous
                    .iter()
                    .find(|old| same_spawn(&old.resolved, &resolved))
                    .map(|old| Arc::clone(&old.supervisor))
                    .unwrap_or_default();
                Entry {
                    resolved,
                    supervisor,
                }
            })
            .collect();
        Self {
            entries,
            rejected,
            known_names,
        }
    }
}

/// Whether two resolutions describe the same child process. Only the spawn
/// matters; anything else that moved in the manifest does not.
fn same_spawn(a: &ResolvedServer, b: &ResolvedServer) -> bool {
    a.name == b.name
        && a.args == b.args
        && a.env == b.env
        && a.cwd == b.cwd
        && match (&a.command, &b.command) {
            (ServerCommand::Path(x), ServerCommand::Path(y)) => x == y,
            (ServerCommand::Installation(x), ServerCommand::Installation(y)) => x.name == y.name,
            _ => false,
        }
}

/// `Cargo.lock`'s modification time, searched upward from `cwd`. Walked
/// rather than asked of cargo: this runs on every describe.
fn cargo_lock_mtime(cwd: &std::path::Path) -> Option<std::time::SystemTime> {
    let mut dir = Some(cwd);
    while let Some(current) = dir {
        let candidate = current.join("Cargo.lock");
        if let Ok(meta) = std::fs::metadata(&candidate) {
            return meta.modified().ok();
        }
        dir = current.parent();
    }
    None
}

/// Report a name no tool answers to.
///
/// The proxy answers any property, so a typo only surfaces here. Naming the
/// nearest match makes it recoverable inside the script.
fn unknown_tool(server: &str, key: &str, table: &[ToolBinding]) -> String {
    let mut names: Vec<&str> = table
        .iter()
        .flat_map(|b| b.keys.iter().map(String::as_str))
        .collect();
    names.sort_unstable();

    match closest(key, &names) {
        Some(nearest) => format!(
            "`{server}` has no tool `{key}`. Closest match: `{nearest}`. Call `{list}` for the full list.",
            list = crate::mcp::server::LIST_TOOLS
        ),
        None => format!(
            "`{server}` exposes no tools. Call `{list}` to see what is available.",
            list = crate::mcp::server::LIST_TOOLS
        ),
    }
}

/// The candidate sharing the longest prefix with `key`: a wrong suffix or a
/// dropped separator.
fn closest<'a>(key: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let normalize = |s: &str| s.to_ascii_lowercase().replace(['-', '_'], "");
    let target = normalize(key);
    candidates
        .iter()
        .copied()
        .max_by_key(|candidate| {
            let other = normalize(candidate);
            let shared = target
                .chars()
                .zip(other.chars())
                .take_while(|(a, b)| a == b)
                .count();
            (shared, usize::MAX - other.len())
        })
        .filter(|candidate| {
            let other = normalize(candidate);
            target.chars().next() == other.chars().next()
        })
}

fn is_read_only(tool: &Tool) -> bool {
    tool.annotations
        .as_ref()
        .and_then(|a| a.read_only_hint)
        .unwrap_or(false)
}

fn render(server: &str, tools: &[&Tool], detail: Detail) -> String {
    match detail {
        Detail::Names => {
            let mut out = format!("{server}:\n");
            for tool in tools {
                match tool.description.as_deref().map(str::trim) {
                    Some(text) if !text.is_empty() => {
                        out.push_str(&format!("  {} - {}\n", tool.name, first_line(text)));
                    }
                    _ => out.push_str(&format!("  {}\n", tool.name)),
                }
            }
            out
        }
        Detail::Signatures | Detail::Full => {
            // The schemas are owned so the declaration renderer, which works
            // in plain JSON, never sees a protocol type. Output schemas follow
            // the input schema's gate: signatures name a tool's shape-free
            // form, so spelling out the return there while hiding the
            // parameter would be inconsistent.
            let schemas: Vec<(Option<Value>, Option<Value>)> = tools
                .iter()
                .map(|tool| {
                    if detail != Detail::Full {
                        return (None, None);
                    }
                    (
                        Some(Value::Object((*tool.input_schema).clone())),
                        tool.output_schema
                            .as_ref()
                            .map(|schema| Value::Object((**schema).clone())),
                    )
                })
                .collect();
            let decls: Vec<ToolDecl> = tools
                .iter()
                .zip(&schemas)
                .map(|(tool, (input, output))| ToolDecl {
                    name: tool.name.as_ref(),
                    description: tool.description.as_deref(),
                    // Signatures name the parameter; full spells out its shape.
                    input_schema: input.as_ref(),
                    output_schema: output.as_ref(),
                })
                .collect();
            render_server(server, &decls)
        }
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_string()
}

fn string_list(value: Option<&Value>) -> Option<Vec<String>> {
    let array = value?.as_array()?;
    Some(
        array
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
    )
}

/// Match a name against a pattern where `*` stands for any run of characters.
///
/// A dependency for this would be more machinery than the feature is worth.
fn glob_matches(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let mut rest = name;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match index {
            // A pattern not starting with `*` must match from the front.
            0 => match rest.strip_prefix(part) {
                Some(tail) => rest = tail,
                None => return false,
            },
            _ if index == parts.len() - 1 => {
                // The final piece must land at the end.
                return rest.ends_with(part) && rest.len() >= part.len();
            }
            _ => match rest.find(part) {
                Some(at) => rest = &rest[at + part.len()..],
                None => return false,
            },
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The global a script uses must be a legal identifier even when the
    /// server's declared name is not.
    #[test]
    fn namespace_keys_are_identifiers() {
        assert_eq!(namespace_key("sqlx"), "sqlx");
        assert_eq!(namespace_key("sea-orm"), "sea_orm");
        assert_eq!(namespace_key("2fa"), "_2fa");
    }

    #[test]
    fn detail_defaults_to_names() {
        assert_eq!(Detail::parse(None), Detail::Names);
        assert_eq!(Detail::parse(Some("nonsense")), Detail::Names);
        assert_eq!(Detail::parse(Some("full")), Detail::Full);
        assert_eq!(Detail::parse(Some("signatures")), Detail::Signatures);
    }

    /// Asking about specific servers or tools is a request for detail; making
    /// the caller also pass `detail` would be a needless second round trip.
    #[test]
    fn naming_a_target_implies_wanting_detail() {
        let query = Query {
            servers: Some(vec!["sqlx".into()]),
            ..Query::default()
        };
        assert_eq!(query.effective_detail(), Detail::Full);

        let unfiltered = Query::default();
        assert_eq!(unfiltered.effective_detail(), Detail::Names);
    }

    #[test]
    fn explicit_detail_is_respected_even_when_filtering() {
        let query = Query {
            servers: Some(vec!["sqlx".into()]),
            detail: Detail::Signatures,
            ..Query::default()
        };
        assert_eq!(query.effective_detail(), Detail::Signatures);
    }

    #[test]
    fn arguments_are_read_leniently() {
        let query = Query::from_arguments(&serde_json::json!({
            "servers": ["a", "b"],
            "pattern": "get_*",
            "detail": "full",
            "unrecognized": 1
        }));
        assert_eq!(query.servers, Some(vec!["a".into(), "b".into()]));
        assert_eq!(query.pattern.as_deref(), Some("get_*"));
        assert_eq!(query.detail, Detail::Full);
        assert_eq!(query.tools, None);
    }

    // -- glob --

    #[test]
    fn glob_matches_prefixes_suffixes_and_middles() {
        assert!(glob_matches("get_*", "get_user"));
        assert!(!glob_matches("get_*", "set_user"));
        assert!(glob_matches("*_user", "get_user"));
        assert!(!glob_matches("*_user", "get_team"));
        assert!(glob_matches("get_*_by_*", "get_user_by_id"));
        assert!(glob_matches("*", "anything"));
    }

    #[test]
    fn glob_without_a_wildcard_is_an_exact_match() {
        assert!(glob_matches("query", "query"));
        assert!(!glob_matches("query", "query2"));
    }
}
