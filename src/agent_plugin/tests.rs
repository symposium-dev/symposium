use std::collections::BTreeMap;

use super::*;
use crate::config::UseEntry;
use crate::plugins::{Plugin, PluginSource, SkillGroup};
use crate::pm::{ANY_VERSION, PackageId};
use crate::predicate::{Predicate, PredicateSet};
use crate::skills::Skill;

fn wildcard() -> PredicateSet {
    PredicateSet::from_depends_on("*").expect("wildcard")
}

fn on_serde() -> PredicateSet {
    PredicateSet::from_depends_on("serde").expect("serde")
}

fn registry_plugin(name: &str, predicates: PredicateSet) -> ParsedPlugin {
    ParsedPlugin {
        plugin: Plugin {
            name: name.to_string(),
            predicates,
            ..Default::default()
        },
        workspace_member: false,
        canonical: PackageId::new("user-plugins", name, ANY_VERSION),
    }
}

fn skill_of(plugin: &ParsedPlugin, name: &str, path: &str) -> SkillWithGroupContext {
    SkillWithGroupContext {
        skill: Skill {
            frontmatter: BTreeMap::from([("name".to_string(), name.to_string())]),
            predicates: PredicateSet::default(),
            path: PathBuf::from(path),
        },
        origin_hash: crate::skills::hash_origin_key(&path),
        plugin: plugin.plugin.name.clone(),
        plugin_id: plugin.canonical.clone(),
    }
}

fn no_config() -> PluginsConfig {
    PluginsConfig::default()
}

// ── scope ────────────────────────────────────────────────────────────

fn used_globally(name: &str) -> PluginsConfig {
    PluginsConfig {
        used: vec![UseEntry::Global(name.to_string())],
        ..Default::default()
    }
}

#[test]
fn global_needs_both_a_global_use_entry_and_a_workspace_independent_gate() {
    let plugin = registry_plugin("pdf-tools", wildcard());
    assert_eq!(
        Scope::of(&plugin, &[], &no_config()),
        Scope::Project,
        "a workspace-independent gate is not on its own a request to install for the user"
    );
    assert_eq!(
        Scope::of(&plugin, &[], &used_globally("pdf-tools")),
        Scope::Global
    );

    let dep_gated = registry_plugin("pdf-tools", on_serde());
    assert_eq!(
        Scope::of(&dep_gated, &[], &used_globally("pdf-tools")),
        Scope::Project,
        "a global entry on a workspace-dependent plugin installs per project instead"
    );
}

#[test]
fn a_concrete_dependency_gate_keeps_a_plugin_project_scoped() {
    let plugin = registry_plugin("pdf-tools", on_serde());
    assert_eq!(Scope::of(&plugin, &[], &no_config()), Scope::Project);
}

#[test]
fn workspace_members_and_crate_plugins_are_project_scoped() {
    let mut member = registry_plugin("house-style", wildcard());
    member.workspace_member = true;
    assert_eq!(
        Scope::of(&member, &[], &used_globally("house-style")),
        Scope::Project,
        "membership is what activates a workspace plugin, so it cannot be global"
    );

    let mut from_crate = registry_plugin("widget", wildcard());
    from_crate.canonical = PackageId::new("cargo", "widget", "1.0.0");
    assert_eq!(
        Scope::of(&from_crate, &[], &used_globally("widget")),
        Scope::Project,
        "a crate plugin is reached through this workspace's dependency graph"
    );
}

#[test]
fn a_dormant_plugin_goes_global_only_when_used_globally() {
    let mut dormant = registry_plugin("pdf-tools", PredicateSet::default());
    dormant.plugin.requires_use = true;

    assert_eq!(Scope::of(&dormant, &[], &no_config()), Scope::Project);

    let workspace_scoped = PluginsConfig {
        used: vec![UseEntry::Workspace {
            name: "pdf-tools".into(),
            workspace: PathBuf::from("/work/reporter"),
        }],
        ..Default::default()
    };
    assert_eq!(
        Scope::of(&dormant, &[], &workspace_scoped),
        Scope::Project,
        "a workspace `use` entry is workspace-dependent by definition"
    );

    let globally = PluginsConfig {
        used: vec![UseEntry::Global("pdf_tools".into())],
        ..Default::default()
    };
    assert_eq!(
        Scope::of(&dormant, &[], &globally),
        Scope::Global,
        "global `use` names match hyphen/underscore-insensitively"
    );
}

