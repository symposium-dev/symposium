//! Remote dependency plugins participate in consent, installation, and cleanup.
//! The shell fixture uses Unix paths in JSON, as in `remote_pm.rs`.
#![cfg(unix)]

use std::{path::Path, sync::Arc, time::Duration};

use anyhow::Result;
use serde_json::Value;
use symposium::UpdateLevel;
use symposium::config::{PluginRef, Symposium};
use symposium::pm::{
    PackageId, PluginKind, PmInstance, PmRegistry, RemotePm, RemotePmCommand, Workspace,
};
use symposium_testlib::{TestMode, with_fixture};

fn workspace(root: &Path, content: &Path, mode: &str, name: &str) -> Result<Arc<Workspace>> {
    let script = root.join(format!("{name}-{mode}.sh"));
    std::fs::write(
        &script,
        r#"
root="$1"
content="$2"
mode="$3"
name="$4"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed 's/.*"id":\([0-9]*\),"method".*/\1/')
  case "$line" in
    *'"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocol_version":1,"name":"%s"}}\n' "$id" "$name" ;;
    *'"canonical_name"'*)
      params=$(printf '%s' "$line" | sed 's/^.*"params"://; s/}$//')
      printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$id" "$params" ;;
    *'"project_info"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"project":{"root":"%s","inputs":[],"local_packages":[]}}}\n' "$id" "$root" ;;
    *'"list_deps"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"deps":[{"pm":"%s","name":"widget_lib","version":"1.0.0"}]}}\n' "$id" "$name" ;;
    *'"load_plugin"'*|*'"active_plugins"'*)
      case "$line" in
        *'"load_plugin"'*)
          case "$mode" in
            exit) exit 1 ;;
            timeout) while IFS= read -r ignored; do :; done; exit 0 ;;
            error)
              printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32001,"message":"package not found"}}\n' "$id"
              continue ;;
          esac ;;
      esac
      printf '{"jsonrpc":"2.0","id":%s,"result":{"plugins":[{"id":{"pm":"%s","name":"widget_lib","version":"1.0.0"},"root":"%s","manifest":{"name":"widget_lib","skills":[{"source":{"path":"skills"}}],"mcp_servers":[{"name":"widget-server","command":"/usr/bin/true","args":[],"env":[]}]}}]}}\n' "$id" "$name" "$content" ;;
    *)
      printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"unknown method"}}\n' "$id" ;;
  esac
done
"#,
    )?;
    let remote = RemotePm::new(
        name,
        RemotePmCommand::new("/bin/sh")
            .arg(script.display().to_string())
            .arg(root.display().to_string())
            .arg(content.display().to_string())
            .arg(mode)
            .arg(name)
            .timeout(Duration::from_secs(1)),
    );
    Ok(Arc::new(Workspace::new(
        Some(root.to_path_buf()),
        PmRegistry::new(vec![PmInstance::new(
            name,
            false,
            PluginKind::Package,
            Box::new(remote),
        )]),
    )))
}

fn content(root: &Path) -> Result<std::path::PathBuf> {
    let content = root.join("widget-content");
    std::fs::create_dir_all(content.join("skills/widget-usage"))?;
    std::fs::write(
        content.join("skills/widget-usage/SKILL.md"),
        "---\nname: widget-usage\ndescription: Widget guidance\n---\nBody.\n",
    )?;
    Ok(content)
}

fn mcp_config(root: &Path) -> Result<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(
        root.join(".mcp.json"),
    )?)?)
}

