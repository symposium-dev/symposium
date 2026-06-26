//! Rust crate source fetching and management.
//!
//! Registry crate fetching delegates to `cargo fetch` via a dummy temporary
//! package (see [`probe`]) rather than hitting `crates.io` HTTP endpoints
//! directly. Local path dependencies short-circuit through the workspace's
//! path overrides and never touch the registry.

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use symposium_install::UpdateLevel;
use symposium_sdk::workspace::{WorkspaceCrate, WorkspaceDeps};

use crate::config::{CargoDependencySpec, CrateSourceSpec, PluginsEntrySource, UsedSourceConfig, Symposium};

mod list;
mod probe;

pub use list::crate_pairs;

/// Registry namespace that produced a source root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRegistry {
    Path,
    Git,
    Crate,
}

/// A plugin source declaration before registry resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum RegistrySourceSpec {
    /// Direct path registry source.
    Path(PathBuf),
    /// Direct git registry source.
    Git(String),
    /// Cargo crate-registry source.
    Crate(CrateSourceSpec),
}

/// Concrete source root produced by a registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSourceRoot {
    pub registry: SourceRegistry,
    pub source_id: String,
    pub path: PathBuf,
}

/// Non-exclusive provenance flags for why a source is in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceProvenance {
    Used,
    Workspace,
    Dependency,
}

/// Human-readable reason a source was reached.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceReason {
    pub provenance: SourceProvenance,
    pub detail: String,
}

/// One deduplicated source node in the resolved graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSourceNode {
    pub root: ResolvedSourceRoot,
    pub provenance: BTreeSet<SourceProvenance>,
    pub reasons: BTreeSet<SourceReason>,
}

/// Resolved plugin source graph before discovery mutates agent directories.
#[derive(Debug, Clone, Default)]
pub struct ResolvedSourceGraph {
    nodes: Vec<ResolvedSourceNode>,
    /// Config entries deferred because they reference custom predicates not
    /// yet defined. Drained by `expand_source_graph`.
    deferred_config_entries: Vec<DeferredSource>,
}

impl ResolvedSourceGraph {
    /// Build the initial source graph for sync and status: installed sources
    /// plus workspace root/members when agents-syncing is enabled.
    ///
    /// The workspace root is only added when it has an explicit `SYMPOSIUM.toml`.
    /// Without one, the synthesized manifest's recursive `[[plugins]]` search
    /// would scan the entire project tree.
    pub async fn build_initial(sym: &Symposium, deps: &mut WorkspaceDeps) -> Self {
        let resolver = SourceRegistryResolver::new(sym);
        let mut graph = Self::default();

        // Evaluate each [[plugins]] entry's predicates before adding its sources.
        // Entries with unknown custom predicates are deferred for the expand loop.
        let cwd = deps.cwd().to_path_buf();
        let crate_list: Vec<(String, semver::Version)> = Vec::new();
        let known_customs: std::collections::HashSet<String> = std::collections::HashSet::new();

        for entry in &sym.config.plugins {
            if !crate::predicate::all_predicates_known(&entry.predicates, &known_customs) {
                // Defer: contains custom predicates not yet defined.
                // These will be retried by expand_source_graph.
                for spec in entry_source_specs(&entry.source) {
                    graph.deferred_config_entries.push(DeferredSource {
                        spec,
                        provenance: BTreeSet::from([SourceProvenance::Used]),
                        detail: "used config (deferred)".to_string(),
                        predicates: entry.predicates.clone(),
                        origin: "config.toml".to_string(),
                    });
                }
                continue;
            }

            let mut pred_ctx = crate::predicate::PredicateContext::with_cwd(&crate_list, &cwd);
            pred_ctx.set_source_provenance(BTreeSet::from([SourceProvenance::Used]));

            if !entry.predicates.evaluate(&mut pred_ctx) {
                tracing::debug!(
                    predicates = ?entry.predicates,
                    "skipping [[plugins]] entry: predicates not satisfied"
                );
                continue;
            }
            for spec in entry_source_specs(&entry.source) {
                match resolver.resolve(&spec).await {
                    Ok(root) => {
                        graph.add_root(
                            root,
                            SourceReason {
                                provenance: SourceProvenance::Used,
                                detail: "used config".to_string(),
                            },
                        );
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to resolve used source, skipping");
                    }
                }
            }
        }

        if sym.config.agents_syncing
            && let Some(loaded) = deps.load()
        {
            let root_path =
                std::fs::canonicalize(&loaded.root).unwrap_or_else(|_| loaded.root.clone());
            if root_path.join("SYMPOSIUM.toml").is_file() {
                graph.add_root(
                    ResolvedSourceRoot {
                        registry: SourceRegistry::Path,
                        source_id: format!("workspace:{}", root_path.display()),
                        path: root_path.clone(),
                    },
                    SourceReason {
                        provenance: SourceProvenance::Workspace,
                        detail: "workspace root".to_string(),
                    },
                );
            }

            for member in &loaded.members {
                let Some(path) = member.path.as_ref() else {
                    continue;
                };
                let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
                if path == root_path {
                    continue;
                }
                graph.add_root(
                    ResolvedSourceRoot {
                        registry: SourceRegistry::Path,
                        source_id: format!("workspace-member:{}@{}", member.name, member.version),
                        path,
                    },
                    SourceReason {
                        provenance: SourceProvenance::Workspace,
                        detail: format!("workspace member {}", member.name),
                    },
                );
            }
        }

        graph
    }

