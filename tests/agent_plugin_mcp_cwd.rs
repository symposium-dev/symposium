//! Portable MCP servers resolve relative arguments and resources from their package.
//! This simulates a client's launch of the exported configuration without running
//! a real agent, including servers that supply their own command search path.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result};
use serde_json::{Value, json};
use symposium::config::Symposium;
use symposium_testlib::{TestMode, with_fixture};

/// Launch the exported stdio entry as a client would, from its working directory
/// unless the entry explicitly supplies one. A generated launcher works here too.
fn launch(server: &Value, client_dir: &Path) -> Result<Output> {
    let mut command = Command::new(server["command"].as_str().context("stdio command")?);
    if let Some(args) = server.get("args") {
        for arg in args.as_array().context("stdio arguments")? {
            command.arg(arg.as_str().context("string argument")?);
        }
    }
    if let Some(env) = server.get("env") {
        for (name, value) in env.as_object().context("stdio environment")? {
            command.env(name, value.as_str().context("string environment value")?);
        }
    }
    let cwd = match server.get("cwd") {
        Some(cwd) => client_dir.join(cwd.as_str().context("stdio working directory")?),
        None => client_dir.to_path_buf(),
    };
    command
        .current_dir(cwd)
        .output()
        .context("launching stdio server")
}

fn package_fixture(root: &Path, portable_server: &Value) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let project = root.join("project");
    fs::create_dir_all(project.join("src"))?;
    fs::write(project.join("src/lib.rs"), "")?;
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"cwd-host\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )?;

    let config = root.join("config");
    let package = config.join("plugins/portable-cwd");
    fs::create_dir_all(&package)?;
    fs::write(
        config.join("config.toml"),
        indoc::indoc! {r#"
                hook-scope = "project"
                auto-update = "off"

                [defaults]
                symposium-recommendations = false
                user-plugins = true

                [plugins]
                use = ["portable-cwd"]
            "#},
    )?;
    fs::write(
        package.join("plugin.json"),
        serde_json::to_vec(&json!({
            "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name": "portable-cwd",
        }))?,
    )?;
    fs::write(
        package.join("mcp.json"),
        serde_json::to_vec(&json!({
            "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
            "mcpServers": { "package-resource": portable_server },
        }))?,
    )?;
    Ok((project, config, package))
}

#[tokio::test]
async fn exported_mcp_server_preserves_the_package_working_directory() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        let portable_server = json!({
            "type": "stdio",
            "command": "sh",
            "args": ["server.sh"],
        });
        let (project, config, package) = package_fixture(&ctx.tempdir, &portable_server)?;
        fs::write(package.join("server.sh"), "cat package-resource.txt\n")?;
        fs::write(package.join("package-resource.txt"), "from the package\n")?;

        let control = launch(&portable_server, &package)?;
        assert!(control.status.success(), "{control:?}");
        assert_eq!(control.stdout, b"from the package\n");

        ctx.sym = Symposium::from_dir(&config);
        ctx.workspace_root = Some(project.clone());
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["sync"]).await?;

        let exported: Value = serde_json::from_slice(&fs::read(project.join(".mcp.json"))?)?;
        let server = exported["mcpServers"]
            .get("package-resource")
            .context("the enabled package's MCP server must be registered")?;
        let actual = launch(server, &project)?;
        assert!(
            actual.status.success(),
            "the exported server must launch from the project and still read its package resources: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(actual.stdout, control.stdout);
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn exported_mcp_server_starts_with_its_private_path() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        let portable_server = json!({
            "type": "stdio",
            "command": "./bin/server",
            "env": { "PATH": "${PLUGIN_ROOT}/bin" },
        });
        let (project, config, package) = package_fixture(&ctx.tempdir, &portable_server)?;
        fs::create_dir_all(package.join("bin"))?;
        // Match PLUGIN_ROOT expansion even when the platform's temp directory
        // is reached through a symlink (for example /var on macOS).
        let private_bin = fs::canonicalize(package.join("bin"))?;
        let executable = private_bin.join("server");
        // The absolute interpreter and shell builtin work without PATH lookup.
        // Printing PATH also proves that a launcher preserves the server's env.
        fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' \"$PATH\"\n")?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
        let private_path = private_bin.display().to_string();
        let control = launch(
            &json!({
                "command": executable,
                "env": { "PATH": private_path },
            }),
            &package,
        )?;
        assert!(control.status.success(), "{control:?}");
        assert_eq!(control.stdout, format!("{private_path}\n").as_bytes());

        ctx.sym = Symposium::from_dir(&config);
        ctx.workspace_root = Some(project.clone());
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["sync"]).await?;

        let exported: Value = serde_json::from_slice(&fs::read(project.join(".mcp.json"))?)?;
        let server = exported["mcpServers"]
            .get("package-resource")
            .context("the enabled package's MCP server must be registered")?;
        assert_eq!(server["env"]["PATH"], private_path);
        let actual = launch(server, &project)
            .context("the exported server must start even when its PATH contains no shell")?;
        assert!(
            actual.status.success(),
            "the exported server must start with its private PATH: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(actual.stdout, control.stdout);
        Ok(())
    })
    .await
    .unwrap();
}
