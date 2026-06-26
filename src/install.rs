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
    out: &Output,
) -> Result<()> {
    validate_source_form("use", &crates, &paths, &git)?;

    let mut changed = false;
    if !crates.is_empty() {
        for spec in crates {
            let spec: CrateUseSpec = spec.parse()?;
            let name = spec.name;
            let mutation = insert_crate(sym, name.clone(), spec.dependency);
            changed |= matches!(mutation, Mutation::Added | Mutation::Updated);
            emit_source_event("crate", &name, mutation, out);
        }
    } else if !paths.is_empty() {
        for path in paths {
            let mutation = insert_list_entry(&mut sym.config.used.paths, path.clone());
            changed |= mutation == Mutation::Added;
            emit_source_event("path", &path, mutation, out);
        }
    } else if !git.is_empty() {
        for url in git {
            let mutation = insert_list_entry(&mut sym.config.used.git, url.clone());
            changed |= mutation == Mutation::Added;
            emit_source_event("git", &url, mutation, out);
        }
    } else {
        bail!("use requires a crate name, --path, or --git");
    }

    if changed {
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
            let mutation = if sym.config.used.crates.remove(&name).is_some() {
                Mutation::Removed
            } else {
                Mutation::NotPresent
            };
            changed |= mutation == Mutation::Removed;
            emit_source_event("crate", &name, mutation, out);
        }
    } else if !paths.is_empty() {
        for path in paths {
            let mutation = remove_list_entry(&mut sym.config.used.paths, &path);
            changed |= mutation == Mutation::Removed;
            emit_source_event("path", &path, mutation, out);
        }
    } else if !git.is_empty() {
        for url in git {
            let mutation = remove_list_entry(&mut sym.config.used.git, &url);
            changed |= mutation == Mutation::Removed;
            emit_source_event("git", &url, mutation, out);
        }
    } else {
        bail!("remove requires a crate name, --path, or --git");
    }

    if changed {
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

fn insert_crate(sym: &mut Symposium, name: String, dependency: CargoDependencySpec) -> Mutation {
    match sym.config.used.crates.get(&name) {
        Some(existing) if existing == &dependency => Mutation::AlreadyPresent,
        Some(_) => {
            sym.config.used.crates.insert(name, dependency);
            Mutation::Updated
        }
        None => {
            sym.config.used.crates.insert(name, dependency);
            Mutation::Added
        }
    }
}

fn insert_list_entry(entries: &mut Vec<String>, value: String) -> Mutation {
    if entries.contains(&value) {
        return Mutation::AlreadyPresent;
    }
    entries.push(value);
    entries.sort();
    Mutation::Added
}

fn remove_list_entry(entries: &mut Vec<String>, value: &str) -> Mutation {
    let before = entries.len();
    entries.retain(|entry| entry != value);
    if entries.len() == before {
        Mutation::NotPresent
    } else {
        Mutation::Removed
    }
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
        Mutation::NotPresent => {
            out.already_ok(format!("{registry} source not present: {source}"))
        }
    }
}
