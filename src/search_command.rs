//! `cargo agents search` — find plugins across every configured source.
//!
//! Two arms, in the order a user cares about:
//!
//! 1. **Already loaded** — plugin and standalone-skill names in the
//!    [`PluginRegistry`](crate::plugins::PluginRegistry). A configured
//!    registry is a trust root, so a hit here is available now, no `use`
//!    needed (unless the plugin is dormant).
//! 2. **Offered by a package manager** — [`PmRegistry::search`] unions each
//!    instance's search. A PM without a searchable registry returns an empty
//!    list rather than an error, and an instance that fails outright is
//!    skipped, so an offline crates.io never fails the command.
//!
//! Every hit is tagged with the instance name it came from.
//!
//! [`PmRegistry::search`]: crate::pm::PmRegistry::search

use anyhow::Result;

use crate::config::Symposium;
use crate::pm::PmContext;
use crate::report::ReportEvent;

/// One search hit, in display form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// The instance the hit came from: a configured registry's name, a
    /// package-manager transport (`cargo`), or `(workspace)`.
    pub origin: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
}

/// Case-insensitive substring match — the same looseness `cargo search` has.
fn name_matches(name: &str, query: &str) -> bool {
    name.to_lowercase().contains(&query.to_lowercase())
}

/// Collect matches from the loaded registry and from every package manager.
pub async fn find_matches(sym: &Symposium, query: &str) -> Vec<SearchMatch> {
    let mut matches = Vec::new();

    let registry = crate::plugins::load_registry(sym).await;
    for parsed in &registry.plugins {
        if name_matches(&parsed.plugin.name, query) {
            matches.push(SearchMatch {
                origin: parsed.canonical.pm.clone(),
                name: parsed.plugin.name.clone(),
                version: None,
                description: parsed
                    .plugin
                    .requires_use
                    .then(|| "dormant — enable with `cargo agents use`".to_string()),
            });
        }
    }
    for entry in &registry.standalone_skills {
        if name_matches(entry.skill.name(), query) {
            matches.push(SearchMatch {
                origin: "(standalone skills)".to_string(),
                name: entry.skill.name().to_string(),
                version: None,
                description: entry.skill.frontmatter.get("description").cloned(),
            });
        }
    }

    let cx = PmContext::new(sym, &[]);
    for (instance, info) in sym.package_managers().search(query, &cx).await {
        matches.push(SearchMatch {
            origin: instance,
            name: info.id.name.clone(),
            version: Some(info.id.version.clone()),
            description: info.description,
        });
    }

    matches
}

/// The `cargo agents search` entry point: report every match grouped by the
/// instance it came from, or a nothing-found message.
pub async fn search(sym: &Symposium, query: &str) -> Result<()> {
    let matches = find_matches(sym, query).await;
    if matches.is_empty() {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!("no plugins matching `{query}` found"),
            },
        );
        return Ok(());
    }

    // Group by origin, preserving the order origins were first seen (loaded
    // registry first, then package managers in config order).
    let mut origins: Vec<&str> = Vec::new();
    for m in &matches {
        if !origins.contains(&m.origin.as_str()) {
            origins.push(&m.origin);
        }
    }
    for origin in origins {
        tracing::info!(
            report = %ReportEvent::Info { message: format!("from {origin}:") },
        );
        for m in matches.iter().filter(|m| m.origin == origin) {
            tracing::info!(
                report = %ReportEvent::SearchMatch {
                    origin: m.origin.clone(),
                    name: m.name.clone(),
                    version: m.version.clone(),
                    description: m.description.clone(),
                },
            );
        }
    }
    Ok(())
}
