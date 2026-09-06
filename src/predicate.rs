//! Predicate **evaluation**: checking a predicate against the workspace
//! dependency graph and the live environment.
//!
//! The predicate *syntax*: the [`Predicate`] tree, the `depends-on` and
//! `predicates` surface spellings, parsing, `Display`, and serde: lives in
//! [`symposium_sdk::predicate::syntax`] and is re-exported here, so
//! `crate::predicate::Predicate` still names the same type it always did. The
//! split is what lets a package manager carry predicates through a manifest
//! without linking evaluation: a PM never asks whether a predicate holds.
//!
//! Evaluation is [`PredicateEval`], an extension trait rather than an inherent
//! method, since the types are now foreign. A predicate is purely a boolean
//! gate.

use std::path::Path;
use std::process::Command;

use std::sync::Arc;

use anyhow::Result;
use symposium_sdk::predicate::CustomPredicateEvent;
pub use symposium_sdk::predicate::syntax::{
    DependsOnList, Predicate, PredicateSet, parse_dep_atom, validate_custom_predicate_name,
};

use crate::pm::PackageId;
use crate::predicate_cache::{
    CacheEntry, CacheTtl, Fingerprints, PredicateCache, WatchSet, cache_key, now_ms,
};

/// Evaluate a predicate (or a set of them) against a [`PredicateContext`].
///
/// An extension trait because [`Predicate`] and [`PredicateSet`] are defined in
/// the SDK, which knows nothing about workspaces or the environment.
pub trait PredicateEval {
    /// True if this holds in `ctx`.
    fn evaluate(&self, ctx: &mut PredicateContext) -> bool;
}

impl PredicateEval for Predicate {
    /// Short-circuits: `any` stops at the first true child, `all` at the first
    /// false.
    fn evaluate(&self, ctx: &mut PredicateContext) -> bool {
        match self {
            Predicate::DependsOn(name, version_req) => ctx
                .deps
                .iter()
                .any(|dep| dep_matches(dep, name, version_req.as_ref())),
            Predicate::DependsOnWildcard => true,
            Predicate::Shell(cmd) => run_shell(cmd),
            Predicate::PathExists(arg) => path_exists(arg),
            Predicate::Env(name, expected) => env_matches(name, expected.as_deref()),
            Predicate::WorkspaceMember => ctx.workspace_member,
            Predicate::Not(inner) => !inner.evaluate(ctx),
            Predicate::Any(children) => children.iter().any(|p| p.evaluate(ctx)),
            Predicate::All(children) => children.iter().all(|p| p.evaluate(ctx)),
            Predicate::Custom { name, arg } => ctx.evaluate_custom(name, arg),
        }
    }
}

impl PredicateEval for PredicateSet {
    /// True if every predicate holds (or the set is empty).
    fn evaluate(&self, ctx: &mut PredicateContext) -> bool {
        self.predicates.iter().all(|p| p.evaluate(ctx))
    }
}

/// The evaluation environment a predicate is checked against.
///
/// The workspace dependency list is passed explicitly; the OS environment
/// (`shell`, `path_exists`, `env`) is read ambiently at evaluation time. Custom
/// (plugin-defined) predicates are resolved entries whose results are cached
/// for the lifetime of the context.
#[derive(Debug)]
pub struct PredicateContext<'a> {
    pub deps: &'a [PackageId],
    /// Whether the plugin currently being evaluated is defined by a member
    /// of the active workspace. This is *provenance*, not a workspace fact:
    /// the loader stamps it per plugin (via `ParsedPlugin::applies`) before
    /// that plugin's predicate sets are evaluated.
    workspace_member: bool,
    /// Plugin names enabled by the applicable `[plugins] use` entries,
    /// normalized. A plugin with no gate of its own
    /// ([`Plugin::requires_use`](crate::plugins::Plugin::requires_use)) is
    /// dormant unless it is named here.
    used_names: std::collections::HashSet<String>,
    custom_entries: std::collections::HashMap<String, ResolvedPredicateEntry>,
    custom_cache: std::collections::HashMap<(String, String), CustomPredicateResult>,
    /// The current (possibly mutated) view of the on-disk cache.
    disk_cache: Option<Arc<PredicateCache>>,
    /// The load-time snapshot. `Arc::ptr_eq(&disk_cache, &disk_cache_original)`
    /// stays true until the first mutation goes through `Arc::make_mut`, so
    /// "was this modified?" is derived from the data, not a separate flag.
    disk_cache_original: Option<Arc<PredicateCache>>,
}

impl<'a> PredicateContext<'a> {
    pub fn new(deps: &'a [PackageId]) -> Self {
        Self {
            deps,
            workspace_member: false,
            used_names: std::collections::HashSet::new(),
            custom_entries: std::collections::HashMap::new(),
            custom_cache: std::collections::HashMap::new(),
            disk_cache: None,
            disk_cache_original: None,
        }
    }

