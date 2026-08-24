use super::*;
use crate::agent_plugin::manifest::Manifest;
use crate::pm::{ANY_VERSION, PackageId};

fn plugin(name: &str, dir: &str, version: &str) -> CompiledPlugin {
    CompiledPlugin {
        source_id: PackageId::new("test", "src", ANY_VERSION),
        dir_name: dir.to_string(),
        manifest: Manifest::new(name.to_string(), version.to_string(), None),
        scope: Scope::Global,
        skills: Vec::new(),
    }
}

fn registration<'a>(
    root: &'a Path,
    marketplace: &'a str,
    plugins: &'a [&'a CompiledPlugin],
    scope: Scope,
) -> Registration<'a> {
    Registration {
        marketplace,
        root,
        plugins,
        scope,
    }
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("read")).expect("json")
}

// ── which agent takes which scope ────────────────────────────────────

#[test]
fn only_claude_code_takes_a_project_scoped_plugin() {
    assert!(Agent::Claude.accepts_plugin_scope(Scope::Project));
    assert!(Agent::Claude.accepts_plugin_scope(Scope::Global));

    for agent in [Agent::Codex, Agent::Copilot, Agent::Gemini] {
        assert!(agent.accepts_plugin_scope(Scope::Global), "{agent:?}");
        assert!(
            !agent.accepts_plugin_scope(Scope::Project),
            "{agent:?} stores plugins per user with no way to bound one to a project"
        );
    }
    for agent in [Agent::Goose, Agent::Kiro, Agent::OpenCode] {
        assert!(!agent.accepts_plugin_scope(Scope::Global), "{agent:?}");
        assert!(!agent.accepts_plugin_scope(Scope::Project), "{agent:?}");
    }
}

// ── claude ───────────────────────────────────────────────────────────

#[test]
fn claude_registers_the_root_and_copies_nothing() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tmp.path().join("home");
    let root = tmp.path().join("staging");
    let one = plugin("pdf-tools", "pdf-tools", "1.2.0");

    let written = Agent::Claude
        .install_plugins(
            &registration(&root, "symposium", &[&one], Scope::Global),
            &home,
            tmp.path(),
            Duration::ZERO,
        )
        .expect("install");
    assert!(
        written.is_empty(),
        "Claude reads the staging root in place, so nothing is copied"
    );

    let settings = read(&home.join(".claude/settings.json"));
    assert_eq!(
        settings["extraKnownMarketplaces"]["symposium"]["source"]["source"],
        "directory"
    );
    assert_eq!(
        settings["extraKnownMarketplaces"]["symposium"]["source"]["path"],
        root.display().to_string()
    );
    assert_eq!(settings["enabledPlugins"]["pdf-tools@symposium"], true);

    let known = read(&home.join(".claude/plugins/known_marketplaces.json"));
    assert_eq!(
        known["symposium"]["installLocation"],
        root.display().to_string(),
        "the record Claude needs in the same session, not just next time"
    );
    assert!(known["symposium"]["lastUpdated"].is_string());
}

#[test]
fn claude_enables_a_project_plugin_in_the_project_settings() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tmp.path().join("home");
    let project = tmp.path().join("project");
    let root = project.join(".symposium/plugins");
    let one = plugin("house-style", "house-style", "0.0.0");

    Agent::Claude
        .install_plugins(
            &registration(
                &root,
                "symposium-reporter-ab12cd34",
                &[&one],
                Scope::Project,
            ),
            &home,
            &project,
            Duration::ZERO,
        )
        .expect("install");

    let user = read(&home.join(".claude/settings.json"));
    assert!(
        user["extraKnownMarketplaces"]["symposium-reporter-ab12cd34"].is_object(),
        "registration is user-level even for a project-scoped plugin"
    );
    assert!(
        user.get("enabledPlugins")
            .is_none_or(|v| v.get("house-style@symposium-reporter-ab12cd34").is_none()),
        "but enablement must not leak into other projects"
    );

    let scoped = read(&project.join(".claude/settings.json"));
    assert_eq!(
        scoped["enabledPlugins"]["house-style@symposium-reporter-ab12cd34"],
        true
    );
}

