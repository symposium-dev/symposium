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
    /// Which manifest defined the plugin, when that is known. A hit found by
    /// searching a package manager has not been loaded, so there is nothing to
    /// report yet.
    pub kind: Option<String>,
    /// Set when the plugin is loaded but inactive until a `use` entry names it.
    pub dormant: bool,
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
                version: parsed.plugin.version.clone(),
                description: parsed.plugin.description.clone(),
                kind: parsed.plugin.kind.label().map(str::to_string),
                dormant: parsed.plugin.requires_use,
            });
        }
    }
    // Search is workspace-independent; a detached resolver stands in.
    for (instance, info) in sym.detached_managers().search(query).await {
        matches.push(SearchMatch {
            origin: instance,
            name: info.id.name.clone(),
            version: Some(info.id.version.clone()),
            description: info.description,
            // Found by asking a package manager, so nothing has been loaded and
            // there is no manifest to report yet.
            kind: None,
            dormant: false,
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
                    plugin_kind: m.kind.clone(),
                    dormant: m.dormant,
                },
            );
        }
    }
    Ok(())
}
