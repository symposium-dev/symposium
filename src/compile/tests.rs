use super::*;

use std::collections::BTreeMap;
use std::time::SystemTime;

use expect_test::expect;
use symposium_sdk::manifest::RawPluginManifest;

use crate::config::UseEntry;
use crate::pm::{ANY_VERSION, PackageId, PluginKind, UnvalidatedPlugin};
use crate::predicate::PredicateSet;
use crate::skills::Skill;

fn validated(kind: PluginKind, pm: &str, name: &str, manifest: &str) -> Plugin {
    crate::plugins::validate_plugin(
        UnvalidatedPlugin::new(
            PackageId::new(pm, name, ANY_VERSION),
            "/nonexistent",
            RawPluginManifest::parse(manifest).unwrap(),
        ),
        kind,
    )
    .unwrap()
}

fn registry(name: &str, gate: &str) -> Plugin {
    validated(
        PluginKind::Registry,
        "user-plugins",
        name,
        &format!("name = \"{name}\"\n{gate}"),
    )
}

fn global(name: &str) -> UseEntry {
    UseEntry::Global(name.into())
}

fn skill(name: &str, origin: &str, skill_md: &Path, plugin_index: usize) -> SkillWithGroupContext {
    SkillWithGroupContext {
        skill: Skill {
            frontmatter: BTreeMap::from([("name".to_string(), name.to_string())]),
            predicates: PredicateSet::default(),
            path: skill_md.to_path_buf(),
        },
        origin_hash: origin.into(),
        plugin_index,
    }
}

fn write_skill(dir: &Path, name: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let skill_md = dir.join("SKILL.md");
    fs::write(
        &skill_md,
        format!("---\nname: {name}\ndescription: {name} guidance\n---\n\nBody of {name}.\n"),
    )
    .unwrap();
    skill_md
}

fn tree(dir: &Path) -> String {
    let mut lines = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current).unwrap().flatten() {
            let path = entry.path();
            let rel = path
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                lines.push(format!("{rel}/"));
                stack.push(path);
            } else {
                lines.push(rel);
            }
        }
    }
    lines.sort();
    lines.join("\n")
}

fn is_valid_plugin_name(name: &str) -> bool {
    let grammar = regex::Regex::new("^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$").unwrap();
    (1..=MAX_NAME_LEN).contains(&name.len()) && grammar.is_match(name) && !name.contains("--")
}

#[test]
fn only_a_global_use_entry_naming_the_plugin_installs_it_for_the_user() {
    let mut crate_plugin = validated(PluginKind::Package, "cargo", "crate_a", "");
    crate_plugin.manifest.name = "a-tools".into();
    let mut member = registry("house-style", "");
    member.workspace_member = true;
    let workspace_use = UseEntry::Workspace {
        name: "dormant-guide".into(),
        workspace: "/ws".into(),
    };

    let cases = [
        (
            "global use on a dormant registry plugin",
            registry("dormant-guide", ""),
            vec![global("dormant-guide")],
            true,
        ),
        (
            "global use on a crate, by its crate name",
            crate_plugin,
            vec![global("crate-a")],
            true,
        ),
        (
            "workspace use",
            registry("dormant-guide", ""),
            vec![workspace_use],
            false,
        ),
        (
            "registry plugin active by depends-on(*), no use",
            registry("always-guide", "depends-on = [\"*\"]"),
            vec![],
            false,
        ),
        (
            "dependency-gated registry plugin, no use",
            registry("serde-guide", "depends-on = [\"serde\"]"),
            vec![],
            false,
        ),
        (
            "workspace member named by a global use",
            member,
            vec![global("house-style")],
            false,
        ),
    ];
    for (label, plugin, used, expected) in cases {
        let config = PluginsConfig {
            used,
            ..Default::default()
        };
        assert_eq!(installs_globally(&config, &plugin), expected, "{label}");
    }
}

#[test]
fn names_normalize_to_what_every_agent_accepts() {
    let long = "a".repeat(70);
    let cases: Vec<(&str, Option<String>)> = vec![
        ("serde_json", Some("serde-json".into())),
        ("Serde", Some("serde".into())),
        ("pdf.tools", Some("pdf-tools".into())),
        ("a--b", Some("a-b".into())),
        ("-edges-", Some("edges".into())),
        ("café au lait", Some("caf-au-lait".into())),
        (&long, Some("a".repeat(64))),
        ("___", None),
        ("日本", None),
        ("", None),
    ];
    for (input, expected) in cases {
        let normalized = normalize_name(input);
        assert_eq!(normalized, expected, "{input:?}");
        if let Some(name) = normalized {
            assert!(is_valid_plugin_name(&name), "{name:?}");
        }
    }

    let truncated = normalize_name(&format!("{}_tail", "a".repeat(63))).unwrap();
    assert_eq!(
        truncated,
        "a".repeat(63),
        "no trailing `-` after truncation"
    );
}