    pub async fn resolve_used_and_workspace(
        sym: &Symposium,
        workspace: &mut WorkspaceDeps,
    ) -> Result<Self> {
        let resolver = SourceRegistryResolver::new(sym);
        let mut graph = ResolvedSourceGraph::default();

        for spec in used_source_specs(sym.used_sources()) {
            match resolver.resolve(&spec).await {
                Ok(root) => {
                    graph.add_root(
                        root,
                        SourceReason {
                            provenance: SourceProvenance::Used,
                            detail: "used config".to_string(),
                        },
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to resolve used source, skipping");
                }
            }
        }

        if let Some(loaded) = workspace.load() {
            let root_path =
                std::fs::canonicalize(&loaded.root).unwrap_or_else(|_| loaded.root.clone());
            graph.add_root(
                ResolvedSourceRoot {
                    registry: SourceRegistry::Path,
                    source_id: format!("workspace:{}", root_path.display()),
                    path: root_path,
                },
                SourceReason {
                    provenance: SourceProvenance::Workspace,
                    detail: "workspace root".to_string(),
                },
            );

            for member in &loaded.members {
                let Some(path) = member.path.as_ref() else {
                    continue;
                };
                let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
                graph.add_root(
                    ResolvedSourceRoot {
                        registry: SourceRegistry::Path,
                        source_id: format!("workspace-member:{}@{}", member.name, member.version),
                        path,
                    },
                    SourceReason {
                        provenance: SourceProvenance::Workspace,
                        detail: format!("workspace member {}", member.name),
                    },
                );
            }
        }

        Ok(graph)
    }

    pub fn nodes(&self) -> &[ResolvedSourceNode] {
        &self.nodes
    }

    pub fn add_resolved_root(&mut self, root: ResolvedSourceRoot, reason: SourceReason) {
        self.add_root(root, reason);
    }

    /// Returns true if the given canonical path is already a node in the graph.
    pub fn contains_path(&self, path: &std::path::Path) -> bool {
        self.nodes.iter().any(|node| node.root.path == path)
    }

    /// Number of source nodes in the graph.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// True if the graph has no source nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Add a source with a full provenance set. Returns `true` if provenance
    /// grew on an existing node (i.e., at least one new provenance flag was
    /// added that wasn't there before).
    pub(crate) fn add_root_with_provenance(
        &mut self,
        root: &ResolvedSourceRoot,
        provenance: &BTreeSet<SourceProvenance>,
        detail: &str,
    ) -> bool {
        if let Some(existing) = self
            .nodes
            .iter_mut()
            .find(|node| node.root.path == root.path)
        {
            let before = existing.provenance.len();
            for &prov in provenance {
                existing.provenance.insert(prov);
                existing.reasons.insert(SourceReason {
                    provenance: prov,
                    detail: detail.to_string(),
                });
            }
            return existing.provenance.len() > before;
        }

        let reasons = provenance
            .iter()
            .map(|&prov| SourceReason {
                provenance: prov,
                detail: detail.to_string(),
            })
            .collect();
        self.nodes.push(ResolvedSourceNode {
            root: root.clone(),
            provenance: provenance.clone(),
            reasons,
        });
        false
    }

    /// Get the current provenance set for a path, or empty if not in graph.
    pub(crate) fn provenance_for(&self, path: &std::path::Path) -> BTreeSet<SourceProvenance> {
        self.nodes
            .iter()
            .find(|node| node.root.path == path)
            .map(|node| node.provenance.clone())
            .unwrap_or_default()
    }

    fn add_root(&mut self, root: ResolvedSourceRoot, reason: SourceReason) {
        if let Some(existing) = self
            .nodes
            .iter_mut()
            .find(|node| node.root.path == root.path)
        {
            existing.provenance.insert(reason.provenance);
            existing.reasons.insert(reason);
            return;
        }

        let mut provenance = BTreeSet::new();
        provenance.insert(reason.provenance);
        let mut reasons = BTreeSet::new();
        reasons.insert(reason);
        self.nodes.push(ResolvedSourceNode {
            root,
            provenance,
            reasons,
        });
    }
}

/// A worklist item: a source to resolve and add to the graph.
struct WorklistItem {
    spec: RegistrySourceSpec,
    provenance: BTreeSet<SourceProvenance>,
    detail: String,
}

/// A source declaration deferred because its predicates reference unknown
/// custom predicates. Will be retried when the defining plugin is loaded.
#[derive(Debug, Clone)]
struct DeferredSource {
    spec: RegistrySourceSpec,
    provenance: BTreeSet<SourceProvenance>,
    detail: String,
    predicates: crate::predicate::PredicateSet,
    origin: String,
}

/// Info needed to resolve a custom predicate at retry time.
#[derive(Debug, Clone)]
struct CustomPredicateInfo {
    /// The resolved runnable for this predicate.
    runnable: symposium_install::Runnable,
    /// Static args to pass before the dynamic arg.
    args: Vec<String>,
}

/// Try to resolve a custom predicate's installation from a parsed plugin.
fn resolve_custom_pred_info(
    parsed: &crate::plugins::ParsedPlugin,
    cp: &crate::plugins::CustomPredicate,
) -> Option<CustomPredicateInfo> {
    let install = parsed.plugin.get_installation(&cp.command)?;
    let source_dir = &parsed.source_dir;

    // Resolve the script or executable path relative to the source dir.
    if let Some(script) = &install.script {
        let script_path = if std::path::Path::new(script).is_absolute() {
            PathBuf::from(script)
        } else {
            source_dir.join(script)
        };
        Some(CustomPredicateInfo {
            runnable: symposium_install::Runnable::Script(script_path),
            args: cp.args.clone(),
        })
    } else if let Some(executable) = &install.executable {
        let exec_path = if std::path::Path::new(executable).is_absolute() {
            PathBuf::from(executable)
        } else {
            source_dir.join(executable)
        };
        Some(CustomPredicateInfo {
            runnable: symposium_install::Runnable::Exec(exec_path),
            args: cp.args.clone(),
        })
    } else {
        None
    }
}

/// Expand a source graph using a worklist algorithm with deferred retry.
///
/// Seeds the worklist from each existing node's `[[plugins]] source.*`
/// declarations and from discovery-allowed workspace dependency candidates.
/// Processes items one at a time: resolve → add to graph → scan for plugins
/// → push new work. When a plugin source declaration is gated on a custom
/// predicate not yet defined, it is deferred and retried after the worklist
/// drains (when newly-discovered custom predicates may unblock it).
///
/// When a resolved path is already in the graph but with less provenance,
/// the new provenance is unioned in and the node's children are re-pushed
/// so provenance propagates through recursive edges.
///
/// Returns the number of new sources added.
pub async fn expand_source_graph(
    graph: &mut ResolvedSourceGraph,
    sym: &Symposium,
    workspace_crates: &[symposium_sdk::workspace::WorkspaceCrate],
) -> usize {
    use crate::discovery::CollectedPolicy;
    use crate::plugins::scan_source_dir_public;

    let resolver = SourceRegistryResolver::new(sym);
    let initial_count = graph.len();

    // Collect discovery policy from user config.
    let mut policy = CollectedPolicy::default();
    policy.add_policy(&sym.config.discovery);

    // Track custom predicate names and their resolution info as discovered.
    let mut known_custom_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut custom_pred_info: std::collections::HashMap<String, CustomPredicateInfo> =
        std::collections::HashMap::new();

    // Sources deferred because they reference unknown custom predicates.
    // Seed from any config entries deferred during build_initial.
    let mut deferred: Vec<DeferredSource> = std::mem::take(&mut graph.deferred_config_entries);

    // Track which (path, provenance) combinations we've already scanned so
    // we don't re-process a node unless new provenance arrives.
    let mut scanned: std::collections::BTreeMap<PathBuf, BTreeSet<SourceProvenance>> =
        std::collections::BTreeMap::new();

    // Seed: scan existing graph nodes (installed sources, workspace, legacy)
    // and collect their contributions to the worklist and policy.
    let mut worklist: Vec<WorklistItem> = Vec::new();
    seed_from_nodes(
        graph,
        &mut policy,
        &mut worklist,
        &mut scanned,
        workspace_crates,
        &mut known_custom_names,
        &mut custom_pred_info,
        &mut deferred,
    );

    // Seed: evaluate workspace dep candidates against initial policy.
    push_allowed_deps(&policy, workspace_crates, &mut worklist, &scanned);

    // Process worklist with deferred-retry loop.
    let mut safety_limit = 1000usize;
    loop {
        // Drain the worklist.
        while let Some(item) = worklist.pop() {
            safety_limit = safety_limit.saturating_sub(1);
            if safety_limit == 0 {
                tracing::warn!("expand_source_graph hit safety limit, stopping");
                return graph.len() - initial_count;
            }

            // Resolve the spec to a concrete path.
            let root = match resolver.resolve(&item.spec).await {
                Ok(root) => root,
                Err(e) => {
                    tracing::debug!(spec = ?item.spec, error = %e, "failed to resolve source");
                    continue;
                }
            };

            // For dependency candidates, only add if it has plugin content.
            if item.provenance.contains(&SourceProvenance::Dependency)
                && item.provenance.len() == 1
                && !root.path.join("SYMPOSIUM.toml").is_file()
                && !root.path.join("skills").is_dir()
            {
                continue;
            }

            // Add to graph (unions provenance if already present).
            let provenance_grew =
                graph.add_root_with_provenance(&root, &item.provenance, &item.detail);

            // Check if we've already scanned this path with at least this provenance.
            let dominated = scanned
                .get(&root.path)
                .is_some_and(|prev| item.provenance.is_subset(prev));
            if dominated && !provenance_grew {
                continue;
            }

            // Record what we're about to scan.
            scanned
                .entry(root.path.clone())
                .or_default()
                .extend(item.provenance.iter().copied());

            // Scan the source for plugins.
            let source_name = &root.source_id;
            let dir = &root.path;
            let node_provenance = graph.provenance_for(&root.path);
            let Ok(contents) =
                scan_source_dir_public(dir, source_name, workspace_crates, &node_provenance)
            else {
                continue;
            };

            let policy_grew_before = policy.rule_count();

            for result in contents {
                let Ok(parsed) = result else { continue };

                // Collect custom predicate definitions from this plugin.
                for cp in &parsed.plugin.custom_predicates {
                    known_custom_names.insert(cp.name.clone());
                    if let Some(info) = resolve_custom_pred_info(&parsed, cp) {
                        custom_pred_info.insert(cp.name.clone(), info);
                    }
                }

                // Contribute discovery policy.
                policy.add_policy(&parsed.plugin.discovery);

                // Push recursive [[plugins]] source.* declarations.
                for ps in &parsed.plugin.plugin_sources {
                    if !try_push_plugin_source(
                        ps,
                        &node_provenance,
                        workspace_crates,
                        &known_custom_names,
                        source_name,
                        &mut worklist,
                        &mut deferred,
                    ) {
                        continue;
                    }
                }
            }

            // If new policy rules were added, re-check workspace dep candidates.
            if policy.rule_count() > policy_grew_before {
                push_allowed_deps(&policy, workspace_crates, &mut worklist, &scanned);
            }
        }

        // Worklist is empty. Retry deferred entries whose predicates are now known.
        if deferred.is_empty() {
            break;
        }

        let mut progress = false;
        let prev_deferred = std::mem::take(&mut deferred);
        let crate_pairs = crate_pairs(workspace_crates);

        // Build custom predicate entries for evaluation.
        let custom_entries: std::collections::HashMap<String, crate::predicate::ResolvedPredicateEntry> =
            custom_pred_info.iter().map(|(name, info)| {
                (name.clone(), crate::predicate::ResolvedPredicateEntry {
                    runnable: info.runnable.clone(),
                    args: info.args.clone(),
                })
            }).collect();

        for d in prev_deferred {
            if crate::predicate::all_predicates_known(&d.predicates, &known_custom_names) {
                // Predicates are now fully resolvable — evaluate them.
                let mut ctx = crate::predicate::PredicateContext::with_custom_predicates(
                    &crate_pairs, custom_entries.clone()
                );
                ctx.set_source_provenance(d.provenance.clone());
                if d.predicates.evaluate(&mut ctx) {
                    worklist.push(WorklistItem {
                        spec: d.spec,
                        provenance: d.provenance,
                        detail: d.detail,
                    });
                    progress = true;
                }
                // else: predicate evaluated to false — legitimately skip
            } else {
                // Still unknown — keep deferred
                deferred.push(d);
            }
        }

        if !progress {
            // Stuck: remaining entries reference undefined predicates.
            for d in &deferred {
                tracing::warn!(
                    origin = %d.origin,
                    predicates = ?d.predicates,
                    "plugin source deferred: custom predicate(s) never defined"
                );
            }
            break;
        }
    }

    graph.len() - initial_count
}

/// Seed the worklist by scanning existing graph nodes for recursive source
/// declarations and discovery policy.
fn seed_from_nodes(
    graph: &ResolvedSourceGraph,
    policy: &mut crate::discovery::CollectedPolicy,
    worklist: &mut Vec<WorklistItem>,
    scanned: &mut std::collections::BTreeMap<PathBuf, BTreeSet<SourceProvenance>>,
    workspace_crates: &[symposium_sdk::workspace::WorkspaceCrate],
    known_custom_names: &mut std::collections::HashSet<String>,
    custom_pred_info: &mut std::collections::HashMap<String, CustomPredicateInfo>,
    deferred: &mut Vec<DeferredSource>,
) {
    use crate::plugins::scan_source_dir_public;

    for node in graph.nodes() {
        let source_name = &node.root.source_id;
        let dir = &node.root.path;

        scanned
            .entry(dir.clone())
            .or_default()
            .extend(node.provenance.iter().copied());

        let Ok(contents) =
            scan_source_dir_public(dir, source_name, workspace_crates, &node.provenance)
        else {
            continue;
        };
        for result in contents {
            let Ok(parsed) = result else { continue };

            // Collect custom predicate definitions.
            for cp in &parsed.plugin.custom_predicates {
                known_custom_names.insert(cp.name.clone());
                if let Some(info) = resolve_custom_pred_info(&parsed, cp) {
                    custom_pred_info.insert(cp.name.clone(), info);
                }
            }

            policy.add_policy(&parsed.plugin.discovery);
            for ps in &parsed.plugin.plugin_sources {
                try_push_plugin_source(
                    ps,
                    &node.provenance,
                    workspace_crates,
                    known_custom_names,
                    source_name,
                    worklist,
                    deferred,
                );
            }
        }
    }
}

/// Attempt to push a plugin source declaration onto the worklist.
/// If the predicates reference unknown custom predicates, defers the source instead.
/// Returns true if the source was pushed (or deferred), false if predicates failed.
fn try_push_plugin_source(
    ps: &crate::plugins::PluginSearchSource,
    provenance: &BTreeSet<SourceProvenance>,
    workspace_crates: &[symposium_sdk::workspace::WorkspaceCrate],
    known_custom_names: &std::collections::HashSet<String>,
    source_name: &str,
    worklist: &mut Vec<WorklistItem>,
    deferred: &mut Vec<DeferredSource>,
) -> bool {
    use crate::plugins::PluginSourceDecl;

    // Convert the source declaration to specs (only git/crate go through worklist).
    let specs: Vec<RegistrySourceSpec> = match &ps.source {
        PluginSourceDecl::Git(url) => vec![RegistrySourceSpec::Git(url.clone())],
        PluginSourceDecl::Crate(crate_specs) => crate_specs
            .iter()
            .map(|s| RegistrySourceSpec::Crate(s.clone()))
            .collect(),
        PluginSourceDecl::Path(_) => return true, // path sources resolved inline
    };

    if ps.predicates.is_empty() {
        // No predicates — push directly.
        for spec in specs {
            worklist.push(WorklistItem {
                spec,
                provenance: provenance.clone(),
                detail: format!("recursive from {source_name}"),
            });
        }
        return true;
    }

    // Check if all predicates are resolvable with currently known customs.
    if !crate::predicate::all_predicates_known(&ps.predicates, known_custom_names) {
        // Defer: we can't evaluate yet.
        for spec in specs {
            deferred.push(DeferredSource {
                spec,
                provenance: provenance.clone(),
                detail: format!("recursive from {source_name}"),
                predicates: ps.predicates.clone(),
                origin: source_name.to_string(),
            });
        }
        return true;
    }

    // All predicates known — evaluate them.
    let crate_pairs = crate_pairs(workspace_crates);
    let mut ctx = crate::predicate::PredicateContext::new(&crate_pairs);
    ctx.set_source_provenance(provenance.clone());
    if !ps.predicates.evaluate(&mut ctx) {
        return false;
    }

    for spec in specs {
        worklist.push(WorklistItem {
            spec,
            provenance: provenance.clone(),
            detail: format!("recursive from {source_name}"),
        });
    }
    true
}

/// Push workspace dependency candidates that are allowed by policy but not
/// yet scanned with `Dependency` provenance.
fn push_allowed_deps(
    policy: &crate::discovery::CollectedPolicy,
    workspace_crates: &[symposium_sdk::workspace::WorkspaceCrate],
    worklist: &mut Vec<WorklistItem>,
    scanned: &std::collections::BTreeMap<PathBuf, BTreeSet<SourceProvenance>>,
) {
    use crate::discovery::{DiscoveryCandidate, PolicyVerdict};

    if !policy.has_any_allow_rules() {
        return;
    }

    for wc in workspace_crates {
        let candidate = DiscoveryCandidate::Crate {
            name: wc.name.clone(),
            version: wc.version.to_string(),
        };
        if policy.evaluate(&candidate) != PolicyVerdict::Allowed {
            continue;
        }

        // Check if we've already scanned this dep's path with Dependency provenance.
        if let Some(path) = &wc.path {
            let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            if scanned
                .get(&canonical)
                .is_some_and(|prev| prev.contains(&SourceProvenance::Dependency))
            {
                continue;
            }
        }

        let dep_provenance = BTreeSet::from([SourceProvenance::Dependency]);
        let spec = if let Some(path) = &wc.path {
            RegistrySourceSpec::Path(path.clone())
        } else {
            RegistrySourceSpec::Crate(CrateSourceSpec {
                key: Some(wc.name.clone()),
                dependency: CargoDependencySpec::Version(format!("={}", wc.version)),
            })
        };
        worklist.push(WorklistItem {
            spec,
            provenance: dep_provenance,
            detail: format!("discovery allowed: {}", wc.name),
        });
    }
}

/// Resolver for installed or transitive plugin source declarations.
pub struct SourceRegistryResolver<'a> {
    sym: &'a Symposium,
    update: UpdateLevel,
}

