//! The event each `[expected] hooks` entry runs on, from the `[[hooks]]` of the task's
//! plugins (RFD reference, "Synchronization and hooks").
//!
//! This reader is the checker's own, not Symposium's, and reads only the fields it
//! needs. A task runs on every client, so a hook limited to one agent, by `agent` or by
//! a `format` other than `symposium`, cannot be expected.

use std::fs;
use std::io;

use serde::Deserialize;

use super::{HookName, PluginName, TaskError, distinct};
use crate::client::HookEvent;
use crate::evals::Evals;

/// Symposium's names for the events a hook can run on.
const EVENTS: [&str; 5] = [
    "PreToolUse",
    "PostToolUse",
    "UserPromptSubmit",
    "SessionStart",
    "Stop",
];

/// An expected plugin hook, with the event its plugin declares it on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpectedHook {
    pub(crate) name: HookName,
    pub(crate) event: HookEvent,
}

#[derive(Debug, Default, Deserialize)]
struct Manifest {
    #[serde(default)]
    hooks: Vec<RawHook>,
}

#[derive(Debug, Deserialize)]
struct RawHook {
    name: String,
    event: String,
    agent: Option<String>,
    format: Option<String>,
}

/// A hook a plugin declares.
struct Declared<'a> {
    plugin: &'a PluginName,
    hook: RawHook,
}

/// Finds each of `expected` in exactly one of `plugins`' manifests. A plugin without a
/// `Symposium.toml` declares no hooks; one that cannot be read could declare any, so
/// then only its error is reported.
pub(super) fn resolve(
    evals: &Evals,
    plugins: &[PluginName],
    expected: &[HookName],
    errors: &mut Vec<TaskError>,
) -> Vec<ExpectedHook> {
    if expected.is_empty() {
        return Vec::new();
    }
    let mut declared = Vec::new();
    let mut unreadable = false;
    for plugin in distinct(plugins) {
        match manifest(evals, plugin) {
            Ok(manifest) => declared.extend(
                manifest
                    .hooks
                    .into_iter()
                    .map(|hook| Declared { plugin, hook }),
            ),
            Err(error) => {
                errors.push(error);
                unreadable = true;
            }
        }
    }
    if unreadable {
        return Vec::new();
    }
    distinct(expected)
        .filter_map(|name| {
            let mut matching = declared
                .iter()
                .filter(|declared| declared.hook.name == name.as_ref());
            let Some(found) = matching.next() else {
                errors.push(TaskError::UndeclaredHook(name.clone()));
                return None;
            };
            if let Some(again) = matching.next() {
                errors.push(TaskError::AmbiguousHook {
                    hook: name.clone(),
                    first: found.plugin.clone(),
                    second: again.plugin.clone(),
                });
                return None;
            }
            check(name, found, errors)
        })
        .collect()
}

fn check(
    name: &HookName,
    found: &Declared<'_>,
    errors: &mut Vec<TaskError>,
) -> Option<ExpectedHook> {
    let hook = &found.hook;
    let limited = hook.agent.is_some()
        || hook
            .format
            .as_deref()
            .is_some_and(|format| format != "symposium");
    if limited {
        errors.push(TaskError::AgentLimitedHook {
            hook: name.clone(),
            plugin: found.plugin.clone(),
        });
        return None;
    }
    if !EVENTS.contains(&hook.event.as_str()) {
        errors.push(TaskError::UnknownHookEvent {
            hook: name.clone(),
            event: hook.event.clone(),
        });
        return None;
    }
    Some(ExpectedHook {
        name: name.clone(),
        event: HookEvent::named(&hook.event),
    })
}

fn manifest(evals: &Evals, plugin: &PluginName) -> Result<Manifest, TaskError> {
    let path = evals.plugin_dir(plugin).join("Symposium.toml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Manifest::default()),
        Err(source) => return Err(TaskError::Inspect { path, source }),
    };
    toml::from_str(&text).map_err(|source| TaskError::PluginManifest {
        plugin: plugin.clone(),
        source,
    })
}