#[test]
fn colliding_names_are_all_suffixed_and_unsluggable_ones_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("mine")).unwrap();
    let md = root.join("SKILL.md");
    let skills = [
        skill("s1", "11111111", &md, 0),
        skill("s2", "22222222", &md, 1),
        skill("s3", "33333333", &md, 2),
        skill("s4", "44444444", &md, 3),
        skill("s5", "55555555", &md, 4),
        skill("s6", "66666666", &md, 5),
        skill("s7", "77777777", &md, 6),
    ];
    let long_lower = "a".repeat(70);
    let long_upper = "A".repeat(70);
    let names = [
        "serde_json",
        "serde-json",
        "___",
        "plain",
        "mine",
        long_lower.as_str(),
        long_upper.as_str(),
    ];
    let plugins: Vec<GlobalPlugin> = names
        .iter()
        .zip(&skills)
        .map(|(name, skill)| GlobalPlugin {
            name,
            skills: vec![skill],
        })
        .collect();

    let assigned = plugin_dir_names(root, &plugins);

    let serde = assigned[0].as_deref().unwrap();
    let serde_alike = assigned[1].as_deref().unwrap();
    assert!(serde.starts_with("serde-json-") && serde_alike.starts_with("serde-json-"));
    assert_ne!(serde, serde_alike);
    assert_eq!(assigned[2], None);
    assert_eq!(assigned[3].as_deref(), Some("plain"));
    assert!(
        assigned[4].as_deref().unwrap().starts_with("mine-"),
        "a directory the user owns keeps the plain name: {assigned:?}"
    );
    let (lower, upper) = (
        assigned[5].as_deref().unwrap(),
        assigned[6].as_deref().unwrap(),
    );
    assert_ne!(lower, upper);
    for name in assigned.iter().flatten() {
        assert!(is_valid_plugin_name(name), "{name:?}");
    }
}