impl<'a> SourceRegistryResolver<'a> {
    pub fn new(sym: &'a Symposium) -> Self {
        Self {
            sym,
            update: UpdateLevel::None,
        }
    }

    pub fn update(mut self, update: UpdateLevel) -> Self {
        self.update = update;
        self
    }

    pub async fn resolve(&self, spec: &RegistrySourceSpec) -> Result<ResolvedSourceRoot> {
        match spec {
            RegistrySourceSpec::Path(path) => self.resolve_path(path),
            RegistrySourceSpec::Git(url) => self.resolve_git(url).await,
            RegistrySourceSpec::Crate(spec) => self.resolve_crate(spec).await,
        }
    }

    pub async fn resolve_installed_sources(&self) -> Result<Vec<ResolvedSourceRoot>> {
        let mut roots = Vec::new();
        for spec in used_source_specs(self.sym.used_sources()) {
            roots.push(self.resolve(&spec).await?);
        }
        Ok(roots)
    }

    fn resolve_path(&self, path: &std::path::Path) -> Result<ResolvedSourceRoot> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.sym.config_dir().join(path)
        };
        let canonical = std::fs::canonicalize(&path)
            .with_context(|| format!("failed to resolve path source {}", path.display()))?;
        if !canonical.is_dir() {
            bail!("path source {} is not a directory", canonical.display());
        }
        Ok(ResolvedSourceRoot {
            registry: SourceRegistry::Path,
            source_id: format!("path:{}", canonical.display()),
            path: canonical,
        })
    }

    async fn resolve_git(&self, url: &str) -> Result<ResolvedSourceRoot> {
        let cache_mgr = symposium_install::git::GitCacheManager::new(
            &self.sym.install_context(),
            "plugin-sources",
        );
        let path = cache_mgr.fetch_url(url, self.update).await?;
        Ok(ResolvedSourceRoot {
            registry: SourceRegistry::Git,
            source_id: format!("git:{url}"),
            path,
        })
    }

    async fn resolve_crate(&self, spec: &CrateSourceSpec) -> Result<ResolvedSourceRoot> {
        let result =
            probe::fetch_dependency_via_cargo(spec.key.as_deref(), &spec.dependency).await?;
        let path = std::fs::canonicalize(&result.path).unwrap_or(result.path);
        Ok(ResolvedSourceRoot {
            registry: SourceRegistry::Crate,
            source_id: format!("crate:{}@{}", result.name, result.version),
            path,
        })
    }
}