    pub fn with_custom_predicates(
        deps: &'a [PackageId],
        entries: std::collections::HashMap<String, ResolvedPredicateEntry>,
    ) -> Self {
        Self {
            custom_entries: entries,
            ..Self::new(deps)
        }
    }

    /// Record the plugin names the applicable `[plugins] use` entries enable.
    /// Matching is hyphen/underscore-insensitive, like every other name
    /// comparison against user-typed config.
    pub fn with_used_names<S: AsRef<str>>(mut self, names: &[S]) -> Self {
        self.used_names.extend(
            names
                .iter()
                .map(|n| crate::crate_sources::normalize_crate_name(n.as_ref())),
        );
        self
    }

    /// Is the named plugin enabled by a `use` entry in this context?
    pub fn is_used(&self, plugin_name: &str) -> bool {
        self.used_names
            .contains(&crate::crate_sources::normalize_crate_name(plugin_name))
    }

    /// Load a persistent cache from disk and attach it to this context.
    /// Missing / malformed cache files yield an empty cache (see
    /// [`PredicateCache::load`]).
    pub fn with_disk_cache(mut self, cache_path: &Path) -> Self {
        let loaded = Arc::new(PredicateCache::load(cache_path));
        self.disk_cache_original = Some(Arc::clone(&loaded));
        self.disk_cache = Some(loaded);
        self
    }

    /// Persist the disk cache back to `cache_path`. No-op if this context
    /// was not built with `with_disk_cache` or the cache was not modified.
    pub fn persist_disk_cache(&mut self, cache_path: &Path) -> Result<()> {
        let (Some(current), Some(original)) = (&self.disk_cache, &self.disk_cache_original) else {
            return Ok(());
        };
        if Arc::ptr_eq(current, original) {
            return Ok(());
        }
        current.save(cache_path)?;
        self.disk_cache_original = Some(Arc::clone(current));
        Ok(())
    }

    /// Stamp whether the plugin about to be evaluated arrived via workspace
    /// membership. Call before evaluating each plugin's predicate sets; the
    /// value applies to all of that plugin's nested components (groups,
    /// skills, hooks, MCP servers, subcommands).
    pub fn set_workspace_member(&mut self, workspace_member: bool) {
        self.workspace_member = workspace_member;
    }

    /// Evaluate a custom predicate by name and argument, returning the cached
    /// result if already computed. Consults the in-memory cache first, then
    /// the on-disk cache (when present). On miss, spawns the predicate and
    /// updates both caches according to the emitted watch events.
    fn evaluate_custom(&mut self, name: &str, arg: &str) -> bool {
        let mem_key = (name.to_string(), arg.to_string());
        if let Some(result) = self.custom_cache.get(&mem_key) {
            return result.passed;
        }

        let disk_key = cache_key(name, arg);
        if let Some(cache) = &self.disk_cache
            && let Some(entry) = cache.get(&disk_key)
            && !entry.is_time_expired(now_ms())
            && Fingerprints::capture(&watch_set_from_entry(entry)) == entry.fingerprints
        {
            // Disk hit. Populate the in-memory cache with a result
            // that has no events; the events belong to the run that
            // originally produced this entry.
            let passed = entry.result;
            self.custom_cache.insert(
                mem_key,
                CustomPredicateResult {
                    passed,
                    events: Vec::new(),
                },
            );
            return passed;
        }

        let result = run_custom_predicate(&self.custom_entries, name, arg);
        let passed = result.passed;

        if let Some(cache_arc) = self.disk_cache.as_mut() {
            let cache = Arc::make_mut(cache_arc);
            let set = WatchSet::from_events(&result.events);
            if !matches!(set.cache_ttl, CacheTtl::Never) {
                cache.put(disk_key, CacheEntry::from_result(passed, &set));
            } else {
                // Explicit no-cache: drop any stale entry we might have had.
                cache.entries.remove(&cache_key(name, arg));
            }
        }

        self.custom_cache.insert(mem_key, result);
        passed
    }
}

/// Recompute the watch set from a stored cache entry so we can capture
/// fresh fingerprints and compare them against the stored ones.
fn watch_set_from_entry(entry: &CacheEntry) -> WatchSet {
    WatchSet {
        files: entry.fingerprints.files.keys().cloned().collect(),
        env: entry.fingerprints.env.keys().cloned().collect(),
        cache_ttl: CacheTtl::Forever,
    }
}

/// A dependency id satisfies a `depends-on` atom when the name matches
/// exactly and, when the atom carries a version requirement, the id's
/// version component parses as semver and satisfies it.
fn dep_matches(dep: &PackageId, name: &str, req: Option<&semver::VersionReq>) -> bool {
    dep.name == name
        && req.is_none_or(|req| semver::Version::parse(&dep.version).is_ok_and(|v| req.matches(&v)))
}