#[test]
fn a_dependency_gated_group_or_skill_keeps_the_plugin_project_scoped() {
    let globally = used_globally("pdf-tools");
    let mut grouped = registry_plugin("pdf-tools", wildcard());
    grouped.plugin.skills = vec![SkillGroup {
        predicates: on_serde(),
        source: PluginSource::Path(PathBuf::from("skills")),
        ..Default::default()
    }];
    assert_eq!(Scope::of(&grouped, &[], &globally), Scope::Project);

    let plugin = registry_plugin("pdf-tools", wildcard());
    let mut gated = skill_of(&plugin, "extract-tables", "/reg/pdf/skills/x/SKILL.md");
    gated.skill.predicates = on_serde();
    assert_eq!(
        Scope::of(&plugin, &[&gated], &globally),
        Scope::Project,
        "a dep-gated skill makes the compiled content vary by workspace"
    );
}

#[test]
fn shell_and_path_predicates_are_treated_as_workspace_dependent() {
    let set = PredicateSet {
        predicates: vec![Predicate::Shell("true".into())],
    };
    assert!(!set.is_workspace_independent());

    assert!(wildcard().is_workspace_independent());
    assert!(PredicateSet::default().is_workspace_independent());
    assert!(!on_serde().is_workspace_independent());
}

// ── compile ──────────────────────────────────────────────────────────

#[test]
fn one_bundle_referenced_by_two_plugins_is_emitted_once() {
    let first = registry_plugin("pdf-tools", wildcard());
    let second = registry_plugin("csv-tools", wildcard());
    let shared = "/reg/shared/skills/extract/SKILL.md";
    let skills = vec![
        skill_of(&first, "extract", shared),
        skill_of(&second, "extract", shared),
        skill_of(&second, "split-rows", "/reg/csv/skills/split/SKILL.md"),
    ];

    let compiled = compile(&[first, second], &skills, &no_config());
    assert_eq!(
        compiled[0].skills.len(),
        1,
        "the first plugin to claim the bundle carries it"
    );
    let second_dirs: Vec<&str> = compiled[1]
        .skills
        .iter()
        .map(|s| s.dir_name.as_str())
        .collect();
    assert_eq!(
        second_dirs,
        vec!["split-rows"],
        "the second plugin keeps its own skills but not a second copy of the shared one"
    );
}

#[test]
fn skills_are_grouped_under_the_plugin_that_contributed_them() {
    let one = registry_plugin("pdf-tools", wildcard());
    let two = registry_plugin("csv-tools", wildcard());
    let skills = vec![
        skill_of(&one, "extract-tables", "/reg/pdf/skills/extract/SKILL.md"),
        skill_of(&one, "read-forms", "/reg/pdf/skills/forms/SKILL.md"),
        skill_of(&two, "split-rows", "/reg/csv/skills/split/SKILL.md"),
    ];

    let compiled = compile(&[one, two], &skills, &no_config());
    let names: Vec<(&str, usize)> = compiled
        .iter()
        .map(|p| (p.dir_name.as_str(), p.skills.len()))
        .collect();
    assert_eq!(names, vec![("pdf-tools", 2), ("csv-tools", 1)]);
}

#[test]
fn a_plugin_with_no_applicable_skills_compiles_to_nothing() {
    let plugin = registry_plugin("pdf-tools", wildcard());
    assert!(compile(&[plugin], &[], &no_config()).is_empty());
}

#[test]
fn names_that_slug_alike_are_both_suffixed() {
    let underscored = registry_plugin("pdf_tools", wildcard());
    let hyphenated = registry_plugin("pdf-tools", wildcard());
    let skills = vec![
        skill_of(&underscored, "a", "/reg/one/skills/a/SKILL.md"),
        skill_of(&hyphenated, "b", "/reg/two/skills/b/SKILL.md"),
    ];

    let compiled = compile(&[underscored, hyphenated], &skills, &no_config());
    assert_eq!(compiled.len(), 2);
    for plugin in &compiled {
        assert!(
            plugin.dir_name.starts_with("pdf-tools-"),
            "expected a suffixed name, got {}",
            plugin.dir_name
        );
        assert!(manifest::is_valid_name(&plugin.dir_name));
    }
    assert_ne!(compiled[0].dir_name, compiled[1].dir_name);
    for plugin in &compiled {
        assert_eq!(
            plugin.manifest.name, plugin.dir_name,
            "agents key a plugin by its manifest name, so it is suffixed too"
        );
        assert!(manifest::is_valid_name(&plugin.manifest.name));
    }
    assert_ne!(compiled[0].manifest.name, compiled[1].manifest.name);
}

