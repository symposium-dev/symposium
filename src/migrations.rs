//! One-shot cleanups for state a past release wrote and the current one no
//! longer maintains.
//!
//! Dropping an agent also drops the code that would otherwise reap what
//! symposium installed for it, so the files outlive the support: hook entries
//! keep invoking a subcommand that no longer accepts them, and skill
//! directories sit in a tree nothing scans. Each migration is recorded by id in
//! `state.toml`, so it runs once per config directory regardless of which
//! release the user came from.

use std::fs;
use std::path::Path;

use crate::config::Symposium;
use crate::output::{Output, display_path};
use crate::state;

/// Removes what symposium installed for Gemini CLI, whose support was dropped.
const GEMINI_REMOVAL: &str = "remove-gemini-support";

/// Apply any pending one-shot migrations.
///
/// Every step is individually best-effort and returns nothing to propagate:
/// this runs on the startup path of every command, hook dispatch included, so a
/// leftover it cannot remove must not fail the command the user actually asked
/// for. A migration is marked applied either way rather than retried forever.
pub fn run_pending(sym: &mut Symposium, out: &Output) {
    if !state::migration_applied(sym.config_dir(), GEMINI_REMOVAL) {
        remove_gemini_support(sym, out);
        state::record_migration(sym.config_dir(), GEMINI_REMOVAL);
    }
}

/// Undo the Gemini installation: drop the config entry, unregister the hooks
/// that would now invoke a removed subcommand, and reap the skill directories
/// symposium owns under `~/.gemini/`.
fn remove_gemini_support(sym: &mut Symposium, out: &Output) {
    let home = sym.home_dir().to_path_buf();

    // The hook entries matter most: Gemini keeps firing them, and the command
    // they name no longer accepts `gemini`.
    crate::agents::unregister_settings_hooks(
        &home.join(".gemini").join("settings.json"),
        "cargo-agents hook",
        out,
    );

    // Only Gemini's own skills directory is ours to reap. Project skills went
    // to the shared `.agents/skills/`, which other agents still use, so the
    // ordinary marker-based cleanup handles those. Antigravity's directories
    // live under `.gemini/config/`, which this must not touch.
    reap_marked_dirs(&home.join(".gemini").join("skills"), out);

    let before = sym.config.agents.len();
    sym.config
        .agents
        .retain(|a| !crate::agents::Agent::is_retired(&a.name));
    if sym.config.agents.len() != before && sym.save_config().is_ok() {
        out.removed("removed the retired `gemini` agent from your symposium config");
    }
}

/// Remove every immediate subdirectory of `parent` carrying the symposium
/// marker, leaving user-authored skills alone.
fn reap_marked_dirs(parent: &Path, out: &Output) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.join(crate::sync::MARKER_FILE).exists() {
            continue;
        }
        if fs::remove_dir_all(&dir).is_ok() {
            out.removed(format!("{}: removed", display_path(&dir)));
        }
    }
    // An emptied parent is ours too, but a non-empty one is not an error.
    let _ = fs::remove_dir(parent);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentEntry;

    #[test]
    fn gemini_removal_clears_config_hooks_and_managed_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let mut sym = Symposium::from_dir(tmp.path());
        let home = sym.home_dir().to_path_buf();

        sym.config.agents = vec![
            AgentEntry {
                name: "antigravity".into(),
            },
            AgentEntry {
                name: "gemini".into(),
            },
        ];
        sym.save_config().unwrap();

        let settings = home.join(".gemini").join("settings.json");
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        fs::write(
            &settings,
            serde_json::json!({
                "hooks": {
                    "BeforeTool": [{
                        "matcher": ".*",
                        "hooks": [{ "command": "cargo-agents hook gemini pre-tool-use" }]
                    }]
                },
                "theme": "dark"
            })
            .to_string(),
        )
        .unwrap();

        let managed = home.join(".gemini").join("skills").join("serde-guidance");
        let user_authored = home.join(".gemini").join("skills").join("mine");
        fs::create_dir_all(&managed).unwrap();
        fs::write(managed.join(crate::sync::MARKER_FILE), "").unwrap();
        fs::create_dir_all(&user_authored).unwrap();

        // Antigravity lives under the same `.gemini` root; reaping must not
        // reach into its directories.
        let antigravity_skill = home
            .join(".gemini")
            .join("config")
            .join("skills")
            .join("kept");
        fs::create_dir_all(&antigravity_skill).unwrap();
        fs::write(antigravity_skill.join(crate::sync::MARKER_FILE), "").unwrap();

        remove_gemini_support(&mut sym, &Output::quiet());

        assert_eq!(sym.config.agents.len(), 1, "the retired entry is dropped");
        assert_eq!(sym.config.agents[0].name, "antigravity");

        let after = fs::read_to_string(&settings).unwrap();
        assert!(
            !after.contains("cargo-agents hook"),
            "the hook that would invoke a removed subcommand is gone"
        );
        let parsed: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(parsed["theme"], "dark", "unrelated settings are preserved");

        assert!(!managed.exists(), "a symposium-managed skill is reaped");
        assert!(
            user_authored.exists(),
            "a skill symposium did not install is left alone"
        );
        assert!(
            antigravity_skill.exists(),
            "Antigravity's own skills under .gemini/config are untouched"
        );
    }

    #[test]
    fn the_migration_runs_once() {
        let tmp = tempfile::tempdir().unwrap();
        let mut sym = Symposium::from_dir(tmp.path());

        run_pending(&mut sym, &Output::quiet());
        assert!(state::migration_applied(sym.config_dir(), GEMINI_REMOVAL));

        // A second run is a no-op: a config the user has since re-added is not
        // silently rewritten.
        sym.config.agents = vec![AgentEntry {
            name: "gemini".into(),
        }];
        run_pending(&mut sym, &Output::quiet());
        assert_eq!(sym.config.agents.len(), 1);
    }
}