#[test]
fn a_plugin_that_stops_applying_loses_its_entries_and_the_users_are_left_alone() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tmp.path().join("home");
    let root = tmp.path().join("staging");
    let settings_path = home.join(".claude/settings.json");
    std::fs::create_dir_all(settings_path.parent().unwrap()).expect("create");
    std::fs::write(
        &settings_path,
        r#"{
            "enabledPlugins": { "caveman@caveman": true },
            "extraKnownMarketplaces": { "caveman": { "source": { "source": "github" } } }
        }"#,
    )
    .expect("seed");

    let one = plugin("pdf-tools", "pdf-tools", "1.2.0");
    let listed = [&one];
    Agent::Claude
        .install_plugins(
            &registration(&root, "symposium", &listed, Scope::Global),
            &home,
            tmp.path(),
            Duration::ZERO,
        )
        .expect("install");
    assert_eq!(
        read(&settings_path)["enabledPlugins"]["pdf-tools@symposium"],
        true
    );

    Agent::Claude
        .install_plugins(
            &registration(&root, "symposium", &[], Scope::Global),
            &home,
            tmp.path(),
            Duration::ZERO,
        )
        .expect("uninstall");

    let settings = read(&settings_path);
    assert!(
        settings["enabledPlugins"]
            .get("pdf-tools@symposium")
            .is_none(),
        "our entry goes when the plugin stops applying"
    );
    assert_eq!(
        settings["enabledPlugins"]["caveman@caveman"], true,
        "a plugin from a marketplace we do not own is never touched"
    );
    assert!(
        settings["extraKnownMarketplaces"]
            .get("symposium")
            .is_none()
    );
    assert!(settings["extraKnownMarketplaces"]["caveman"].is_object());
    assert!(
        read(&home.join(".claude/plugins/known_marketplaces.json"))
            .get("symposium")
            .is_none()
    );
}

// ── codex ────────────────────────────────────────────────────────────

fn staged_plugin(root: &Path, dir: &str) -> CompiledPlugin {
    let skill = root.join(dir).join("skills").join("probe");
    std::fs::create_dir_all(&skill).expect("create");
    std::fs::write(skill.join("SKILL.md"), "---\nname: probe\n---\nbody\n").expect("write");
    std::fs::write(root.join(dir).join("plugin.json"), "{}").expect("write");
    plugin(dir, dir, "0.4.2")
}

#[test]
fn codex_gets_config_entries_and_a_version_keyed_copy() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tmp.path().join("home");
    let root = tmp.path().join("staging");
    let one = staged_plugin(&root, "pdf-tools");

    let config_path = home.join(".codex/config.toml");
    std::fs::create_dir_all(config_path.parent().unwrap()).expect("create");
    std::fs::write(
        &config_path,
        "[projects.\"/work/reporter\"]\ntrust_level = \"trusted\"\n",
    )
    .expect("seed");

    let written = Agent::Codex
        .install_plugins(
            &registration(&root, "symposium", &[&one], Scope::Global),
            &home,
            tmp.path(),
            Duration::ZERO,
        )
        .expect("install");

    let config = std::fs::read_to_string(&config_path).expect("read");
    assert!(
        config.contains("[projects.\"/work/reporter\"]"),
        "unrelated config survives: {config}"
    );
    assert!(config.contains("[marketplaces.symposium]"), "{config}");
    assert!(config.contains("source_type = \"local\""), "{config}");
    assert!(
        config.contains("[plugins.\"pdf-tools@symposium\"]"),
        "{config}"
    );
    assert!(config.contains("enabled = true"), "{config}");

    let expected = home.join(".codex/plugins/cache/symposium/pdf-tools/0.4.2");
    assert_eq!(written, vec![expected.clone()]);
    assert!(
        expected.join("skills/probe/SKILL.md").is_file(),
        "Codex loads only from its own cache, so the content is copied"
    );
}

#[test]
fn codex_drops_our_entries_when_a_plugin_stops_applying() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tmp.path().join("home");
    let root = tmp.path().join("staging");
    let one = staged_plugin(&root, "pdf-tools");

    Agent::Codex
        .install_plugins(
            &registration(&root, "symposium", &[&one], Scope::Global),
            &home,
            tmp.path(),
            Duration::ZERO,
        )
        .expect("install");
    Agent::Codex
        .install_plugins(
            &registration(&root, "symposium", &[], Scope::Global),
            &home,
            tmp.path(),
            Duration::ZERO,
        )
        .expect("uninstall");

    let config = std::fs::read_to_string(home.join(".codex/config.toml")).expect("read");
    assert!(!config.contains("marketplaces.symposium"), "{config}");
    assert!(!config.contains("pdf-tools@symposium"), "{config}");
}

// ── copilot and gemini ───────────────────────────────────────────────

#[test]
fn copilot_gets_settings_entries_and_a_copy() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tmp.path().join("home");
    let root = tmp.path().join("staging");
    let one = staged_plugin(&root, "pdf-tools");

    let written = Agent::Copilot
        .install_plugins(
            &registration(&root, "symposium", &[&one], Scope::Global),
            &home,
            tmp.path(),
            Duration::ZERO,
        )
        .expect("install");

    let settings = read(&home.join(".copilot/settings.json"));
    assert_eq!(
        settings["extraKnownMarketplaces"]["symposium"]["source"]["path"],
        root.display().to_string()
    );
    assert_eq!(settings["enabledPlugins"]["pdf-tools@symposium"], true);

    let expected = home.join(".copilot/installed-plugins/symposium/pdf-tools");
    assert_eq!(written, vec![expected.clone()]);
    assert!(expected.join("skills/probe/SKILL.md").is_file());
}