#[test]
fn a_suffixed_name_stays_within_the_length_limit() {
    let long = "x".repeat(64);
    let a = registry_plugin(&long, wildcard());
    let b = registry_plugin(&format!("{long}!"), wildcard());
    let skills = vec![
        skill_of(&a, "a", "/reg/one/skills/a/SKILL.md"),
        skill_of(&b, "b", "/reg/two/skills/b/SKILL.md"),
    ];

    let compiled = compile(&[a, b], &skills, &no_config());
    assert_eq!(compiled.len(), 2);
    for plugin in &compiled {
        assert!(
            manifest::is_valid_name(&plugin.manifest.name),
            "{} is not a valid manifest name",
            plugin.manifest.name
        );
    }
    assert_ne!(compiled[0].manifest.name, compiled[1].manifest.name);
}

#[test]
fn a_plugin_whose_skills_were_all_claimed_compiles_to_nothing() {
    let first = registry_plugin("first", wildcard());
    let second = registry_plugin("second", wildcard());
    let shared = "/reg/shared/skills/guide/SKILL.md";
    let skills = vec![
        skill_of(&first, "guide", shared),
        skill_of(&second, "guide", shared),
    ];

    let compiled = compile(&[first, second], &skills, &no_config());
    assert_eq!(
        compiled.len(),
        1,
        "the second plugin has nothing left after dedup, so it must not be emitted"
    );
    assert_eq!(compiled[0].manifest.name, "first");
}

#[test]
fn one_skill_reached_twice_through_a_plugin_is_compiled_once() {
    let plugin = registry_plugin("pdf-tools", wildcard());
    let once = skill_of(&plugin, "extract", "/reg/pdf/skills/extract/SKILL.md");
    let twice = skill_of(&plugin, "extract", "/reg/pdf/skills/extract/SKILL.md");
    let compiled = compile(&[plugin], &[once, twice], &no_config());
    assert_eq!(compiled[0].skills.len(), 1);
}

#[test]
fn same_named_skills_from_different_paths_both_survive_with_suffixes() {
    let plugin = registry_plugin("pdf-tools", wildcard());
    let skills = vec![
        skill_of(&plugin, "extract", "/reg/pdf/a/SKILL.md"),
        skill_of(&plugin, "extract", "/reg/pdf/b/SKILL.md"),
    ];
    let compiled = compile(&[plugin], &skills, &no_config());
    let dirs: Vec<&str> = compiled[0]
        .skills
        .iter()
        .map(|s| s.dir_name.as_str())
        .collect();
    assert_eq!(dirs.len(), 2);
    assert!(dirs.iter().all(|d| d.starts_with("extract-")), "{dirs:?}");
    assert_ne!(dirs[0], dirs[1]);
}

#[test]
fn the_version_comes_from_the_manifest_then_the_resolved_crate() {
    let mut declared = registry_plugin("pdf-tools", wildcard());
    declared.plugin.version = Some("1.2.0".into());
    assert_eq!(version_of(&declared), "1.2.0");

    let mut from_crate = registry_plugin("widget", wildcard());
    from_crate.canonical = PackageId::new("cargo", "widget", "0.3.1");
    assert_eq!(version_of(&from_crate), "0.3.1");

    let placeholder = registry_plugin("pdf-tools", wildcard());
    assert_eq!(
        version_of(&placeholder),
        UNVERSIONED,
        "the `*` placeholder is not a version, and Codex keys its cache on one"
    );
}

// ── write and reap ───────────────────────────────────────────────────

fn skill_on_disk(dir: &Path, name: &str, body: &str) -> PathBuf {
    let skill_dir = dir.join(name);
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: d\n---\n{body}\n"),
    )
    .expect("write SKILL.md");
    skill_dir.join("SKILL.md")
}

