//! Config-only use/remove commands.
//!
//! Source acquisition is wired in later phases. This module only mutates the
//! registry-ready used-source config deterministically.

use anyhow::{Result, bail};

use crate::config::{CargoDependencySpec, CrateUseSpec, Symposium};
use crate::output::Output;
use crate::report::ReportEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mutation {
    Added,
    Updated,
    AlreadyPresent,
    Removed,
    NotPresent,
}

impl Mutation {
    fn as_status(self) -> &'static str {
        match self {
            Mutation::Added => "added",
            Mutation::Updated => "updated",
            Mutation::AlreadyPresent => "already_added",
            Mutation::Removed => "removed",
            Mutation::NotPresent => "not_present",
        }
    }
}

/// Add crate-, path-, or git-registry sources by editing config.toml.
pub fn use_source(
    sym: &mut Symposium,
    crates: Vec<String>,
    paths: Vec<String>,
    git: Vec<String>,
    global: bool,
    cwd: &std::path::Path,
    out: &Output,
) -> Result<()> {
    validate_source_form("use", &crates, &paths, &git)?;

    let entry = if global {
        global_plugins_entry(&mut sym.config.plugins)
    } else {
        directory_scoped_plugins_entry(&mut sym.config.plugins, cwd)
    };

    let mut changed = false;
    if !crates.is_empty() {
        for spec in crates {
            let spec: CrateUseSpec = spec.parse()?;
            let name = spec.name;
            let mutation =
                insert_crate_entry(&mut entry.source.crates, name.clone(), spec.dependency);
            changed |= matches!(mutation, Mutation::Added | Mutation::Updated);
            emit_source_event("crate", &name, mutation, out);
        }
    } else if !paths.is_empty() {
        for path in paths {
            let mutation = insert_list_entry(&mut entry.source.paths, path.clone());
            changed |= mutation == Mutation::Added;
            emit_source_event("path", &path, mutation, out);
        }
    } else if !git.is_empty() {
        for url in git {
            let mutation = insert_list_entry(&mut entry.source.git, url.clone());
            changed |= mutation == Mutation::Added;
            emit_source_event("git", &url, mutation, out);
        }
    } else {
        bail!("use requires a crate name, --path, or --git");
    }

    if changed {
        sym.rebuild_used_compat();
        sym.save_config()?;
    }
    Ok(())
}

/// Remove crate-, path-, or git-registry sources by editing config.toml.
pub fn remove_source(
    sym: &mut Symposium,
    crates: Vec<String>,
    paths: Vec<String>,
    git: Vec<String>,
    out: &Output,
) -> Result<()> {
    validate_source_form("remove", &crates, &paths, &git)?;

    let mut changed = false;
    if !crates.is_empty() {
        for name in crates {
            if name.contains('@') {
                bail!("remove expects crate names without versions: `{name}`");
            }
            let mutation = remove_crate_from_plugins(&mut sym.config.plugins, &name);
            changed |= mutation == Mutation::Removed;
            emit_source_event("crate", &name, mutation, out);
        }
    } else if !paths.is_empty() {
        for path in paths {
            let mutation =
                remove_from_plugins_list(&mut sym.config.plugins, |e| &mut e.source.paths, &path);
            changed |= mutation == Mutation::Removed;
            emit_source_event("path", &path, mutation, out);
        }
    } else if !git.is_empty() {
        for url in git {
            let mutation =
                remove_from_plugins_list(&mut sym.config.plugins, |e| &mut e.source.git, &url);
            changed |= mutation == Mutation::Removed;
            emit_source_event("git", &url, mutation, out);
        }
    } else {
        bail!("remove requires a crate name, --path, or --git");
    }

    if changed {
        clean_empty_plugins_entries(&mut sym.config.plugins);
        sym.rebuild_used_compat();
        sym.save_config()?;
    }
    Ok(())
}

fn validate_source_form(
    command: &str,
    crates: &[String],
    paths: &[String],
    git: &[String],
) -> Result<()> {
    let forms = (!crates.is_empty()) as u8 + (!paths.is_empty()) as u8 + (!git.is_empty()) as u8;
    if forms > 1 {
        bail!("{command} accepts only one source form at a time: crate, --path, or --git");
    }
    Ok(())
}

