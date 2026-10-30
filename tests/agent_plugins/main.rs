mod claude;
mod codex;
mod copilot;

use std::fs;
use std::path::{Path, PathBuf};

use symposium::config::{RegistryConfig, UseEntry};
use symposium_testlib::{TestMode, with_fixture};

pub(crate) const SCOPES: &[&str] = &["agent-plugin-scopes0"];

struct DeliveringAgent {
    name: &'static str,
    project_skills: &'static str,
    workspace_skill: &'static str,
    relocation_var: Option<&'static str>,
    config_root: fn(&Path) -> PathBuf,
    plugin_dir: fn(&Path, &str) -> PathBuf,
}

const AGENTS: &[DeliveringAgent] = &[
    DeliveringAgent {
        name: "claude",
        project_skills: ".claude/skills",
        workspace_skill: ".claude/skills/global-guide/skills/global-guide/SKILL.md",
        relocation_var: Some("CLAUDE_CONFIG_DIR"),
        config_root: |home| home.join(".claude"),
        plugin_dir: |root, plugin| root.join("skills").join(plugin),
    },
    DeliveringAgent {
        name: "codex",
        project_skills: ".agents/skills",
        workspace_skill: ".agents/skills/global-guide/SKILL.md",
        relocation_var: Some("CODEX_HOME"),
        config_root: |home| home.join(".codex"),
        plugin_dir: |root, plugin| root.join(format!("plugins/cache/symposium/{plugin}/0.0.0")),
    },
    DeliveringAgent {
        name: "copilot",
        project_skills: ".agents/skills",
        workspace_skill: ".agents/skills/global-guide/SKILL.md",
        relocation_var: Some("COPILOT_HOME"),
        config_root: |home| home.join(".copilot"),
        plugin_dir: copilot::plugin_dir,
    },
    DeliveringAgent {
        name: "antigravity",
        project_skills: ".agents/skills",
        workspace_skill: ".agents/skills/global-guide/SKILL.md",
        relocation_var: None,
        config_root: |home| home.join(".gemini/config"),
        plugin_dir: |root, plugin| root.join("plugins").join(plugin),
    },
    DeliveringAgent {
        name: "goose",
        project_skills: ".agents/skills",
        workspace_skill: ".agents/skills/global-guide/SKILL.md",
        relocation_var: Some("GOOSE_PATH_ROOT"),
        config_root: |home| home.to_path_buf(),
        plugin_dir: |root, plugin| root.join(".agents/plugins").join(plugin),
    },
];

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