/// Normalize used source config into registry source specs.
pub fn used_source_specs(used: &UsedSourceConfig) -> Vec<RegistrySourceSpec> {
    let mut specs = Vec::new();
    specs.extend(used.crates.iter().map(|(name, dependency)| {
        RegistrySourceSpec::Crate(CrateSourceSpec {
            key: Some(name.clone()),
            dependency: dependency.clone(),
        })
    }));
    specs.extend(
        used.paths
            .iter()
            .map(|path| RegistrySourceSpec::Path(PathBuf::from(path))),
    );
    specs.extend(
        used.git
            .iter()
            .map(|url| RegistrySourceSpec::Git(url.clone())),
    );
    specs
}

/// Convert a single `PluginsEntrySource` into registry source specs.
pub fn entry_source_specs(source: &PluginsEntrySource) -> Vec<RegistrySourceSpec> {
    let mut specs = Vec::new();
    specs.extend(source.crates.iter().map(|(name, dependency)| {
        RegistrySourceSpec::Crate(CrateSourceSpec {
            key: Some(name.clone()),
            dependency: dependency.clone(),
        })
    }));
    specs.extend(
        source
            .paths
            .iter()
            .map(|path| RegistrySourceSpec::Path(PathBuf::from(path))),
    );
    specs.extend(
        source.git.iter().map(|url| RegistrySourceSpec::Git(url.clone())),
    );
    specs
}