// --- environment evaluation ---

fn env_matches(name: &str, expected: Option<&str>) -> bool {
    match expected {
        None => std::env::var_os(name).is_some(),
        Some(value) => std::env::var(name).ok().as_deref() == Some(value),
    }
}

fn run_shell(command: &str) -> bool {
    match Command::new("sh").arg("-c").arg(command).output() {
        Ok(out) if out.status.success() => {
            tracing::trace!(command = %command, "shell predicate passed");
            true
        }
        Ok(out) => {
            tracing::trace!(
                command = %command,
                exit_code = ?out.status.code(),
                stderr = %String::from_utf8_lossy(&out.stderr),
                "shell predicate failed",
            );
            false
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // A missing `sh` makes every shell(...) predicate silently false,
            // which is confusing to debug. Surface it once at warn level.
            tracing::warn!(
                command = %command,
                "shell predicate evaluated false because `sh` was not found on PATH; \
                 add a POSIX shell to PATH to enable shell(...) predicates",
            );
            false
        }
        Err(e) => {
            tracing::trace!(command = %command, error = %e, "shell predicate failed to spawn");
            false
        }
    }
}

fn path_exists(arg: &str) -> bool {
    if arg.is_empty() {
        return false;
    }
    if Path::new(arg).exists() {
        return true;
    }
    if arg.contains('/') || arg.contains(std::path::MAIN_SEPARATOR) {
        return false;
    }
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(arg).exists()))
        .unwrap_or(false)
}

// --- custom predicate evaluation infrastructure ---

/// Cached result of a custom predicate invocation. A custom predicate is a
/// boolean gate: it passes iff the command exits 0.
///
/// `events` holds every well-formed [`CustomPredicateEvent`] the predicate
/// emitted on stdout. Wiring these into cache invalidation is deferred to a
/// follow-up PR; today the events are captured for observability only.
#[derive(Debug, Clone)]
pub struct CustomPredicateResult {
    pub passed: bool,
    #[allow(dead_code)] // consumed by the cache in a follow-up PR
    pub events: Vec<CustomPredicateEvent>,
}

/// A resolved custom predicate entry ready for invocation.
#[derive(Debug)]
pub struct ResolvedPredicateEntry {
    pub runnable: symposium_install::Runnable,
    pub args: Vec<String>,
}

/// Spawn a custom predicate command; it passes iff it exits 0.
fn run_custom_predicate(
    entries: &std::collections::HashMap<String, ResolvedPredicateEntry>,
    name: &str,
    arg: &str,
) -> CustomPredicateResult {
    let Some(entry) = entries.get(name) else {
        tracing::warn!(predicate = name, "custom predicate not found in registry");
        return CustomPredicateResult {
            passed: false,
            events: Vec::new(),
        };
    };

    let mut full_args: Vec<&str> = entry.args.iter().map(|s| s.as_str()).collect();
    if !arg.is_empty() {
        full_args.push(arg);
    }

    tracing::debug!(
        predicate = name,
        args = ?full_args,
        "spawning custom predicate"
    );

    match entry.runnable.spawn(&full_args) {
        Ok(output) => {
            if !output.stderr.is_empty() {
                tracing::debug!(
                    predicate = name,
                    stderr = %String::from_utf8_lossy(&output.stderr),
                    "custom predicate stderr"
                );
            }
            let events = parse_predicate_events(name, &output.stdout);
            CustomPredicateResult {
                passed: output.status.success(),
                events,
            }
        }
        Err(e) => {
            tracing::warn!(
                predicate = name,
                error = %e,
                "failed to spawn custom predicate"
            );
            CustomPredicateResult {
                passed: false,
                events: Vec::new(),
            }
        }
    }
}

/// Parse a predicate's stdout as a JSON Lines stream of
/// [`CustomPredicateEvent`]s. Blank lines are skipped. A line that is not
/// valid UTF-8 or not a known event is logged and skipped so unknown record
/// types do not break older Symposium versions.
fn parse_predicate_events(predicate_name: &str, stdout: &[u8]) -> Vec<CustomPredicateEvent> {
    let text = match std::str::from_utf8(stdout) {
        Ok(s) => s,
        Err(_) => {
            tracing::warn!(
                predicate = predicate_name,
                "custom predicate stdout is not valid UTF-8; discarding events"
            );
            return Vec::new();
        }
    };

    let mut events = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<CustomPredicateEvent>(line) {
            Ok(event) => events.push(event),
            Err(e) => {
                tracing::warn!(
                    predicate = predicate_name,
                    error = %e,
                    line,
                    "unknown custom predicate event; skipping"
                );
            }
        }
    }
    events
}

#[cfg(test)]
mod tests {
    // The parsing tests below exercise the SDK's parser directly.
    use super::*;
    use crate::pm::CARGO_PM;
    use symposium_sdk::predicate::syntax::parse_predicate as parse;