#[tokio::test]
async fn every_delivering_agent_keeps_the_delivery_contracts() {
    for agent in AGENTS {
        with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
            let name = agent.name;
            let root = (agent.config_root)(ctx.sym.home_dir());
            let installed =
                || (agent.plugin_dir)(&root, "global-guide").join("skills/global-guide/SKILL.md");
            let users_own = (agent.plugin_dir)(&root, "users-own").join("notes.md");
            let workspace = ctx.workspace_root.clone().unwrap();
            let project_copy = workspace
                .join(agent.project_skills)
                .join("global-guide/SKILL.md");
            write(&users_own, "mine");

            ctx.symposium(&["init", "--add-agent", name]).await?;
            ctx.symposium(&["sync"]).await?;
            assert!(
                installed().is_file(),
                "{name}: the plugin reaches the agent"
            );
            assert!(!project_copy.exists(), "{name}: and only as a plugin");

            ctx.symposium(&["init", "--add-agent", "opencode"]).await?;
            ctx.symposium(&["sync"]).await?;
            assert!(
                workspace
                    .join(".opencode/skills/global-guide/SKILL.md")
                    .is_file(),
                "{name}: the per-skill copy another agent reads is kept"
            );
            assert!(
                !project_copy.exists(),
                "{name}: in a folder this agent does not read"
            );

            let source = ctx.sym.config_dir().join("plugins/global-guide/SKILL.md");
            write(
                &source,
                "---\nname: global-guide\ndescription: edited\n---\n",
            );
            ctx.symposium(&["sync"]).await?;
            assert!(
                fs::read_to_string(installed())?.contains("edited"),
                "{name}: an edit propagates"
            );

            ctx.symposium(&["use", "--remove", "--global", "global-guide"])
                .await?;
            assert!(
                !installed().exists(),
                "{name}: dropping the global entry removes the plugin"
            );

            ctx.symposium(&["use", "global-guide"]).await?;
            assert!(
                workspace.join(agent.workspace_skill).is_file(),
                "{name}: a workspace entry brings the skill to the workspace"
            );

            ctx.symposium(&["use", "--remove", "global-guide"]).await?;
            ctx.symposium(&["use", "--global", "global-guide"]).await?;
            ctx.symposium(&["init", "--remove-agent", name]).await?;
            assert!(
                !installed().exists(),
                "{name}: removing the agent removes the plugin"
            );

            assert_eq!(
                fs::read_to_string(&users_own)?,
                "mine",
                "{name}: user content is untouched"
            );
            Ok(())
        })
        .await
        .unwrap();

        let Some(var) = agent.relocation_var else {
            continue;
        };
        with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
            let relocated = ctx.tempdir.join("relocated");
            let value = relocated.clone().into_os_string();
            ctx.sym
                .set_env(move |name| (name == var).then(|| value.clone()));

            ctx.symposium(&["init", "--add-agent", agent.name]).await?;
            ctx.symposium(&["sync"]).await?;

            assert!(
                (agent.plugin_dir)(&relocated, "global-guide").is_dir(),
                "{}: relocation",
                agent.name
            );
            let default_root = (agent.config_root)(ctx.sym.home_dir());
            assert!(!(agent.plugin_dir)(&default_root, "global-guide").exists());
            Ok(())
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn a_global_plugin_carries_its_skills_whatever_the_workspace_depends_on() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let plugin = ctx.sym.config_dir().join("plugins/gated-guide");
        write(
            &plugin.join("Symposium.toml"),
            "name = \"gated-guide\"\ndepends-on = [\"serde\"]\n\n[[skills]]\nsource.path = \"skills\"\ndepends-on = [\"tokio\"]\n",
        );
        write(
            &plugin.join("skills/tips/SKILL.md"),
            "---\nname: tips\ndescription: Tips\ndepends-on: rand\n---\n",
        );
        write(
            &plugin.join("skills/never/SKILL.md"),
            "---\nname: never\ndescription: Never\npredicates: env(SYMPOSIUM_TEST_NEVER_SET)\n---\n",
        );
        ctx.sym.config.plugins.used.push(UseEntry::Global("gated-guide".into()));
        ctx.sym.save_config()?;

        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["sync"]).await?;

        let skills = ctx.sym.home_dir().join(".claude/skills/gated-guide/skills");
        assert!(skills.join("tips/SKILL.md").is_file());
        assert!(!skills.join("never").exists(), "other predicates still apply");
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_skill_an_agent_takes_as_a_plugin_does_not_rename_its_namesake() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let plugins = ctx.sym.config_dir().join("plugins");
        for (plugin, gate) in [("gp", ""), ("pp", "depends-on = [\"*\"]\n")] {
            write(
                &plugins.join(plugin).join("Symposium.toml"),
                &format!("name = \"{plugin}\"\n{gate}\n[[skills]]\nsource.path = \"skills\"\n"),
            );
            write(
                &plugins.join(plugin).join("skills/guide/SKILL.md"),
                "---\nname: guide\ndescription: Guide\n---\n",
            );
        }
        ctx.sym
            .config
            .plugins
            .used
            .push(UseEntry::Global("gp".into()));
        ctx.sym.save_config()?;

        ctx.symposium(&[
            "init",
            "--add-agent",
            "claude",
            "--add-agent",
            "kiro",
            "--add-agent",
            "antigravity",
            "--add-agent",
            "opencode",
        ])
        .await?;
        ctx.symposium(&["sync"]).await?;

        let workspace = ctx.workspace_root.clone().unwrap();
        let guides = |dir: &str| -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(workspace.join(dir))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("guide"))
                .collect();
            names.sort();
            names
        };
        assert!(guides(".claude/skills").is_empty());
        assert!(
            workspace
                .join(".claude/skills/pp/skills/guide/SKILL.md")
                .is_file(),
            "claude takes the project plugin as a plugin too"
        );
        let kiro = guides(".kiro/skills");
        assert!(
            kiro.len() == 2 && kiro.iter().all(|name| name.starts_with("guide-")),
            "an agent copying both still needs the suffix: {kiro:?}"
        );
        assert_eq!(guides(".agents/skills"), ["guide"]);
        let opencode = guides(".opencode/skills");
        assert!(
            opencode.len() == 2 && opencode.iter().all(|name| name.starts_with("guide-")),
            "an agent copying both still needs the suffix: {opencode:?}"
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn an_unreadable_registry_keeps_global_plugins_until_a_sync_can_read_it() {
    for agent in AGENTS {
        with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
            let name = agent.name;
            ctx.symposium(&["init", "--add-agent", name]).await?;
            ctx.symposium(&["sync"]).await?;
            let staged = ctx.sym.config_dir().join("installed/global-guide");
            let root = (agent.config_root)(ctx.sym.home_dir());
            let copy = (agent.plugin_dir)(&root, "global-guide");
            assert!(staged.is_dir() && copy.is_dir(), "{name}");

            ctx.sym
                .config
                .plugins
                .used
                .retain(|entry| entry.name() != "global-guide");
            ctx.sym.config.registries.push(RegistryConfig {
                name: "gone".into(),
                git: None,
                path: Some(ctx.tempdir.join("gone").display().to_string()),
                auto_update: false,
            });
            ctx.sym.save_config()?;
            let events = ctx.sync_with_report(tracing::Level::INFO).await?;
            assert!(
                staged.is_dir() && copy.is_dir(),
                "{name}: kept while `gone` cannot be read"
            );
            assert!(
                events.iter().any(|e| e["kind"] == "warning"
                    && e["message"]
                        .to_string()
                        .contains("`gone` could not be fully read")),
                "{events:#?}"
            );

            ctx.sym
                .config
                .registries
                .retain(|registry| registry.name != "gone");
            ctx.sym.save_config()?;
            ctx.symposium(&["sync"]).await?;
            assert!(!staged.exists() && !copy.exists(), "{name}");
            Ok(())
        })
        .await
        .unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn an_entry_the_copy_skips_is_reported_once_per_sync() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let skill = ctx.sym.config_dir().join("plugins/global-guide");
        std::os::unix::fs::symlink("missing.md", skill.join("dangling.md"))?;
        ctx.symposium(&["init", "--add-agent", "claude", "--add-agent", "opencode"])
            .await?;

        let events = ctx.sync_with_report(tracing::Level::INFO).await?;

        let reported = events
            .iter()
            .filter(|e| e["kind"] == "warning" && e["message"].to_string().contains("dangling.md"))
            .count();
        assert_eq!(reported, 1, "{events:#?}");
        Ok(())
    })
    .await
    .unwrap();
}