#[test]
fn compile_writes_one_self_contained_directory_per_plugin() {
    let tmp = tempfile::tempdir().unwrap();
    let sources = tmp.path().join("sources");
    let root = tmp.path().join("installed");
    let nested = write_skill(&sources.join("deep/nested/alpha"), "alpha");
    fs::create_dir_all(sources.join("deep/nested/alpha/reference")).unwrap();
    fs::write(
        sources.join("deep/nested/alpha/reference/notes.md"),
        "notes",
    )
    .unwrap();
    let dup_one = write_skill(&sources.join("one/dup"), "dup");
    let dup_two = write_skill(&sources.join("two/dup"), "dup");
    let escaping = write_skill(&sources.join("escaping"), "escaping");

    let skills = [
        skill("alpha", "aaaaaaaa", &nested, 0),
        skill("dup", "11111111", &dup_one, 0),
        skill("dup", "22222222", &dup_two, 0),
        skill("../escape", "eeeeeeee", &escaping, 0),
    ];
    let plugins = [GlobalPlugin {
        name: "Guide_Tools",
        skills: skills.iter().collect(),
    }];

    let compiled = compile(&root, &plugins, Duration::ZERO);

    expect![[r#"
        .claude-plugin/
        .claude-plugin/marketplace.json
        guide-tools/
        guide-tools/.claude-plugin/
        guide-tools/.claude-plugin/plugin.json
        guide-tools/.symposium
        guide-tools/plugin.json
        guide-tools/skills/
        guide-tools/skills/alpha/
        guide-tools/skills/alpha/SKILL.md
        guide-tools/skills/alpha/reference/
        guide-tools/skills/alpha/reference/notes.md
        guide-tools/skills/dup-11111111/
        guide-tools/skills/dup-11111111/SKILL.md
        guide-tools/skills/dup-22222222/
        guide-tools/skills/dup-22222222/SKILL.md"#]]
    .assert_eq(&tree(&root));
    assert_eq!(
        compiled.plugins,
        vec![CompiledPlugin {
            name: "guide-tools".into(),
            version: "0.0.0".into(),
            dir: root.join("guide-tools"),
        }]
    );
    assert_eq!(
        compiled.skill_origins,
        BTreeSet::from(["aaaaaaaa".into(), "11111111".into(), "22222222".into()]),
        "the skipped skill is left to the per-skill path"
    );
    assert_eq!(
        fs::read(root.join("guide-tools/plugin.json")).unwrap(),
        fs::read(root.join("guide-tools/.claude-plugin/plugin.json")).unwrap(),
    );
    assert_eq!(
        fs::read_to_string(root.join("guide-tools/skills/alpha/SKILL.md")).unwrap(),
        fs::read_to_string(&nested).unwrap(),
        "skill files are copied verbatim"
    );
}

#[cfg(unix)]
#[test]
fn compile_follows_symlinks_in_skill_content() {
    let tmp = tempfile::tempdir().unwrap();
    let skill_md = write_skill(&tmp.path().join("src/linked"), "linked");
    fs::write(tmp.path().join("shared.md"), "shared").unwrap();
    std::os::unix::fs::symlink("../../shared.md", tmp.path().join("src/linked/shared.md")).unwrap();
    let root = tmp.path().join("installed");
    let skills = [skill("linked", "aaaaaaaa", &skill_md, 0)];

    compile(
        &root,
        &[GlobalPlugin {
            name: "links",
            skills: skills.iter().collect(),
        }],
        Duration::ZERO,
    );

    let copied = root.join("links/skills/linked/shared.md");
    assert!(!fs::symlink_metadata(&copied).unwrap().is_symlink());
    assert_eq!(fs::read_to_string(copied).unwrap(), "shared");
}

#[test]
fn recompiling_writes_only_what_changed_and_reaps_what_left() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("installed");
    let keep_md = write_skill(&tmp.path().join("src/keep"), "keep");
    let gone_md = write_skill(&tmp.path().join("src/gone"), "gone");
    let skills = [
        skill("keep", "11111111", &keep_md, 0),
        skill("gone", "22222222", &gone_md, 1),
    ];
    let keep = GlobalPlugin {
        name: "keep",
        skills: vec![&skills[0]],
    };
    let gone = GlobalPlugin {
        name: "gone",
        skills: vec![&skills[1]],
    };
    fs::create_dir_all(root.join("users-own")).unwrap();

    compile(&root, &[keep, gone], Duration::ZERO);
    let copied = root.join("keep/skills/keep/SKILL.md");
    let an_hour_ago = SystemTime::now() - Duration::from_secs(3600);
    fs::File::options()
        .write(true)
        .open(&copied)
        .unwrap()
        .set_modified(an_hour_ago)
        .unwrap();

    let keep = GlobalPlugin {
        name: "keep",
        skills: vec![&skills[0]],
    };
    compile(&root, &[keep], Duration::ZERO);

    assert_eq!(
        fs::metadata(&copied).unwrap().modified().unwrap(),
        an_hour_ago,
        "unchanged content is not rewritten"
    );
    assert!(!root.join("gone").exists());
    assert!(
        root.join("users-own").exists(),
        "unmarked directories are not ours"
    );
    let index: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join(".claude-plugin/marketplace.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        index["plugins"],
        json!([{ "name": "keep", "source": "./keep" }])
    );

    fs::write(&keep_md, "---\nname: keep\ndescription: edited\n---\n").unwrap();
    let keep = GlobalPlugin {
        name: "keep",
        skills: vec![&skills[0]],
    };
    compile(&root, &[keep], Duration::ZERO);
    assert!(fs::read_to_string(&copied).unwrap().contains("edited"));

    compile(&root, &[], Duration::ZERO);
    assert!(!root.join("keep").exists());
    assert!(
        !root.join(".claude-plugin").exists(),
        "an empty root has no index"
    );
    assert!(root.join("users-own").exists());
}

#[test]
fn a_skill_reached_by_two_global_plugins_compiles_into_the_first() {
    let tmp = tempfile::tempdir().unwrap();
    let skill_md = write_skill(&tmp.path().join("shared"), "shared");
    let active = [
        registry("first", ""),
        registry("second", ""),
        registry("project", ""),
    ];
    let deduplicated = [skill("shared", "11111111", &skill_md, 0)];
    let config = PluginsConfig {
        used: vec![global("first"), global("second")],
        ..Default::default()
    };

    let grouped = global_plugins(&config, &active, &deduplicated.iter().collect::<Vec<_>>());

    assert_eq!(grouped.len(), 1, "the plugin dedup emptied is dropped");
    assert_eq!(grouped[0].name, "first");
}

#[test]
fn manifests_follow_the_agent_plugins_schema() {
    expect![[r#"
        {
          "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
          "name": "pdf-tools",
          "version": "0.0.0"
        }"#]]
    .assert_eq(&serde_json::to_string_pretty(&manifest("pdf-tools")).unwrap());

    let allowed = [
        "$schema",
        "name",
        "version",
        "description",
        "author",
        "homepage",
        "repository",
        "license",
        "keywords",
        "extensions",
    ];
    for input in ["serde_json", "Pdf.Tools", "a--b", &"x".repeat(80)] {
        let name = normalize_name(input).unwrap();
        let rendered = manifest(&name);
        let fields = rendered.as_object().unwrap();
        assert_eq!(
            fields["$schema"], AGENT_PLUGINS_SCHEMA,
            "required `$schema`"
        );
        assert!(
            is_valid_plugin_name(fields["name"].as_str().unwrap()),
            "required `name`"
        );
        assert!(
            fields.keys().all(|key| allowed.contains(&key.as_str())),
            "closed field set: {fields:?}"
        );
    }
}

#[test]
fn the_marketplace_index_lists_every_plugin_by_relative_path() {
    let plugin = |name: &str| CompiledPlugin {
        name: name.into(),
        version: PLUGIN_VERSION.into(),
        dir: PathBuf::from("/installed").join(name),
    };
    expect![[r#"
        {
          "name": "symposium",
          "owner": {
            "name": "symposium"
          },
          "plugins": [
            {
              "name": "alpha",
              "source": "./alpha"
            },
            {
              "name": "beta",
              "source": "./beta"
            }
          ]
        }"#]]
    .assert_eq(
        &serde_json::to_string_pretty(&marketplace(&[plugin("beta"), plugin("alpha")])).unwrap(),
    );
}