    fn ctx<'a>(deps: &'a [PackageId]) -> PredicateContext<'a> {
        PredicateContext::new(deps)
    }

    fn ws(pairs: &[(&str, &str)]) -> Vec<PackageId> {
        pairs
            .iter()
            .map(|(n, ver)| PackageId::new(CARGO_PM, *n, *ver))
            .collect()
    }

    // --- workspace-member ---

    #[test]
    fn workspace_member_parses_and_roundtrips() {
        let p = parse("workspace-member()").unwrap();
        assert_eq!(p, Predicate::WorkspaceMember);
        assert_eq!(p.to_string(), "workspace-member()");
        // No-argument predicate: an argument is a parse error.
        assert!(parse("workspace-member(foo)").is_err());
        // Reserved: a custom predicate can't claim the name.
        assert!(validate_custom_predicate_name("workspace-member").is_err());
    }

    #[test]
    fn workspace_member_follows_context_stamp() {
        let deps = ws(&[]);
        let mut c = ctx(&deps);
        let p = Predicate::WorkspaceMember;
        assert!(!p.evaluate(&mut c));

        c.set_workspace_member(true);
        assert!(p.evaluate(&mut c));
        // Composes with combinators.
        assert!(!Predicate::Not(Box::new(Predicate::WorkspaceMember)).evaluate(&mut c));

        c.set_workspace_member(false);
        assert!(!p.evaluate(&mut c));
    }

    // --- crate-atom parsing ---

    #[test]
    fn parse_crate_atom_bare_and_versioned() {
        assert_eq!(
            parse_dep_atom("serde").unwrap(),
            Predicate::DependsOn("serde".into(), None)
        );
        assert_eq!(
            parse_dep_atom("serde>=1.0").unwrap(),
            Predicate::DependsOn(
                "serde".into(),
                Some(semver::VersionReq::parse(">=1.0").unwrap())
            )
        );
        assert_eq!(parse_dep_atom("*").unwrap(), Predicate::DependsOnWildcard);
    }

    #[test]
    fn crate_list_lowers_to_any() {
        assert_eq!(DependsOnList::parse("").unwrap().into_predicate(), None);
        assert_eq!(
            DependsOnList::parse("serde").unwrap().into_predicate(),
            Some(Predicate::DependsOn("serde".into(), None))
        );
        assert_eq!(
            DependsOnList::parse("serde, tokio")
                .unwrap()
                .into_predicate(),
            Some(Predicate::Any(vec![
                Predicate::DependsOn("serde".into(), None),
                Predicate::DependsOn("tokio".into(), None),
            ]))
        );
        // Function-call syntax is rejected in the `depends-on` field.
        assert!(DependsOnList::parse("bp(cli, web)").is_err());
        assert!(DependsOnList::parse("serde, bp(a, b)").is_err());
        assert!(DependsOnList::parse("all()").is_err());
        assert!(DependsOnList::parse("depends-on(serde)").is_err());
        assert!(DependsOnList::parse("not(serde)").is_err());
        assert!(DependsOnList::parse("shell(true)").is_err());
    }

    // --- function-call parsing ---

    #[test]
    fn predicates_field_rejects_bare_names() {
        // The `predicates` field requires function-call syntax.
        assert!(parse("serde").is_err());
        assert!(parse("tokio>=1.0").is_err());
        assert!(parse("*").is_err());
    }

    #[test]
    fn parse_rejects_renamed_crate_predicate() {
        let err = parse("crate(serde)").unwrap_err();
        assert!(
            err.to_string().contains("use `depends-on(serde)` instead"),
            "expected migration hint, got: {err}"
        );
    }

    #[test]
    fn parse_function_calls() {
        assert_eq!(
            parse("depends-on(serde)").unwrap(),
            Predicate::DependsOn("serde".into(), None)
        );
        assert_eq!(
            parse("depends-on(*)").unwrap(),
            Predicate::DependsOnWildcard
        );
        assert_eq!(
            parse("shell(command -v rg)").unwrap(),
            Predicate::Shell("command -v rg".into())
        );
        assert_eq!(parse("env(CI)").unwrap(), Predicate::Env("CI".into(), None));
        assert_eq!(
            parse("not(depends-on(serde))").unwrap(),
            Predicate::Not(Box::new(Predicate::DependsOn("serde".into(), None)))
        );
        assert_eq!(
            parse("any(depends-on(a), path_exists(rg))").unwrap(),
            Predicate::Any(vec![
                Predicate::DependsOn("a".into(), None),
                Predicate::PathExists("rg".into()),
            ])
        );
        assert!(parse("all()").is_err());
        // Unknown function names now parse as Custom predicates
        assert_eq!(
            parse("bogus(x)").unwrap(),
            Predicate::Custom {
                name: "bogus".into(),
                arg: "x".into()
            }
        );
    }

    // --- evaluation ---

    #[test]
    fn evaluate_crate_and_wildcard() {
        let w = ws(&[("serde", "1.0.0")]);
        assert!(parse("depends-on(serde)").unwrap().evaluate(&mut ctx(&w)));
        assert!(!parse("depends-on(tokio)").unwrap().evaluate(&mut ctx(&w)));
        assert!(parse("depends-on(*)").unwrap().evaluate(&mut ctx(&[])));
    }

    #[test]
    fn evaluate_combinators() {
        let w = ws(&[("serde", "1.0.0")]);
        assert!(
            parse("not(depends-on(tokio))")
                .unwrap()
                .evaluate(&mut ctx(&w))
        );
        assert!(
            parse("any(depends-on(tokio), depends-on(serde))")
                .unwrap()
                .evaluate(&mut ctx(&w))
        );
        assert!(
            !parse("all(depends-on(serde), depends-on(tokio))")
                .unwrap()
                .evaluate(&mut ctx(&w))
        );
    }

    #[test]
    fn path_exists_empty_is_false() {
        // `path_exists()` must not resolve to a `$PATH` dir via `dir.join("")`.
        assert!(!Predicate::PathExists(String::new()).evaluate(&mut ctx(&[])));
    }

    // --- introspection ---

    #[test]
    fn collect_and_references_walk_the_tree() {
        let p = parse("any(depends-on(serde), not(depends-on(tokio)))").unwrap();
        let mut names = std::collections::BTreeSet::new();
        p.collect_dep_names(&mut names);
        assert_eq!(
            names.into_iter().collect::<Vec<_>>(),
            vec!["serde", "tokio"]
        );
        assert!(p.references_dep("serde"));
        assert!(p.references_dep("tokio"));
        assert!(!p.references_dep("anyhow"));
    }

    #[test]
    fn has_concrete_dep() {
        assert!(
            PredicateSet::from_depends_on("serde")
                .unwrap()
                .has_concrete_dep()
        );
        assert!(
            !PredicateSet::from_depends_on("*")
                .unwrap()
                .has_concrete_dep()
        );
        assert!(
            !PredicateSet::parse("shell(true)")
                .unwrap()
                .has_concrete_dep()
        );
    }

    // --- Display round-trip ---

    #[test]
    fn display_round_trip() {
        for input in [
            "depends-on(serde)",
            "depends-on(serde>=1.0)",
            "depends-on(*)",
            "shell(command -v rg)",
            "path_exists(rg)",
            "env(CI)",
            "env(MODE=debug)",
            "not(depends-on(serde))",
            "any(depends-on(a), path_exists(b))",
            "all(depends-on(a), not(env(CI)))",
        ] {
            let p = parse(input).unwrap();
            assert_eq!(p.to_string(), input, "display drift: {input}");
            assert_eq!(parse(&p.to_string()).unwrap(), p, "round-trip: {input}");
        }
    }

    // --- TOML deserialization of the two fields ---

    #[test]
    fn toml_fields_deserialize() {
        #[derive(serde::Deserialize)]
        struct Container {
            #[serde(default, rename = "depends-on")]
            depends_on: DependsOnList,
            #[serde(default)]
            predicates: PredicateSet,
        }
        let c: Container = toml::from_str(
            r#"depends-on = ["serde", "tokio>=1.0"]
               predicates = ["path_exists(jq)", "not(depends-on(foo))"]"#,
        )
        .unwrap();
        assert_eq!(c.depends_on.0.len(), 2);
        assert_eq!(c.predicates.predicates.len(), 2);

        // single-string depends-on form
        let c2: Container = toml::from_str(r#"depends-on = "serde""#).unwrap();
        assert_eq!(
            c2.depends_on.0,
            vec![Predicate::DependsOn("serde".into(), None)]
        );
    }

    // --- Custom predicate parsing tests ---

    #[test]
    fn parse_custom_predicate_expression() {
        let p = parse("battery_pack(cli>=0.3)").unwrap();
        assert_eq!(
            p,
            Predicate::Custom {
                name: "battery_pack".into(),
                arg: "cli>=0.3".into()
            }
        );
    }

    #[test]
    fn parse_custom_predicate_rejects_invalid_names() {
        // Hyphens not allowed
        assert!(parse("battery-pack(cli>=0.3)").is_err());
        assert!(parse("my-pred()").is_err());
        // Must start with a letter
        assert!(parse("0foo(x)").is_err());
        assert!(parse("_foo(x)").is_err());
        // Builtin names cannot be redefined (they're matched first anyway,
        // but the validator rejects them if somehow reached)
        assert!(validate_custom_predicate_name("crate").is_err());
        assert!(validate_custom_predicate_name("shell").is_err());
        assert!(validate_custom_predicate_name("not").is_err());
    }

    #[test]
    fn parse_custom_predicate_empty_arg() {
        let p = parse("my_pred()").unwrap();
        assert_eq!(
            p,
            Predicate::Custom {
                name: "my_pred".into(),
                arg: "".into()
            }
        );
    }

    #[test]
    fn parse_custom_predicate_arg_with_parens() {
        let p = parse("foo(bar(baz))").unwrap();
        assert_eq!(
            p,
            Predicate::Custom {
                name: "foo".into(),
                arg: "bar(baz)".into()
            }
        );
    }

    #[test]
    fn display_roundtrip_custom() {
        let p = Predicate::Custom {
            name: "battery_pack".into(),
            arg: "cli>=0.3".into(),
        };
        let displayed = p.to_string();
        assert_eq!(displayed, "battery_pack(cli>=0.3)");
        let reparsed = parse(&displayed).unwrap();
        assert_eq!(p, reparsed);
    }

    #[test]
    fn custom_not_confused_with_builtin() {
        let p = parse("depends-on(serde)").unwrap();
        assert_eq!(p, Predicate::DependsOn("serde".into(), None));
    }

    #[test]
    fn has_concrete_crate_custom_is_false() {
        let p = Predicate::Custom {
            name: "foo".into(),
            arg: "x".into(),
        };
        assert!(!p.has_concrete_dep());
    }

    #[test]
    fn mentions_crate_custom_is_false() {
        let p = Predicate::Custom {
            name: "foo".into(),
            arg: "x".into(),
        };
        assert!(!p.mentions_dep());
    }

    #[test]
    fn references_crate_custom_is_false() {
        let p = Predicate::Custom {
            name: "foo".into(),
            arg: "x".into(),
        };
        assert!(!p.references_dep("foo"));
        assert!(!p.references_dep("x"));
    }

    #[test]
    fn collect_crate_names_custom_is_noop() {
        let p = Predicate::Custom {
            name: "foo".into(),
            arg: "x".into(),
        };
        let mut names = std::collections::BTreeSet::new();
        p.collect_dep_names(&mut names);
        assert!(names.is_empty());
    }

    // --- Custom predicate evaluation tests ---

    /// Create a context with custom predicate entries using shell scripts.
    /// Each entry is `(name, exit_code)` — the script does `exit <code>`.
    fn ctx_with_exit_codes(
        entries: Vec<(&str, u8)>,
    ) -> (PredicateContext<'static>, Vec<tempfile::NamedTempFile>) {
        use std::io::Write;
        let mut map = std::collections::HashMap::new();
        let mut scripts = Vec::new();
        for (name, code) in entries {
            let script = tempfile::Builder::new().suffix(".sh").tempfile().unwrap();
            writeln!(script.as_file(), "#!/bin/sh\nexit {code}").unwrap();
            map.insert(
                name.to_string(),
                ResolvedPredicateEntry {
                    runnable: symposium_install::Runnable::Script(script.path().to_path_buf()),
                    args: vec![],
                },
            );
            scripts.push(script);
        }
        (PredicateContext::with_custom_predicates(&[], map), scripts)
    }

    /// Render `path` for use *inside* a `/bin/sh` script body. On Windows `sh`
    /// is git-bash's MSYS shell, which reads `C:\a\b` as escapes; rewrite it to
    /// the `/c/a/b` form the shell understands.
    #[cfg(windows)]
    fn sh_path(path: &Path) -> String {
        let slashed = path.to_string_lossy().replace('\\', "/");
        match slashed.split_once(':') {
            Some((drive, rest)) if drive.len() == 1 => {
                format!("/{}{}", drive.to_ascii_lowercase(), rest)
            }
            _ => slashed,
        }
    }

    #[cfg(not(windows))]
    fn sh_path(path: &Path) -> String {
        path.to_string_lossy().to_string()
    }

    #[test]
    fn evaluate_custom_predicate_pass() {
        let (mut ctx, _scripts) = ctx_with_exit_codes(vec![("foo", 0)]);
        let pred = Predicate::Custom {
            name: "foo".into(),
            arg: "x".into(),
        };
        assert!(pred.evaluate(&mut ctx));
    }

    #[test]
    fn evaluate_custom_predicate_fail() {
        let (mut ctx, _scripts) = ctx_with_exit_codes(vec![("foo", 1)]);
        let pred = Predicate::Custom {
            name: "foo".into(),
            arg: "x".into(),
        };
        assert!(!pred.evaluate(&mut ctx));
    }

    #[test]
    fn evaluate_custom_predicate_missing_from_registry() {
        let mut ctx = PredicateContext::new(&[]);
        let pred = Predicate::Custom {
            name: "nonexistent".into(),
            arg: "x".into(),
        };
        assert!(!pred.evaluate(&mut ctx));
    }

    #[test]
    fn evaluate_custom_predicate_spawn_failure() {
        use std::collections::HashMap;
        let mut entries = HashMap::new();
        entries.insert(
            "foo".to_string(),
            ResolvedPredicateEntry {
                runnable: symposium_install::Runnable::Exec(std::path::PathBuf::from(
                    "/nonexistent/binary/zzz",
                )),
                args: vec![],
            },
        );
        let mut ctx = PredicateContext::with_custom_predicates(&[], entries);
        let pred = Predicate::Custom {
            name: "foo".into(),
            arg: "x".into(),
        };
        assert!(!pred.evaluate(&mut ctx));
    }

    #[test]
    fn evaluate_custom_predicate_cached() {
        use std::io::Write;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let counter_path = tmp.path().to_path_buf();

        let script = tempfile::Builder::new().suffix(".sh").tempfile().unwrap();
        writeln!(
            script.as_file(),
            "#!/bin/sh\necho x >> \"{}\"\nexit 0",
            sh_path(&counter_path)
        )
        .unwrap();

        let mut entries = std::collections::HashMap::new();
        entries.insert(
            "counter".to_string(),
            ResolvedPredicateEntry {
                runnable: symposium_install::Runnable::Script(script.path().to_path_buf()),
                args: vec![],
            },
        );
        let mut ctx = PredicateContext::with_custom_predicates(&[], entries);
        let pred = Predicate::Custom {
            name: "counter".into(),
            arg: "a".into(),
        };

        // Evaluate twice with same (name, arg)
        assert!(pred.evaluate(&mut ctx));
        assert!(pred.evaluate(&mut ctx));

        // Script should have been called only once
        let content = std::fs::read_to_string(&counter_path).unwrap();
        assert_eq!(content.lines().count(), 1);
    }

    #[test]
    fn evaluate_custom_predicate_args_appended() {
        use std::io::Write;
        let output_file = tempfile::NamedTempFile::new().unwrap();
        let output_path = output_file.path().to_path_buf();

        let script = tempfile::Builder::new().suffix(".sh").tempfile().unwrap();
        writeln!(
            script.as_file(),
            "#!/bin/sh\necho \"$@\" > \"{}\"",
            sh_path(&output_path)
        )
        .unwrap();

        let mut entries = std::collections::HashMap::new();
        entries.insert(
            "checker".to_string(),
            ResolvedPredicateEntry {
                runnable: symposium_install::Runnable::Script(script.path().to_path_buf()),
                args: vec!["--static".into(), "arg".into()],
            },
        );
        let mut ctx = PredicateContext::with_custom_predicates(&[], entries);
        let pred = Predicate::Custom {
            name: "checker".into(),
            arg: "dynamic-arg".into(),
        };

        assert!(pred.evaluate(&mut ctx));

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert_eq!(content.trim(), "--static arg dynamic-arg");
    }

    #[test]
    fn evaluate_custom_predicate_empty_arg_not_passed() {
        use std::io::Write;
        let output_file = tempfile::NamedTempFile::new().unwrap();
        let output_path = output_file.path().to_path_buf();

        let script = tempfile::Builder::new().suffix(".sh").tempfile().unwrap();
        writeln!(
            script.as_file(),
            "#!/bin/sh\necho \"$@\" > \"{}\"",
            sh_path(&output_path)
        )
        .unwrap();

        let mut entries = std::collections::HashMap::new();
        entries.insert(
            "checker".to_string(),
            ResolvedPredicateEntry {
                runnable: symposium_install::Runnable::Script(script.path().to_path_buf()),
                args: vec!["--static".into()],
            },
        );
        let mut ctx = PredicateContext::with_custom_predicates(&[], entries);

        // Empty arg (from `foo()`) — should not be appended.
        let pred = Predicate::Custom {
            name: "checker".into(),
            arg: "".into(),
        };
        assert!(pred.evaluate(&mut ctx));
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert_eq!(content.trim(), "--static");
    }

    #[test]
    fn parse_custom_predicate_whitespace_arg_is_empty() {
        // `foo( )` parses to empty arg after trimming.
        let p = parse("foo( )").unwrap();
        assert_eq!(
            p,
            Predicate::Custom {
                name: "foo".into(),
                arg: "".into()
            }
        );
        // `foo(  \t  )` also trims to empty.
        let p2 = parse("foo(  \t  )").unwrap();
        assert_eq!(
            p2,
            Predicate::Custom {
                name: "foo".into(),
                arg: "".into()
            }
        );
        // Leading/trailing whitespace is stripped from the argument.
        let p3 = parse("foo(  hello  )").unwrap();
        assert_eq!(
            p3,
            Predicate::Custom {
                name: "foo".into(),
                arg: "hello".into()
            }
        );
    }

    // --- Disk cache integration tests ---

    fn ctx_with_disk_cache<'a>(
        deps: &'a [PackageId],
        entries: std::collections::HashMap<String, ResolvedPredicateEntry>,
        cache_path: &Path,
    ) -> PredicateContext<'a> {
        PredicateContext::with_custom_predicates(deps, entries).with_disk_cache(cache_path)
    }

    fn entries_for(
        name: &str,
        script_path: &Path,
    ) -> std::collections::HashMap<String, ResolvedPredicateEntry> {
        let mut map = std::collections::HashMap::new();
        map.insert(
            name.to_string(),
            ResolvedPredicateEntry {
                runnable: symposium_install::Runnable::Script(script_path.to_path_buf()),
                args: vec![],
            },
        );
        map
    }

    #[test]
    fn disk_cache_hit_avoids_second_spawn() {
        use std::io::Write;
        // Script writes one line to counter_path on every run and emits no
        // events → cached indefinitely.
        let counter = tempfile::NamedTempFile::new().unwrap();
        let counter_path = counter.path().to_path_buf();
        let script = tempfile::Builder::new().suffix(".sh").tempfile().unwrap();
        writeln!(
            script.as_file(),
            "#!/bin/sh\necho ran >> {}\nexit 0",
            counter_path.display()
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let cache_path = crate::predicate_cache::PredicateCache::path_for_workspace(
            dir.path(),
            workspace.path(),
        );

        // First eval: fresh context populates the disk cache.
        {
            let mut ctx =
                ctx_with_disk_cache(&[], entries_for("always", script.path()), &cache_path);
            let pred = Predicate::Custom {
                name: "always".into(),
                arg: "x".into(),
            };
            assert!(pred.evaluate(&mut ctx));
            ctx.persist_disk_cache(&cache_path).unwrap();
        }

        // Second eval: brand-new context (no in-memory carryover) reads the
        // disk cache and must not spawn the script again.
        {
            let mut ctx =
                ctx_with_disk_cache(&[], entries_for("always", script.path()), &cache_path);
            let pred = Predicate::Custom {
                name: "always".into(),
                arg: "x".into(),
            };
            assert!(pred.evaluate(&mut ctx));
        }

        let runs = std::fs::read_to_string(&counter_path).unwrap();
        assert_eq!(runs.lines().count(), 1, "predicate should spawn once");
    }

    #[test]
    fn disk_cache_time_expired_entry_respawns() {
        use std::io::Write;
        // Script emits WatchTime(1) so any subsequent read after 1ms wall
        // clock is stale and must respawn.
        let counter = tempfile::NamedTempFile::new().unwrap();
        let counter_path = counter.path().to_path_buf();
        let script = tempfile::Builder::new().suffix(".sh").tempfile().unwrap();
        writeln!(
            script.as_file(),
            "#!/bin/sh\necho ran >> {}\nprintf '{{\"watchTime\":1}}\\n'\nexit 0",
            counter_path.display()
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let cache_path = crate::predicate_cache::PredicateCache::path_for_workspace(
            dir.path(),
            workspace.path(),
        );

        // First eval populates the cache with a 1ms TTL.
        {
            let mut ctx =
                ctx_with_disk_cache(&[], entries_for("ticking", script.path()), &cache_path);
            let pred = Predicate::Custom {
                name: "ticking".into(),
                arg: "x".into(),
            };
            assert!(pred.evaluate(&mut ctx));
            ctx.persist_disk_cache(&cache_path).unwrap();
        }

        std::thread::sleep(std::time::Duration::from_millis(50));

        // Second eval: entry has expired, script must spawn again.
        {
            let mut ctx =
                ctx_with_disk_cache(&[], entries_for("ticking", script.path()), &cache_path);
            let pred = Predicate::Custom {
                name: "ticking".into(),
                arg: "x".into(),
            };
            assert!(pred.evaluate(&mut ctx));
        }

        let runs = std::fs::read_to_string(&counter_path).unwrap();
        assert_eq!(runs.lines().count(), 2, "predicate should spawn twice");
    }

    // --- Predicate event parser tests ---

    #[test]
    fn parse_predicate_events_skips_blank_and_unknown_lines() {
        let stdout = concat!(
            "\n",
            r#"{"watchFile":"a"}"#,
            "\n",
            r#"{"watchFuture":42}"#,
            "\n",
            "   \n",
            r#"{"watchTime":0}"#,
            "\n",
        );
        let events = parse_predicate_events("foo", stdout.as_bytes());
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], CustomPredicateEvent::WatchFile(_)));
        assert!(matches!(&events[1], CustomPredicateEvent::WatchTime(0)));
    }

    #[test]
    fn parse_predicate_events_invalid_utf8_returns_empty() {
        let events = parse_predicate_events("foo", &[0xFF, 0xFE, 0xFD]);
        assert!(events.is_empty());
    }
}
