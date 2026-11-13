//! The `SKILL.md` that provides each expected skill, from the task's plugins (RFD
//! reference, "Skills").
//!
//! Skills are found as Symposium finds a registry plugin's: a plugin with a
//! `Symposium.toml` provides what its `[[skills]] source.path` groups hold, and one
//! without provides the `SKILL.md` at its root. Within a group, every `SKILL.md` counts
//! except one inside another skill or inside a directory holding sync's marker. A skill
//! is named by its frontmatter, which is also the directory sync installs it in, and
//! must be one Symposium would load. The plugin's listing is read once, and holds no
//! links, so a source is followed nowhere else.

use std::fs;
use std::path::{Component, Path, PathBuf};

use super::manifest::{Plugin, RawSkillGroup};
use super::{SkillExpectations, SkillName, TaskError};
use crate::skill_md;
use crate::tree::EntryKind;

/// The marker sync leaves in every directory it installs. Discovery skips a directory
/// holding an entry of this name, whatever kind it is.
const MARKER: &str = ".symposium";

/// An expected skill, and the directory in `evals/` that provides it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillSource {
    pub(crate) name: SkillName,
    pub(crate) dir: PathBuf,
}

/// A skill a plugin provides.
struct Provided {
    name: String,
    dir: PathBuf,
}

/// Finds the one provider of each skill `expected` names. A skill that must be installed
/// or used needs one; any named skill with two is refused, since sync would then install
/// neither under its own name. One `SKILL.md` reached through two groups is one
/// provider, as sync counts it.
pub(super) fn resolve(
    plugins: &[Plugin<'_>],
    expected: &SkillExpectations,
    errors: &mut Vec<TaskError>,
) -> Vec<SkillSource> {
    let mut provided: Vec<Provided> = Vec::new();
    for plugin in plugins {
        for skill in plugin_skills(plugin, errors) {
            if !provided.iter().any(|seen| seen.dir == skill.dir) {
                provided.push(skill);
            }
        }
    }
    let needed =
        |skill: &SkillName| expected.installed().contains(skill) || expected.used().contains(skill);
    let mut sources = Vec::new();
    for skill in expected.names() {
        let mut providers = provided
            .iter()
            .filter(|provided| provided.name == skill.as_str());
        match (providers.next(), providers.next()) {
            (Some(first), Some(second)) => errors.push(TaskError::AmbiguousSkill {
                skill: skill.clone(),
                first: first.dir.clone(),
                second: second.dir.clone(),
            }),
            (Some(only), None) => sources.push(SkillSource {
                name: skill.clone(),
                dir: only.dir.clone(),
            }),
            (None, _) if needed(skill) => errors.push(TaskError::UnprovidedSkill(skill.clone())),
            (None, _) => {}
        }
    }
    sources
}

fn plugin_skills(plugin: &Plugin<'_>, errors: &mut Vec<TaskError>) -> Vec<Provided> {
    let Some(manifest) = &plugin.manifest else {
        // A plugin without a manifest is a registry's bare skill only at its root.
        let bare = plugin
            .entries
            .iter()
            .any(|entry| entry.path == "SKILL.md" && entry.kind == EntryKind::File);
        if !bare {
            return Vec::new();
        }
        return named(plugin, vec![plugin.dir.clone()], errors);
    };
    let mut dirs = Vec::new();
    for group in &manifest.skills {
        match source_path(group).map(|path| (path, relative(path))) {
            Some((_, Some(source))) => dirs.extend(group_skills(plugin, &source, errors)),
            Some((path, None)) => errors.push(TaskError::EscapingSkillSource {
                plugin: plugin.name.clone(),
                path: path.to_owned(),
            }),
            None => errors.push(TaskError::UnsupportedSkillSource(plugin.name.clone())),
        }
    }
    named(plugin, dirs, errors)
}

/// The group's `source.path`, the only source a trial, which is offline, can use.
fn source_path(group: &RawSkillGroup) -> Option<&str> {
    let source = group.source.as_ref()?.as_table()?;
    match (source.len(), source.get("path")) {
        (1, Some(path)) => path.as_str(),
        _ => None,
    }
}

/// `path` `/`-joined as the listing writes it, `""` for the plugin itself, or `None`
/// when it leaves the plugin, which is all a trial copies.
fn relative(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// The directory of each skill in the group at `source`, outermost first.
fn group_skills(plugin: &Plugin<'_>, source: &str, errors: &mut Vec<TaskError>) -> Vec<PathBuf> {
    let is_dir = source.is_empty()
        || plugin
            .entries
            .iter()
            .any(|entry| entry.path == source && entry.kind == EntryKind::Dir);
    if !is_dir {
        errors.push(TaskError::MissingSkillSource {
            plugin: plugin.name.clone(),
            path: source.to_owned(),
        });
        return Vec::new();
    }
    let mut skills = Vec::new();
    let mut installed = Vec::new();
    for entry in &plugin.entries {
        let inside = match source {
            "" => Some(entry.path.as_str()),
            source => entry
                .path
                .strip_prefix(source)
                .and_then(|rest| rest.strip_prefix('/')),
        };
        let Some(inside) = inside else {
            continue;
        };
        let (parent, file) = inside.rsplit_once('/').unwrap_or(("", inside));
        if file == MARKER {
            installed.push(parent.to_owned());
        }
        if file == "SKILL.md" && entry.kind == EntryKind::File {
            skills.push(parent.to_owned());
        }
    }
    skills.retain(|skill| !installed.iter().any(|dir| is_within(skill, dir)));
    // Shallowest first, so a skill is kept before any nested in it.
    skills.sort_by_key(|skill| skill.matches('/').count() + usize::from(!skill.is_empty()));
    let mut outermost: Vec<String> = Vec::new();
    for skill in skills {
        if !outermost.iter().any(|outer| is_within(&skill, outer)) {
            outermost.push(skill);
        }
    }
    outermost
        .into_iter()
        .map(|skill| {
            [source, skill.as_str()]
                .into_iter()
                .flat_map(|part| part.split('/'))
                .filter(|part| !part.is_empty())
                .fold(plugin.dir.clone(), |dir, part| dir.join(part))
        })
        .collect()
}

/// Whether `/`-joined `path` is `dir` or under it; `""` is the root.
fn is_within(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Each skill in `dirs` with its name, if Symposium would load it.
fn named(plugin: &Plugin<'_>, dirs: Vec<PathBuf>, errors: &mut Vec<TaskError>) -> Vec<Provided> {
    dirs.into_iter()
        .filter_map(|dir| {
            let path = dir.join("SKILL.md");
            let text = match fs::read_to_string(&path) {
                Ok(text) => text,
                Err(source) => {
                    errors.push(TaskError::Inspect { path, source });
                    return None;
                }
            };
            match skill_md::loadable_name(&text) {
                Ok(name) => Some(Provided { name, dir }),
                Err(problem) => {
                    errors.push(TaskError::UnloadableSkill {
                        plugin: plugin.name.clone(),
                        path,
                        problem,
                    });
                    None
                }
            }
        })
        .collect()
}