/// Render Cargo dependency-table entries for crate-registry source specs.
pub fn crate_dependency_table_toml<'a>(
    specs: impl IntoIterator<Item = (&'a str, &'a CargoDependencySpec)>,
) -> Result<String> {
    probe::dependency_table_toml(specs)
}

/// Normalize a crate name for hyphen/underscore-insensitive comparison.
///
/// Cargo treats `foo-bar` and `foo_bar` as the same crate name (published
/// name on crates.io vs. Rust module identifier), so any name-equality check
/// against a user-supplied query should go through this normalization.
pub(crate) fn normalize_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

/// Result of fetching a crate's sources.
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// The canonical crate name (e.g. `serde_json` even if queried as `serde-json`).
    pub name: String,
    /// The exact version that was fetched.
    pub version: String,
    /// Path to the crate sources on disk.
    pub path: PathBuf,
}

/// Builder for accessing Rust crate source code.
pub struct RustCrateFetch<'a> {
    crate_name: String,
    version_spec: Option<String>,
    workspace: &'a [WorkspaceCrate],
}

impl<'a> RustCrateFetch<'a> {
    /// Create a new fetch request for the given crate name.
    pub fn new(name: &str, workspace: &'a [WorkspaceCrate]) -> Self {
        Self {
            crate_name: name.to_string(),
            version_spec: None,
            workspace,
        }
    }

    /// Specify a version constraint (e.g. `"^1.0"`, `"=1.2.3"`).
    pub fn version(mut self, version: &str) -> Self {
        self.version_spec = Some(version.to_string());
        self
    }

    /// Fetch the crate sources, returning a path to the source directory.
    ///
    /// Resolution order:
    /// 1. If the crate is a local path dependency in the workspace (and no
    ///    explicit `--version` was requested), return the path directly.
    /// 2. Otherwise, run `cargo fetch` in a temporary dummy package to
    ///    populate cargo's registry cache, then read `cargo metadata` to get
    ///    the extracted source path under `~/.cargo/registry/src/`.
    pub async fn fetch(self) -> Result<FetchResult> {
        // Check path overrides first (local path dependencies).
        if self.version_spec.is_none() {
            let normalized = normalize_crate_name(&self.crate_name);
            if let Some(wc) = self
                .workspace
                .iter()
                .find(|wc| wc.path.is_some() && normalize_crate_name(&wc.name) == normalized)
            {
                let path = wc.path.as_ref().unwrap();
                tracing::debug!(crate_name = %wc.name, path = %path.display(), "resolved from path override");
                return Ok(FetchResult {
                    name: wc.name.clone(),
                    version: wc.version.to_string(),
                    path: path.clone(),
                });
            }
        }

        let (name, version_req) = self.resolve_registry_spec();
        probe::fetch_via_cargo(&name, &version_req).await
    }

    /// Choose the `(name, version_req)` pair to put in the probe package's
    /// dependency entry when going through the registry.
    ///
    /// Precedence:
    /// 1. Explicit `--version` constraint from the caller.
    /// 2. If the crate is a direct dependency of the current workspace, pin
    ///    to that exact resolved version (`=x.y.z`).
    /// 3. Otherwise, `"*"` — cargo picks the latest compatible version.
    fn resolve_registry_spec(&self) -> (String, String) {
        if let Some(spec) = &self.version_spec {
            return (self.crate_name.clone(), spec.clone());
        }

        let normalized = normalize_crate_name(&self.crate_name);
        if let Some(wc) = self
            .workspace
            .iter()
            .find(|wc| normalize_crate_name(&wc.name) == normalized)
        {
            return (wc.name.clone(), format!("={}", wc.version));
        }

        (self.crate_name.clone(), "*".to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    fn wc(name: &str, version: &str, path: Option<PathBuf>) -> WorkspaceCrate {
        WorkspaceCrate::new(name.to_string(), v(version), path)
    }

    fn spec_table(fields: &[(&str, toml::Value)]) -> CargoDependencySpec {
        CargoDependencySpec::Table(
            fields
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    fn write_minimal_crate(dir: &std::path::Path, name: &str, version: &str) {
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                r#"[package]
name = "{name}"
version = "{version}"
edition = "2021"
"#
            ),
        )
        .unwrap();
    }

    fn write_virtual_workspace(root: &std::path::Path, members: &[(&str, &str)]) {
        let member_names = members
            .iter()
            .map(|(dir, _)| format!("\"{dir}\""))
            .collect::<Vec<_>>()
            .join(", ");
        std::fs::write(
            root.join("Cargo.toml"),
            format!(
                r#"[workspace]
members = [{member_names}]
resolver = "2"
"#
            ),
        )
        .unwrap();
        for (dir, package) in members {
            write_minimal_crate(&root.join(dir), package, "0.1.0");
        }
    }

    // -- Registry dependency rendering ---------------------------------

    #[test]
    fn dependency_table_toml_renders_registry_git_and_path_specs() {
        let registry = CargoDependencySpec::Version("1".to_string());
        let git = spec_table(&[
            (
                "git",
                toml::Value::String("https://github.com/me/plugin".to_string()),
            ),
            ("branch", toml::Value::String("main".to_string())),
        ]);
        let path = spec_table(&[
            ("path", toml::Value::String("../plugin".to_string())),
            ("package", toml::Value::String("actual-plugin".to_string())),
        ]);

        let rendered =
            crate_dependency_table_toml([("foo", &registry), ("bar", &git), ("baz", &path)])
                .unwrap();

        assert_eq!(
            rendered,
            concat!(
                "bar = { branch = \"main\", git = \"https://github.com/me/plugin\" }\n",
                "baz = { package = \"actual-plugin\", path = \"../plugin\" }\n",
                "foo = { version = \"1\" }"
            )
        );
    }

    // -- Path override behaviour ---------------------------------------

    #[tokio::test]
    async fn fetch_uses_path_override_for_path_dep() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = vec![wc("my-crate", "0.1.0", Some(tmp.path().to_path_buf()))];

        let result = RustCrateFetch::new("my-crate", &workspace)
            .fetch()
            .await
            .unwrap();

        assert_eq!(result.name, "my-crate");
        assert_eq!(result.version, "0.1.0");
        assert_eq!(result.path, tmp.path());
    }

    #[tokio::test]
    async fn fetch_path_override_normalizes_hyphens() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = vec![wc("my_crate", "0.1.0", Some(tmp.path().to_path_buf()))];

        // Query with hyphen, workspace entry uses underscore.
        let result = RustCrateFetch::new("my-crate", &workspace)
            .fetch()
            .await
            .unwrap();

        assert_eq!(result.name, "my_crate");
        assert_eq!(result.path, tmp.path());
    }