#[test]
fn write_produces_a_manifest_beside_the_skills() {
    let tmp = tempfile::tempdir().expect("tmp");
    let source = tmp.path().join("source");
    let skill_md = skill_on_disk(&source, "extract", "body");

    let compiled = CompiledPlugin {
        source_id: PackageId::new("test", "src", ANY_VERSION),
        dir_name: "pdf-tools".into(),
        manifest: Manifest::new("pdf-tools".into(), "1.2.0".into(), None),
        scope: Scope::Global,
        skills: vec![CompiledSkill {
            dir_name: "extract".into(),
            source_dir: skill_md.parent().unwrap().to_path_buf(),
        }],
    };

    let root = tmp.path().join("staging");
    let dest = write(&compiled, &root, tmp.path(), Duration::ZERO).expect("write");

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dest.join("plugin.json")).expect("read manifest"))
            .expect("parse manifest");
    assert_eq!(manifest["name"], "pdf-tools");
    assert_eq!(manifest["version"], "1.2.0");
    assert_eq!(manifest["$schema"], manifest::SCHEMA_URL);

    let claude: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(dest.join(".claude-plugin/plugin.json")).expect("read claude manifest"),
    )
    .expect("parse claude manifest");
    assert_eq!(
        claude, manifest,
        "Claude Code reads its own path but the same content"
    );

    let gemini: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(dest.join("gemini-extension.json")).expect("read gemini manifest"),
    )
    .expect("parse gemini manifest");
    assert_eq!(gemini["name"], "pdf-tools");
    assert_eq!(gemini["version"], "1.2.0");

    assert!(dest.join("skills/extract/SKILL.md").is_file());
    assert!(
        dest.join(crate::sync::MARKER_FILE).is_file(),
        "compiled dirs carry the ownership marker so cleanup can find them"
    );
    assert!(
        !dest.join(".gitignore").exists(),
        "the staging root carries the only .gitignore"
    );
}

#[test]
fn rewriting_identical_content_leaves_the_directory_untouched() {
    let tmp = tempfile::tempdir().expect("tmp");
    let source = tmp.path().join("source");
    let skill_md = skill_on_disk(&source, "extract", "body");
    let compiled = CompiledPlugin {
        source_id: PackageId::new("test", "src", ANY_VERSION),
        dir_name: "pdf-tools".into(),
        manifest: Manifest::new("pdf-tools".into(), UNVERSIONED.into(), None),
        scope: Scope::Global,
        skills: vec![CompiledSkill {
            dir_name: "extract".into(),
            source_dir: skill_md.parent().unwrap().to_path_buf(),
        }],
    };
    let root = tmp.path().join("staging");

    let dest = write(&compiled, &root, tmp.path(), Duration::ZERO).expect("first write");
    let installed = dest.join("skills/extract/SKILL.md");
    let before = fs::metadata(&installed)
        .and_then(|m| m.modified())
        .expect("mtime");

    write(&compiled, &root, tmp.path(), Duration::ZERO).expect("second write");
    let after = fs::metadata(&installed)
        .and_then(|m| m.modified())
        .expect("mtime");
    assert_eq!(before, after, "unchanged content must not be recopied");

    fs::write(
        skill_md,
        "---\nname: extract\ndescription: d\n---\nchanged\n",
    )
    .expect("edit");
    write(&compiled, &root, tmp.path(), Duration::ZERO).expect("third write");
    assert!(
        fs::read_to_string(&installed)
            .expect("read")
            .contains("changed"),
        "changed content must be recopied"
    );
}

#[test]
fn reap_removes_marked_directories_and_leaves_user_ones_alone() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().join("staging");
    let kept = root.join("kept");
    let stale = root.join("stale");
    let user = root.join("user-authored");
    for dir in [&kept, &stale, &user] {
        fs::create_dir_all(dir).expect("create");
    }
    for dir in [&kept, &stale] {
        fs::write(dir.join(crate::sync::MARKER_FILE), "").expect("marker");
    }

    reap(&root, &std::collections::BTreeSet::from([kept.clone()]));

    assert!(kept.is_dir(), "a directory written this run stays");
    assert!(
        !stale.exists(),
        "a marked directory we did not write is reaped"
    );
    assert!(user.is_dir(), "an unmarked directory is never touched");
}