async fn check_load_error_cleanup(mode: &str, preserve: bool) {
    with_fixture(
        TestMode::SimulationOnly,
        &["dormant-plugin0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.sym.config.plugins.auto_enable = vec!["*".into()];
            let root = ctx.workspace_root.clone().unwrap();
            let content = content(&root)?;
            let healthy = workspace(&root, &content, "ok", "cargo")?;
            symposium::sync::sync(&ctx.sym, &healthy, UpdateLevel::None).await?;
            let skill = root.join(".claude/skills/widget-lib/skills/widget-usage/SKILL.md");
            assert!(skill.is_file());
            let before = mcp_config(&root)?;
            assert!(
                before["mcpServers"]["widget-server"].is_object(),
                "{before}"
            );

            // The second PM answers project_info, list_deps, and normalization
            // normally; only load_plugin fails after those answers were cached.
            let failed = workspace(&root, &content, mode, "cargo")?;
            symposium::sync::sync(&ctx.sym, &failed, UpdateLevel::None).await?;
            let after = mcp_config(&root)?;
            if preserve {
                assert_eq!(failed.failed(), ["cargo"]);
                assert_eq!(
                    std::fs::read_to_string(&skill)?,
                    std::fs::read_to_string(content.join("skills/widget-usage/SKILL.md"))?
                );
                assert_eq!(
                    after["mcpServers"]["widget-server"],
                    before["mcpServers"]["widget-server"]
                );
            } else {
                assert!(failed.failed().is_empty());
                assert!(!skill.exists());
                assert!(after["mcpServers"].get("widget-server").is_none());
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_crash_during_load_preserves_installed_skills_and_mcp() {
    check_load_error_cleanup("exit", true).await;
}

#[tokio::test]
async fn a_timeout_during_load_preserves_installed_skills_and_mcp() {
    check_load_error_cleanup("timeout", true).await;
}

#[tokio::test]
async fn a_package_error_does_not_disable_cleanup() {
    check_load_error_cleanup("error", false).await;
}

#[tokio::test]
async fn auto_enabled_noncargo_dependencies_install_skills_and_mcp() {
    with_fixture(
        TestMode::SimulationOnly,
        &["dormant-plugin0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let root = ctx.workspace_root.clone().unwrap();
            let content = content(&root)?;
            let ws = workspace(&root, &content, "ok", "demo")?;
            let skill = root.join(".claude/skills/widget-lib/skills/widget-usage/SKILL.md");
            for entry in ["*", "widget_lib"] {
                ctx.sym.config.plugins.auto_enable = vec![entry.into()];
                ctx.sym.config.plugins.disable.clear();
                let discovered = symposium::discovery::discover_for(&ctx.sym, &ws).await;
                assert_eq!(discovered.auto_enabled.len(), 1, "{discovered:?}");
                symposium::sync::sync(&ctx.sym, &ws, UpdateLevel::None).await?;
                assert!(skill.is_file(), "{entry}");
                assert!(mcp_config(&root)?["mcpServers"]["widget-server"].is_object());

                // Explicit declines still take precedence over either spelling.
                ctx.sym.config.plugins.disable = vec!["widget_lib".into()];
                symposium::sync::sync(&ctx.sym, &ws, UpdateLevel::None).await?;
                assert!(!skill.exists());
                assert!(
                    mcp_config(&root)?["mcpServers"]
                        .get("widget-server")
                        .is_none()
                );
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn consent_preserves_distinct_noncargo_names_and_survives_reload() {
    with_fixture(
        TestMode::SimulationOnly,
        &["dormant-plugin0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let root = ctx.workspace_root.clone().unwrap();
            let content = content(&root)?;
            let ws = workspace(&root, &content, "ok", "demo")?;
            ctx.sym.config.plugins.auto_enable = vec!["widget-lib".into()];
            ctx.sym.config.plugins.disable = vec!["widget-lib".into()];
            let discovery = symposium::discovery::discover_for(&ctx.sym, &ws).await;
            assert_eq!(discovery.candidates.len(), 1);
            let id = discovery.candidates[0].id.clone();
            assert_eq!(id, PackageId::new("demo", "widget_lib", "1.0.0"));
            symposium::discovery::apply_consent(&mut ctx.sym, std::slice::from_ref(&id), &[])?;
            let saved = Symposium::from_dir(ctx.sym.config_dir());
            assert_eq!(
                saved.config.plugins.auto_enable,
                [
                    PluginRef::bare("widget-lib"),
                    PluginRef::new("demo", "widget_lib")
                ]
            );
            assert!(
                symposium::discovery::discover_for(&saved, &ws)
                    .await
                    .candidates
                    .is_empty()
            );
            symposium::sync::sync(&saved, &ws, UpdateLevel::None).await?;
            let skill = root.join(".claude/skills/widget-lib/skills/widget-usage/SKILL.md");
            assert!(skill.is_file());

            symposium::discovery::apply_consent(&mut ctx.sym, &[], std::slice::from_ref(&id))?;
            let saved = Symposium::from_dir(ctx.sym.config_dir());
            assert_eq!(
                saved.config.plugins.disable,
                [
                    PluginRef::bare("widget-lib"),
                    PluginRef::new("demo", "widget_lib")
                ]
            );
            let discovery = symposium::discovery::discover_for(&saved, &ws).await;
            assert_eq!(discovery.declined.len(), 1);
            assert!(discovery.candidates.is_empty());
            symposium::sync::sync(&saved, &ws, UpdateLevel::None).await?;
            assert!(!skill.exists());
            Ok(())
        },
    )
    .await
    .unwrap();
}