    // -- Registry spec resolution (pure, no I/O) -----------------------

    #[test]
    fn registry_spec_prefers_explicit_version() {
        let workspace = vec![wc("foo", "1.0.0", None)];
        let fetch = RustCrateFetch::new("foo", &workspace).version("^2.0");
        let (name, req) = fetch.resolve_registry_spec();
        assert_eq!(name, "foo");
        assert_eq!(req, "^2.0");
    }

    #[test]
    fn registry_spec_pins_workspace_version_exactly() {
        let workspace = vec![wc("foo", "1.2.3", None)];
        let fetch = RustCrateFetch::new("foo", &workspace);
        let (name, req) = fetch.resolve_registry_spec();
        assert_eq!(name, "foo");
        assert_eq!(req, "=1.2.3");
    }

    #[test]
    fn registry_spec_normalizes_hyphens_against_workspace() {
        let workspace = vec![wc("serde_json", "1.0.0", None)];
        let fetch = RustCrateFetch::new("serde-json", &workspace);
        let (name, req) = fetch.resolve_registry_spec();
        // Canonical name from the workspace wins.
        assert_eq!(name, "serde_json");
        assert_eq!(req, "=1.0.0");
    }

    #[test]
    fn registry_spec_falls_back_to_wildcard() {
        let workspace: Vec<WorkspaceCrate> = Vec::new();
        let fetch = RustCrateFetch::new("foo", &workspace);
        let (name, req) = fetch.resolve_registry_spec();
        assert_eq!(name, "foo");
        assert_eq!(req, "*");
    }

    #[test]
    fn registry_spec_is_used_when_version_specified_even_with_path_dep() {
        // Explicit version → path override is skipped → registry spec uses
        // the explicit version.
        let tmp = tempfile::tempdir().unwrap();
        let workspace = vec![wc("serde", "1.0.210", Some(tmp.path().to_path_buf()))];
        let fetch = RustCrateFetch::new("serde", &workspace).version("=99.99.99");
        let (name, req) = fetch.resolve_registry_spec();
        assert_eq!(name, "serde");
        assert_eq!(req, "=99.99.99");
    }

    // -- Source registry resolver --------------------------------------

    #[tokio::test]
    async fn path_registry_resolves_local_directory_without_network() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("plugin-source");
        std::fs::create_dir(&source).unwrap();
        let sym = Symposium::from_dir(tmp.path());

        let resolver = SourceRegistryResolver::new(&sym);
        let resolved = resolver
            .resolve(&RegistrySourceSpec::Path(PathBuf::from("plugin-source")))
            .await
            .unwrap();

        assert_eq!(resolved.registry, SourceRegistry::Path);
        assert_eq!(resolved.path, std::fs::canonicalize(&source).unwrap());
        assert!(resolved.source_id.starts_with("path:"));
    }

    #[tokio::test]
    async fn crate_registry_resolves_unkeyed_local_path_crate_without_network() {
        let tmp = tempfile::tempdir().unwrap();
        let crate_dir = tmp.path().join("my-plugin-crate");
        write_minimal_crate(&crate_dir, "actual-plugin", "0.1.0");

        let sym = Symposium::from_dir(tmp.path());
        let spec = CrateSourceSpec {
            key: None,
            dependency: spec_table(&[(
                "path",
                toml::Value::String(crate_dir.to_string_lossy().to_string()),
            )]),
        };

        let resolver = SourceRegistryResolver::new(&sym);
        let resolved = resolver
            .resolve(&RegistrySourceSpec::Crate(spec))
            .await
            .unwrap();

        assert_eq!(resolved.registry, SourceRegistry::Crate);
        assert_eq!(resolved.source_id, "crate:actual-plugin@0.1.0");
        assert_eq!(resolved.path, std::fs::canonicalize(&crate_dir).unwrap());
    }

    #[tokio::test]
    async fn crate_registry_resolves_package_renamed_local_path_crate() {
        let tmp = tempfile::tempdir().unwrap();
        let crate_dir = tmp.path().join("actual-plugin");
        write_minimal_crate(&crate_dir, "actual-plugin", "0.2.0");

        let sym = Symposium::from_dir(tmp.path());
        let spec = CrateSourceSpec {
            key: Some("friendly-name".to_string()),
            dependency: spec_table(&[
                (
                    "path",
                    toml::Value::String(crate_dir.to_string_lossy().to_string()),
                ),
                ("package", toml::Value::String("actual-plugin".to_string())),
            ]),
        };

        let resolver = SourceRegistryResolver::new(&sym);
        let resolved = resolver
            .resolve(&RegistrySourceSpec::Crate(spec))
            .await
            .unwrap();

        assert_eq!(resolved.registry, SourceRegistry::Crate);
        assert_eq!(resolved.source_id, "crate:actual-plugin@0.2.0");
        assert_eq!(resolved.path, std::fs::canonicalize(&crate_dir).unwrap());
    }

    #[tokio::test]
    async fn resolver_resolves_installed_path_and_local_crate_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path();
        let direct_path = config_dir.join("direct-plugin-source");
        std::fs::create_dir(&direct_path).unwrap();

        let crate_dir = config_dir.join("crate-plugin-source");
        write_minimal_crate(&crate_dir, "crate-plugin-source", "0.3.0");

        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                r#"[used]
paths = ["direct-plugin-source"]