#[test]
fn a_plugin_with_no_version_is_emitted_as_unversioned() {
    let tmp = tempfile::tempdir().expect("tmp");
    let skill_md = skill_on_disk(&tmp.path().join("source"), "extract", "body");
    let compiled = CompiledPlugin {
        source_id: PackageId::new("test", "src", ANY_VERSION),
        dir_name: "pdf-tools".into(),
        manifest: Manifest::new("pdf-tools".into(), UNVERSIONED.into(), None),
        scope: Scope::Global,
        skills: vec![CompiledSkill {
            dir_name: "extract".into(),
            source_dir: skill_md.parent().unwrap().to_path_buf(),
        }],
    };
    let dest = write(
        &compiled,
        &tmp.path().join("staging"),
        tmp.path(),
        Duration::ZERO,
    )
    .expect("write");

    for file in ["plugin.json", "gemini-extension.json"] {
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dest.join(file)).expect("read"))
                .expect("json");
        assert_eq!(
            json["version"], UNVERSIONED,
            "{file} needs a version even when the plugin declares none, since Codex keys its \
             cache directory on one"
        );
    }
}

#[test]
fn the_marketplace_index_lists_each_plugin_and_is_removed_when_empty() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().join("staging");
    fs::create_dir_all(&root).expect("create root");

    let one = CompiledPlugin {
        source_id: PackageId::new("test", "src", ANY_VERSION),
        dir_name: "pdf-tools-ab12cd34".into(),
        manifest: Manifest::new(
            "pdf-tools".into(),
            UNVERSIONED.into(),
            Some("Tables".into()),
        ),
        scope: Scope::Global,
        skills: Vec::new(),
    };
    let two = CompiledPlugin {
        source_id: PackageId::new("test", "src", ANY_VERSION),
        dir_name: "csv-tools".into(),
        manifest: Manifest::new("csv-tools".into(), UNVERSIONED.into(), None),
        scope: Scope::Global,
        skills: Vec::new(),
    };

    write_marketplace(&root, "symposium", &[&one, &two]).expect("write index");
    let file = root.join(".claude-plugin/marketplace.json");
    let index: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).expect("read")).expect("json");
    assert_eq!(index["name"], "symposium");
    assert_eq!(
        index["plugins"][0]["source"], "./pdf-tools-ab12cd34",
        "the entry points at the directory, which may be disambiguated"
    );
    assert_eq!(
        index["plugins"][0]["name"], "pdf-tools",
        "while the plugin keeps its declared name"
    );
    assert_eq!(index["plugins"][1]["name"], "csv-tools");
    assert!(index["plugins"][1].get("description").is_none());

    write_marketplace(&root, "symposium", &[]).expect("remove index");
    assert!(
        !file.exists(),
        "a root with no compiled plugins must not advertise a marketplace"
    );
}

#[test]
fn a_project_marketplace_is_named_per_workspace() {
    let global = marketplace_name(Scope::Global, Some(Path::new("/work/reporter")));
    assert_eq!(global, "symposium");
    assert_eq!(
        marketplace_name(Scope::Global, None),
        "symposium",
        "the global root needs no project to name it"
    );

    let one = marketplace_name(Scope::Project, Some(Path::new("/work/reporter")));
    let two = marketplace_name(Scope::Project, Some(Path::new("/elsewhere/reporter")));
    assert!(one.starts_with("symposium-reporter-"), "{one}");
    assert_ne!(
        one, two,
        "registration is user-level, so two projects must not claim one name"
    );
    for name in [&global, &one, &two] {
        assert!(manifest::is_valid_name(name), "{name}");
    }
}

#[test]
fn a_plugin_whose_name_cannot_be_slugged_is_skipped() {
    let unnameable = registry_plugin("___", wildcard());
    let skills = vec![skill_of(
        &unnameable,
        "guidance",
        "/reg/x/skills/g/SKILL.md",
    )];
    assert!(
        compile(&[unnameable], &skills, &no_config()).is_empty(),
        "a package with no usable name cannot be installed anywhere, so it is dropped"
    );
}

#[test]
fn one_unnameable_plugin_does_not_stop_the_others() {
    let bad = registry_plugin("!!!", wildcard());
    let good = registry_plugin("pdf-tools", wildcard());
    let skills = vec![
        skill_of(&bad, "lost", "/reg/bad/skills/lost/SKILL.md"),
        skill_of(&good, "extract", "/reg/good/skills/extract/SKILL.md"),
    ];
    let compiled = compile(&[bad, good], &skills, &no_config());
    let names: Vec<&str> = compiled.iter().map(|p| p.dir_name.as_str()).collect();
    assert_eq!(names, vec!["pdf-tools"]);
}