#[test]
fn gemini_is_a_copy_with_no_configuration_at_all() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tmp.path().join("home");
    let root = tmp.path().join("staging");
    let one = staged_plugin(&root, "pdf-tools-ab12cd34");

    let written = Agent::Gemini
        .install_plugins(
            &registration(&root, "symposium", &[&one], Scope::Global),
            &home,
            tmp.path(),
            Duration::ZERO,
        )
        .expect("install");

    let expected = home.join(".gemini/extensions/pdf-tools-ab12cd34");
    assert_eq!(
        written,
        vec![expected.clone()],
        "the extension directory is named for the compiled directory, which is what gemini lists"
    );
    assert!(expected.join("skills/probe/SKILL.md").is_file());
    assert!(
        !home.join(".gemini/settings.json").exists(),
        "presence in the folder is the whole installation"
    );
}

#[test]
fn every_copy_carries_the_marker_so_it_can_be_reaped() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tmp.path().join("home");
    let root = tmp.path().join("staging");
    let one = staged_plugin(&root, "pdf-tools");

    for agent in [Agent::Codex, Agent::Copilot, Agent::Gemini] {
        let written = agent
            .install_plugins(
                &registration(&root, "symposium", &[&one], Scope::Global),
                &home,
                tmp.path(),
                Duration::ZERO,
            )
            .expect("install");
        for dir in &written {
            assert!(
                dir.join(crate::sync::MARKER_FILE).is_file(),
                "{agent:?} copy at {} has no marker",
                dir.display()
            );
        }
        assert!(
            !agent.plugin_reap_roots(&home).is_empty(),
            "{agent:?} copies, so it needs a reap root"
        );
    }
    assert!(
        Agent::Claude.plugin_reap_roots(&home).is_empty(),
        "Claude copies nothing, so there is nothing of ours to reap"
    );
}

// ── resilience ───────────────────────────────────────────────────────

#[test]
fn a_corrupt_agent_config_is_an_error_rather_than_a_panic() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tmp.path().join("home");
    let settings = home.join(".claude/settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).expect("create");
    std::fs::write(&settings, "{ this is not json").expect("seed");

    let one = plugin("pdf-tools", "pdf-tools", "1.2.0");
    let result = Agent::Claude.install_plugins(
        &registration(
            &tmp.path().join("staging"),
            "symposium",
            &[&one],
            Scope::Global,
        ),
        &home,
        tmp.path(),
        Duration::ZERO,
    );
    assert!(
        result.is_err(),
        "an unreadable config is reported to the caller, which turns it into a warning"
    );
    assert_eq!(
        std::fs::read_to_string(&settings).expect("read"),
        "{ this is not json",
        "and the file is left exactly as the user had it"
    );
}

#[test]
fn copilots_own_record_is_read_leniently() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tmp.path().join("home");
    assert!(
        copilot_recorded_plugins(&home).is_empty(),
        "no config yet means nothing is recorded"
    );

    let config = home.join(".copilot/config.json");
    std::fs::create_dir_all(config.parent().unwrap()).expect("create");

    std::fs::write(&config, "{ not json at all").expect("seed");
    assert!(
        copilot_recorded_plugins(&home).is_empty(),
        "an unparseable file means nothing is recorded, not a failure"
    );

    // Copilot writes this file itself, with `//` comment lines.
    std::fs::write(
        &config,
        "// This file is managed automatically.\n{\n  \"installedPlugins\": [\n    \
         {\"name\": \"pdf-tools\", \"marketplace\": \"symposium\"},\n    \
         {\"name\": \"other\", \"marketplace\": \"elsewhere\"}\n  ]\n}\n",
    )
    .expect("seed");
    let recorded = copilot_recorded_plugins(&home);
    assert!(recorded.contains("pdf-tools@symposium"));
    assert!(
        recorded.contains("other@elsewhere"),
        "entries we do not own are still read, so they are not treated as missing"
    );
}

#[test]
fn the_config_and_copy_land_without_driving_the_copilot_cli() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tmp.path().join("home");
    let root = tmp.path().join("staging");
    let one = staged_plugin(&root, "pdf-tools");

    // `run_copilot` is a no-op in tests, so this is the whole of what symposium
    // writes for itself: whether Copilot then records the plugin is Copilot's
    // half, and driving its CLI from a unit test is what we do not do.
    let written = Agent::Copilot
        .install_plugins(
            &registration(&root, "symposium", &[&one], Scope::Global),
            &home,
            tmp.path(),
            Duration::ZERO,
        )
        .expect("install");

    assert_eq!(written.len(), 1);
    assert!(home.join(".copilot/settings.json").is_file());
    assert!(written[0].join("skills/probe/SKILL.md").is_file());
}