[used.crates]
crate-plugin-source = {{ path = "{}" }}
"#,
                crate_dir.display()
            ),
        )
        .unwrap();

        let sym = Symposium::from_dir(config_dir);
        let resolver = SourceRegistryResolver::new(&sym);
        let roots = resolver.resolve_installed_sources().await.unwrap();

        assert_eq!(roots.len(), 2);
        let canonical_crate_dir = std::fs::canonicalize(&crate_dir).unwrap();
        assert!(roots.iter().any(|root| {
            root.registry == SourceRegistry::Crate
                && root.source_id == "crate:crate-plugin-source@0.3.0"
                && root.path == canonical_crate_dir
        }));
        assert!(roots.iter().any(|root| {
            root.registry == SourceRegistry::Path
                && root.path == std::fs::canonicalize(&direct_path).unwrap()
        }));
    }

    #[tokio::test]
    async fn source_graph_includes_workspace_root_and_members() {
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("symposium-home");
        std::fs::create_dir(&config_dir).unwrap();
        std::fs::write(config_dir.join("config.toml"), "[used]\n").unwrap();

        let workspace_root = tmp.path().join("workspace");
        std::fs::create_dir(&workspace_root).unwrap();
        write_virtual_workspace(&workspace_root, &[("member-a", "member-a")]);

        let sym = Symposium::from_dir(&config_dir);
        let mut deps = sym.workspace_deps(&workspace_root);
        let graph = ResolvedSourceGraph::resolve_used_and_workspace(&sym, &mut deps)
            .await
            .unwrap();

        let root = std::fs::canonicalize(&workspace_root).unwrap();
        let member = std::fs::canonicalize(workspace_root.join("member-a")).unwrap();
        assert!(graph.nodes().iter().any(|node| {
            node.root.path == root && node.provenance.contains(&SourceProvenance::Workspace)
        }));
        assert!(graph.nodes().iter().any(|node| {
            node.root.path == member && node.provenance.contains(&SourceProvenance::Workspace)
        }));
    }

    #[tokio::test]
    async fn source_graph_dedupes_installed_workspace_root_and_keeps_reasons() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace_root = tmp.path().join("workspace");
        std::fs::create_dir(&workspace_root).unwrap();
        write_virtual_workspace(&workspace_root, &[("member-a", "member-a")]);

        let config_dir = tmp.path().join("symposium-home");
        std::fs::create_dir(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                r#"[used]
paths = ["{}"]
"#,
                workspace_root.display()
            ),
        )
        .unwrap();

        let sym = Symposium::from_dir(&config_dir);
        let mut deps = sym.workspace_deps(&workspace_root);
        let graph = ResolvedSourceGraph::resolve_used_and_workspace(&sym, &mut deps)
            .await
            .unwrap();

        let root = std::fs::canonicalize(&workspace_root).unwrap();
        let root_nodes = graph
            .nodes()
            .iter()
            .filter(|node| node.root.path == root)
            .collect::<Vec<_>>();
        assert_eq!(root_nodes.len(), 1);
        let root_node = root_nodes[0];
        assert!(root_node.provenance.contains(&SourceProvenance::Used));
        assert!(root_node.provenance.contains(&SourceProvenance::Workspace));
        assert_eq!(root_node.reasons.len(), 2);
    }

    #[tokio::test]
    async fn crate_registry_resolver_canonicalizes_path() {
        // The crate registry resolves through cargo metadata, which may return
        // non-canonical paths (e.g., through symlinks). The resolver must
        // canonicalize so that graph dedup works across registries.
        let tmp = tempfile::tempdir().unwrap();
        let real_dir = tmp.path().join("real-plugin");
        write_minimal_crate(&real_dir, "shared-plugin", "0.1.0");

        let symlink_dir = tmp.path().join("link-to-plugin");
        std::os::unix::fs::symlink(&real_dir, &symlink_dir).unwrap();

        let sym = Symposium::from_dir(tmp.path());
        let resolver = SourceRegistryResolver::new(&sym);
        let spec = CrateSourceSpec {
            key: Some("shared-plugin".to_string()),
            dependency: spec_table(&[(
                "path",
                toml::Value::String(symlink_dir.to_string_lossy().to_string()),
            )]),
        };

        let resolved = resolver
            .resolve(&RegistrySourceSpec::Crate(spec))
            .await
            .unwrap();

        let canonical = std::fs::canonicalize(&real_dir).unwrap();
        assert_eq!(
            resolved.path, canonical,
            "crate registry should return canonical path, but got {:?} instead of {:?}",
            resolved.path, canonical
        );
    }

    #[test]
    fn source_graph_records_installed_workspace_and_dependency_on_one_node() {
        let tmp = tempfile::tempdir().unwrap();
        let source = std::fs::canonicalize(tmp.path()).unwrap();
        let root = ResolvedSourceRoot {
            registry: SourceRegistry::Path,
            source_id: "test-source".to_string(),
            path: source,
        };

        let mut graph = ResolvedSourceGraph::default();
        for provenance in [
            SourceProvenance::Used,
            SourceProvenance::Workspace,
            SourceProvenance::Dependency,
        ] {
            graph.add_resolved_root(
                root.clone(),
                SourceReason {
                    provenance,
                    detail: format!("{provenance:?} reason"),
                },
            );
        }

        assert_eq!(graph.nodes().len(), 1);
        let node = &graph.nodes()[0];
        assert!(node.provenance.contains(&SourceProvenance::Used));
        assert!(node.provenance.contains(&SourceProvenance::Workspace));
        assert!(node.provenance.contains(&SourceProvenance::Dependency));
        assert_eq!(node.reasons.len(), 3);
    }

    #[tokio::test]
    async fn build_initial_skips_directory_scoped_entry_when_cwd_differs() {
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("symposium-home");
        std::fs::create_dir(&config_dir).unwrap();

        let plugin_dir = config_dir.join("scoped-plugin");
        std::fs::create_dir(&plugin_dir).unwrap();

        let matching_dir = tmp.path().join("matching-project");
        std::fs::create_dir(&matching_dir).unwrap();
        let non_matching_dir = tmp.path().join("other-project");
        std::fs::create_dir(&non_matching_dir).unwrap();

        // Config with a directory-scoped entry pointing at the scoped-plugin path
        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                r#"[[plugins]]
where.predicates = ["directory({}/**)"]
source.paths = ["scoped-plugin"]
"#,
                matching_dir.display()
            ),
        )
        .unwrap();

        let sym = Symposium::from_dir(&config_dir);

        // From non-matching directory: the scoped entry is skipped
        let mut deps = sym.workspace_deps(&non_matching_dir);
        let graph = ResolvedSourceGraph::build_initial(&sym, &mut deps).await;
        assert!(
            graph.nodes().is_empty(),
            "directory-scoped entry should be skipped when cwd doesn't match"
        );
    }

    #[tokio::test]
    async fn build_initial_includes_directory_scoped_entry_when_cwd_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("symposium-home");
        std::fs::create_dir(&config_dir).unwrap();

        let plugin_dir = config_dir.join("scoped-plugin");
        std::fs::create_dir(&plugin_dir).unwrap();

        let matching_dir = tmp.path().join("matching-project");
        std::fs::create_dir(&matching_dir).unwrap();

        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                r#"[[plugins]]
