//! Status command: `cargo agents status`.
//!
//! Shows the resolved plugin/skill state for the current workspace without
//! installing or modifying anything.

use anyhow::Result;

use crate::config::Symposium;
use crate::crate_sources::{self, SourceProvenance};
use crate::plugins;
use crate::report::ReportEvent;
use crate::skills;
use symposium_sdk::workspace::WorkspaceDeps;

/// Run the status command: resolve sources, evaluate predicates, report state.
pub async fn status(sym: &Symposium, deps: &mut WorkspaceDeps) -> Result<()> {
    let loaded = deps
        .load()
        .ok_or_else(|| anyhow::anyhow!("not in a Rust workspace"))?;
    let workspace_crates = loaded.crates.clone();

    // Build the source graph (same as sync, but we won't install anything).
    let mut graph = build_source_graph(sym, deps).await;

    // Expand with discovery + recursive sources.
    crate_sources::expand_source_graph(&mut graph, sym, &workspace_crates).await;

    // Load plugin registry from graph.
    let registry = plugins::load_registry_from_graph(&graph);

    // Resolve custom predicates.
    let custom_entries = resolve_custom_predicates(sym, &registry).await;

    // Report installed sources.
    for node in graph.nodes() {
        let provenance_str = node
            .provenance
            .iter()
            .map(|p| match p {
                SourceProvenance::Installed => "installed",
                SourceProvenance::Workspace => "workspace",
                SourceProvenance::Dependency => "dependency",
            })
            .collect::<Vec<_>>()
            .join(", ");
        tracing::info!(
            report = %ReportEvent::StatusSource {
                source_id: node.root.source_id.clone(),
                path: node.root.path.display().to_string(),
                provenance: provenance_str,
            },
        );
    }

    // Evaluate plugins and skills.
    let semver_pairs = crate_sources::crate_pairs(&workspace_crates);
    let mut ctx =
        crate::predicate::PredicateContext::with_custom_predicates(&semver_pairs, custom_entries);

    for parsed in &registry.plugins {
        ctx.set_source_provenance(parsed.source_provenance.clone());
        let plugin_active = parsed.plugin.applies(&mut ctx);
        tracing::info!(
            report = %ReportEvent::StatusPlugin {
                name: parsed.plugin.name.clone(),
                active: plugin_active,
                source: parsed.source_name.clone(),
            },
        );

        if !plugin_active {
            continue;
        }

        // Evaluate skill groups.
        for group in &parsed.plugin.skills {
            let group_active = group.predicates.evaluate(&mut ctx);
            let source_desc = format!("{:?}", group.source);
            tracing::info!(
                report = %ReportEvent::StatusSkillGroup {
                    plugin: parsed.plugin.name.clone(),
                    source: source_desc,
                    active: group_active,
                },
            );
        }
    }

    // Report applicable skills count.
    let applicable =
        skills::skills_applicable_to(sym, &registry, &workspace_crates, Default::default()).await;
    tracing::info!(
        report = %ReportEvent::StatusSummary {
            sources: graph.nodes().len(),
            plugins: registry.plugins.len(),
            skills: applicable.len(),
            agents: sym.config.agents.len(),
        },
    );

    // Report configured agents.
    for agent in &sym.config.agents {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!("agent: {}", agent.name),
            },
        );
    }

    Ok(())
}

/// Build the initial source graph (same logic as sync's resolve_sync_sources).
async fn build_source_graph(
    sym: &Symposium,
    deps: &mut WorkspaceDeps,
) -> crate_sources::ResolvedSourceGraph {
    use crate_sources::{
        ResolvedSourceGraph, ResolvedSourceRoot, SourceProvenance, SourceReason, SourceRegistry,
        installed_source_specs,
    };

    let resolver = crate_sources::SourceRegistryResolver::new(sym);
    let mut graph = ResolvedSourceGraph::default();

    for spec in installed_source_specs(sym.installed_sources()) {
        match resolver.resolve(&spec).await {
            Ok(root) => {
                graph.add_resolved_root(
                    root,
                    SourceReason {
                        provenance: SourceProvenance::Installed,
                        detail: "installed config".to_string(),
                    },
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to resolve installed source, skipping");
            }
        }
    }

    if sym.config.agents_syncing
        && let Some(loaded) = deps.load()
    {
        let root_path = std::fs::canonicalize(&loaded.root).unwrap_or_else(|_| loaded.root.clone());
        if root_path.join("SYMPOSIUM.toml").is_file() {
            graph.add_resolved_root(
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
            graph.add_resolved_root(
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

    // Legacy sources.
    let sources = sym.plugin_sources();
    let cache_base = sym.cache_dir().join("plugin-sources");
    for resolved in &sources {
        let Some(dir) = plugins::resolve_legacy_plugin_source_dir(resolved, &cache_base) else {
            continue;
        };
        if !dir.is_dir() {
            continue;
        }
        graph.add_resolved_root(
            ResolvedSourceRoot {
                registry: SourceRegistry::Path,
                source_id: format!("legacy:{}", resolved.source.name),
                path: dir,
            },
            SourceReason {
                provenance: SourceProvenance::Installed,
                detail: format!("legacy plugin-source `{}`", resolved.source.name),
            },
        );
    }

    graph
}

/// Resolve custom predicate installations (mirrors sync logic).
async fn resolve_custom_predicates(
    sym: &Symposium,
    registry: &plugins::PluginRegistry,
) -> std::collections::HashMap<String, crate::predicate::ResolvedPredicateEntry> {
    use crate::predicate::ResolvedPredicateEntry;

    let mut entries = std::collections::HashMap::new();

    for (name, resolved) in registry.custom_predicates.iter() {
        let plugin = &registry.plugins[resolved.plugin_index];
        let Some(install) = plugin.plugin.get_installation(&resolved.command) else {
            continue;
        };

        let acquired =
            match crate::installation::acquire_installation(sym, install, None, None).await {
                Ok(a) => a,
                Err(_) => continue,
            };

        let runnable =
            match crate::installation::resolve_runnable(acquired, &format!("predicate `{name}`")) {
                Ok(r) => r,
                Err(_) => continue,
            };

        entries.insert(
            name.clone(),
            ResolvedPredicateEntry {
                runnable,
                args: resolved.args.clone(),
            },
        );
    }

    entries
}
