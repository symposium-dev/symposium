mod copilot;

use std::fs;
use std::path::{Path, PathBuf};

use symposium::config::{RegistryConfig, UseEntry};
use symposium_testlib::{TestMode, with_fixture};

pub(crate) const SCOPES: &[&str] = &["agent-plugin-scopes0"];

struct DeliveringAgent {
    name: &'static str,
    project_skills: &'static str,
    relocation_var: &'static str,
    config_root: fn(&Path) -> PathBuf,
    plugin_dir: fn(&Path, &str) -> PathBuf,
}

const AGENTS: &[DeliveringAgent] = &[
    DeliveringAgent {
        name: "claude",
        project_skills: ".claude/skills",
        relocation_var: "CLAUDE_CONFIG_DIR",
        config_root: |home| home.join(".claude"),
        plugin_dir: |root, plugin| root.join("skills").join(plugin),
    },
    DeliveringAgent {
        name: "copilot",
        project_skills: ".agents/skills",
        relocation_var: "COPILOT_HOME",
        config_root: |home| home.join(".copilot"),
        plugin_dir: copilot::plugin_dir,
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
                    .join(".agents/skills/global-guide/SKILL.md")
                    .is_file(),
                "{name}: the per-skill copy another agent reads is kept"
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
                project_copy.is_file(),
                "{name}: a workspace entry brings per-skill copies back"
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

        with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
            let relocated = ctx.tempdir.join("relocated");
            let value = relocated.clone().into_os_string();
            let var = agent.relocation_var;
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
async fn a_global_plugin_carries_all_its_skills_whatever_the_workspace() {
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
        write(
            &plugin.join("skills/without-rand/SKILL.md"),
            "---\nname: without-rand\ndescription: Without rand\npredicates: not(depends-on(rand))\n---\n",
        );
        ctx.sym.config.plugins.used.push(UseEntry::Global("gated-guide".into()));
        ctx.sym.save_config()?;

        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["sync"]).await?;

        let skills = ctx.sym.home_dir().join(".claude/skills/gated-guide/skills");
        for skill in ["tips", "never", "without-rand"] {
            assert!(skills.join(skill).join("SKILL.md").is_file(), "{skill}");
        }
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_stale_skill_copy_is_reaped_even_when_it_carries_a_plugin_manifest() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let source = ctx.sym.config_dir().join("plugins/packaged");
        write(
            &source.join("SKILL.md"),
            "---\nname: packaged\ndescription: Packaged\n---\n",
        );
        write(&source.join(".claude-plugin/plugin.json"), "{}");
        let copy = ctx
            .workspace_root
            .clone()
            .unwrap()
            .join(".claude/skills/packaged");

        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["use", "packaged"]).await?;
        assert!(copy.join(".claude-plugin/plugin.json").is_file());

        ctx.symposium(&["use", "--remove", "packaged"]).await?;
        assert!(!copy.exists());
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

        ctx.symposium(&["init", "--add-agent", "claude", "--add-agent", "kiro"])
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
        assert_eq!(guides(".claude/skills"), ["guide"]);
        let kiro = guides(".kiro/skills");
        assert!(
            kiro.len() == 2 && kiro.iter().all(|name| name.starts_with("guide-")),
            "an agent copying both still needs the suffix: {kiro:?}"
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn an_unreadable_registry_keeps_global_plugins_until_a_sync_can_read_it() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["sync"]).await?;
        let staged = ctx.sym.config_dir().join("installed/global-guide");
        let copy = ctx.sym.home_dir().join(".claude/skills/global-guide");
        assert!(staged.is_dir() && copy.is_dir());

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
            "kept while `gone` cannot be read"
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
        assert!(!staged.exists() && !copy.exists());
        Ok(())
    })
    .await
    .unwrap();
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