use crate::config::PluginsEntry;
use crate::predicate::Predicate;
use std::collections::BTreeMap;
use std::path::Path;

/// Find or create the first global (no-predicate) plugins entry.
fn global_plugins_entry(plugins: &mut Vec<PluginsEntry>) -> &mut PluginsEntry {
    let idx = plugins.iter().position(|e| e.predicates.is_empty());
    match idx {
        Some(i) => &mut plugins[i],
        None => {
            plugins.push(PluginsEntry {
                predicates: crate::predicate::PredicateSet::default(),
                source: crate::config::PluginsEntrySource::default(),
            });
            plugins.last_mut().unwrap()
        }
    }
}

/// Find or create a directory-scoped plugins entry for the given cwd.
///
/// Looks for an existing entry whose single predicate is `directory(<cwd>/**)`
/// (after canonicalization). If not found, creates one.
fn directory_scoped_plugins_entry<'a>(
    plugins: &'a mut Vec<PluginsEntry>,
    cwd: &Path,
) -> &'a mut PluginsEntry {
    let canonical = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let dir_pattern = format!("{}/**", canonical.display());

    let idx = plugins.iter().position(|e| {
        e.predicates.predicates.len() == 1
            && matches!(
                &e.predicates.predicates[0],
                Predicate::Directory(p) if p == &dir_pattern
            )
    });
    match idx {
        Some(i) => &mut plugins[i],
        None => {
            plugins.push(PluginsEntry {
                predicates: crate::predicate::PredicateSet {
                    predicates: vec![Predicate::Directory(dir_pattern)],
                },
                source: crate::config::PluginsEntrySource::default(),
            });
            plugins.last_mut().unwrap()
        }
    }
}

fn insert_crate_entry(
    crates: &mut BTreeMap<String, CargoDependencySpec>,
    name: String,
    dependency: CargoDependencySpec,
) -> Mutation {
    match crates.get(&name) {
        Some(existing) if existing == &dependency => Mutation::AlreadyPresent,
        Some(_) => {
            crates.insert(name, dependency);
            Mutation::Updated
        }
        None => {
            crates.insert(name, dependency);
            Mutation::Added
        }
    }
}

/// Remove a crate from all plugins entries, returns Removed if found in any.
fn remove_crate_from_plugins(plugins: &mut [PluginsEntry], name: &str) -> Mutation {
    let mut found = false;
    for entry in plugins.iter_mut() {
        if entry.source.crates.remove(name).is_some() {
            found = true;
        }
    }
    if found {
        Mutation::Removed
    } else {
        Mutation::NotPresent
    }
}

/// Remove a value from a list field across all plugins entries.
fn remove_from_plugins_list(
    plugins: &mut [PluginsEntry],
    accessor: impl Fn(&mut PluginsEntry) -> &mut Vec<String>,
    value: &str,
) -> Mutation {
    let mut found = false;
    for entry in plugins.iter_mut() {
        let list = accessor(entry);
        let before = list.len();
        list.retain(|v| v != value);
        if list.len() < before {
            found = true;
        }
    }
    if found {
        Mutation::Removed
    } else {
        Mutation::NotPresent
    }
}

/// Remove empty plugins entries (entries with no sources left).
fn clean_empty_plugins_entries(plugins: &mut Vec<PluginsEntry>) {
    plugins.retain(|e| !e.source.is_empty());
}

fn insert_list_entry(entries: &mut Vec<String>, value: String) -> Mutation {
    if entries.contains(&value) {
        return Mutation::AlreadyPresent;
    }
    entries.push(value);
    entries.sort();
    Mutation::Added
}

fn emit_source_event(registry: &'static str, source: &str, mutation: Mutation, out: &Output) {
    tracing::info!(
        report = %ReportEvent::InstalledSourceChanged {
            registry: registry.to_string(),
            source: source.to_string(),
            status: mutation.as_status().to_string(),
        },
    );

    match mutation {
        Mutation::Added => out.added(format!("{registry} source added: {source}")),
        Mutation::Updated => out.done(format!("{registry} source updated: {source}")),
        Mutation::AlreadyPresent => {
            out.already_ok(format!("{registry} source already added: {source}"))
        }
        Mutation::Removed => out.removed(format!("{registry} source removed: {source}")),
        Mutation::NotPresent => out.already_ok(format!("{registry} source not present: {source}")),
    }
}
