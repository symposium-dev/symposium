//! The event each `[expected] hooks` entry runs on, from the `[[hooks]]` of the task's
//! plugins (RFD reference, "Synchronization and hooks").
//!
//! A task runs on every client, so a hook limited to one agent, by `agent` or by a
//! `format` other than `symposium`, cannot be expected. The manifest's schema already
//! holds each hook to one of Symposium's events.

use super::manifest::{Plugin, RawHook};
use super::{HookName, PluginName, TaskError, distinct};
use crate::client::HookEvent;

/// An expected plugin hook, with the event its plugin declares it on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpectedHook {
    pub(crate) name: HookName,
    pub(crate) event: HookEvent,
}

/// A hook a plugin declares.
struct Declared<'a> {
    plugin: &'a PluginName,
    hook: &'a RawHook,
}

/// Finds each of `expected` in exactly one of `plugins`' manifests. A plugin without a
/// `Symposium.toml` declares no hooks.
pub(super) fn resolve(
    plugins: &[Plugin<'_>],
    expected: &[HookName],
    errors: &mut Vec<TaskError>,
) -> Vec<ExpectedHook> {
    let declared: Vec<_> = plugins
        .iter()
        .flat_map(|plugin| {
            let hooks = plugin.manifest.iter().flat_map(|manifest| &manifest.hooks);
            hooks.map(|hook| Declared {
                plugin: plugin.name,
                hook,
            })
        })
        .collect();
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
    let hook = found.hook;
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
    Some(ExpectedHook {
        name: name.clone(),
        event: HookEvent::named(&hook.event),
    })
}