where.predicates = ["directory({}/**)"]
source.paths = ["scoped-plugin"]
"#,
                matching_dir.display()
            ),
        )
        .unwrap();

        let sym = Symposium::from_dir(&config_dir);

        // From matching directory: the scoped entry is included
        let mut deps = sym.workspace_deps(&matching_dir);
        let graph = ResolvedSourceGraph::build_initial(&sym, &mut deps).await;
        assert_eq!(
            graph.nodes().len(),
            1,
            "directory-scoped entry should be included when cwd matches"
        );
        assert!(graph.nodes()[0].provenance.contains(&SourceProvenance::Used));
    }

    #[tokio::test]
    async fn expand_defers_and_retries_custom_pred_gated_config_entry() {
        // Config entry A: unconditional, loads a plugin that defines `org_check`.
        // Config entry B: gated on `org_check()`, loads source B.
        // After build_initial + expand, source B should be in the graph because
        // org_check is discovered during expansion and the deferred config entry
        // is retried.
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("symposium-home");
        std::fs::create_dir(&config_dir).unwrap();

        // Source A: defines custom predicate org_check
        let source_a = config_dir.join("source-a");
        std::fs::create_dir(&source_a).unwrap();

        let checker_script = source_a.join("checker.sh");
        std::fs::write(&checker_script, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&checker_script, std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }

        std::fs::write(
            source_a.join("SYMPOSIUM.toml"),
            format!(
                r#"name = "source-a-plugin"

[[installations]]
name = "org-checker"
script = "{}"

[[predicate]]
name = "org_check"
command = "org-checker"
"#,
                checker_script.display(),
            ),
        )
        .unwrap();

        // Source B: has a SYMPOSIUM.toml (so it passes the plugin content check)
        let source_b = config_dir.join("source-b");
        std::fs::create_dir(&source_b).unwrap();
        std::fs::write(
            source_b.join("SYMPOSIUM.toml"),
            "name = \"source-b-plugin\"\n",
        )
        .unwrap();

        // Config: entry A unconditional, entry B gated on org_check()
        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                r#"[[plugins]]
source.paths = ["{}"]

[[plugins]]
where.predicates = ["org_check()"]
source.paths = ["{}"]
"#,
                source_a.display(),
                source_b.display(),
            ),
        )
        .unwrap();

        let sym = Symposium::from_dir(&config_dir);
        let workspace_dir = tmp.path().join("workspace");
        std::fs::create_dir(&workspace_dir).unwrap();
        let mut deps = sym.workspace_deps(&workspace_dir);
        let mut graph = ResolvedSourceGraph::build_initial(&sym, &mut deps).await;

        // At this point, entry B should be deferred (org_check not yet known)
        assert!(
            !graph.deferred_config_entries.is_empty(),
            "entry B should be deferred"
        );

        // Expand: scans source A, discovers org_check, retries entry B
        let workspace_crates: Vec<symposium_sdk::workspace::WorkspaceCrate> = vec![];
        let added = expand_source_graph(&mut graph, &sym, &workspace_crates).await;

        // Source B should now be in the graph
        let source_b_canonical = std::fs::canonicalize(&source_b).unwrap();
        assert!(
            graph.nodes().iter().any(|n| n.root.path == source_b_canonical),
            "source B should be in graph after deferred retry resolved org_check(); \
             graph has {} nodes: {:?}",
            graph.nodes().len(),
            graph.nodes().iter().map(|n| &n.root.path).collect::<Vec<_>>()
        );
        assert!(added > 0, "expand should have added source B");
    }

    #[tokio::test]
    async fn expand_skips_custom_pred_gated_config_entry_when_pred_fails() {
        // Config entry B gated on org_check() where the predicate script exits 1.
        // Source B should NOT be in the graph.
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("symposium-home");
        std::fs::create_dir(&config_dir).unwrap();

        let source_a = config_dir.join("source-a");
        std::fs::create_dir(&source_a).unwrap();

        let checker_script = source_a.join("checker.sh");
        std::fs::write(&checker_script, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&checker_script, std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }

        std::fs::write(
            source_a.join("SYMPOSIUM.toml"),
            format!(
                r#"name = "source-a-plugin"

[[installations]]
name = "org-checker"
script = "{}"

[[predicate]]
name = "org_check"
command = "org-checker"
"#,
                checker_script.display(),
            ),
        )
        .unwrap();

        let source_b = config_dir.join("source-b");
        std::fs::create_dir(&source_b).unwrap();
        std::fs::write(
            source_b.join("SYMPOSIUM.toml"),
            "name = \"source-b-plugin\"\n",
        )
        .unwrap();

        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                r#"[[plugins]]
source.paths = ["{}"]

[[plugins]]
where.predicates = ["org_check()"]
source.paths = ["{}"]
"#,
                source_a.display(),
                source_b.display(),
            ),
        )
        .unwrap();

        let sym = Symposium::from_dir(&config_dir);
        let workspace_dir = tmp.path().join("workspace");
        std::fs::create_dir(&workspace_dir).unwrap();
        let mut deps = sym.workspace_deps(&workspace_dir);
        let mut graph = ResolvedSourceGraph::build_initial(&sym, &mut deps).await;

        let workspace_crates: Vec<symposium_sdk::workspace::WorkspaceCrate> = vec![];
        expand_source_graph(&mut graph, &sym, &workspace_crates).await;

        let source_b_canonical = std::fs::canonicalize(&source_b).unwrap();
        assert!(
            !graph.nodes().iter().any(|n| n.root.path == source_b_canonical),
            "source B should NOT be in graph when org_check() fails"
        );
    }

    #[tokio::test]
    async fn expand_warns_on_undefined_custom_pred() {
        // A [[plugins]] source is gated on `never_defined()` — no plugin
        // defines it. The expansion should not include the gated source
        // (it gets deferred and never retried).
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("symposium-home");
        std::fs::create_dir(&config_dir).unwrap();

        let source_a = config_dir.join("source-a");
        std::fs::create_dir(&source_a).unwrap();

        let source_b = config_dir.join("source-b");
        std::fs::create_dir(&source_b).unwrap();
        std::fs::create_dir(source_b.join("skills")).unwrap();

        // Source A references B gated on a predicate nobody defines
        std::fs::write(
            source_a.join("SYMPOSIUM.toml"),
            format!(
                r#"name = "source-a-plugin"

[[plugins]]
where.predicates = ["never_defined()"]
source.path = "{}"
"#,
                source_b.display(),
            ),
        )
        .unwrap();

        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                r#"[[plugins]]
source.paths = ["{}"]
"#,
                source_a.display()
            ),
        )
        .unwrap();

        let sym = Symposium::from_dir(&config_dir);
        let workspace_dir = tmp.path().join("workspace");
        std::fs::create_dir(&workspace_dir).unwrap();
        let mut deps = sym.workspace_deps(&workspace_dir);
        let mut graph = ResolvedSourceGraph::build_initial(&sym, &mut deps).await;

        let workspace_crates: Vec<symposium_sdk::workspace::WorkspaceCrate> = vec![];
        expand_source_graph(&mut graph, &sym, &workspace_crates).await;

        let source_b_canonical = std::fs::canonicalize(&source_b).unwrap();
        assert!(
            !graph.nodes().iter().any(|n| n.root.path == source_b_canonical),
            "source B should NOT be in graph when predicate is never defined"
        );
    }
}
