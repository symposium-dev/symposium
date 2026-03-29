use std::io::{Read, Write};
use std::process::{Command, ExitCode, Stdio};

use serde::Deserialize;

#[derive(Debug, Clone, clap::ValueEnum, Deserialize, PartialEq, Eq)]
pub enum HookEvent {
    /// Claude Code PreToolUse hook
    #[value(name = "claude:pre-tool-use")]
    #[serde(rename = "claude:pre-tool-use")]
    ClaudePreToolUse,
}

pub fn run(event: HookEvent) -> ExitCode {
    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        tracing::warn!(?event, error = %e, "failed to read hook stdin");
        return ExitCode::SUCCESS;
    }

    dispatch_hook(event, &input)
}

/// Handle hook dispatch for a parsed payload string. Separated from `run`
/// so tests and other callers can invoke it without wiring stdin.
pub fn dispatch_hook(event: HookEvent, payload: &str) -> ExitCode {
    tracing::info!(?event, "hook invoked");
    tracing::debug!(?event, payload = %payload, "hook payload");

    let Ok(hooks) = crate::plugins::hooks_for_event(&event) else {
        tracing::warn!(
            ?event,
            error = "failed to load global plugins",
            "skipping plugin hooks"
        );
        return ExitCode::FAILURE;
    };

    for (plugin_name, hook) in hooks {
        tracing::info!(?plugin_name, hook = %hook.name, cmd = %hook.command, "running plugin hook");
        let spawn_res = Command::new("sh")
            .arg("-c")
            .arg(&hook.command)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn();

        match spawn_res {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    if let Err(e) = stdin.write_all(payload.as_bytes()) {
                        tracing::warn!(error = %e, "failed to write hook stdin");
                    }
                }

                match child.wait() {
                    Ok(status) => tracing::info!(?status, "hook finished"),
                    Err(e) => tracing::warn!(error = %e, "failed waiting for hook process"),
                }
            }
            Err(e) => tracing::warn!(error = %e, "failed to spawn hook command"),
        }
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .compact()
            .with_ansi(false)
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive(tracing::Level::DEBUG.into()),
            )
            .try_init();
    }

    #[test]
    fn plugin_hooks_run_and_create_files() {
        setup_tracing();

        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();

        // Point HOME to our temp dir so plugins_dir() is under it.
        unsafe {
            std::env::set_var("HOME", home);
        }

        // Ensure plugins dir exists and get its path.
        let plugins_dir = crate::config::plugins_dir();

        // Prepare two output files that the hooks will create.
        let out1 = home.join("out1.txt");
        let out2 = home.join("out2.txt");

        // Create two plugin TOML files that run simple echo commands.
        let p1 = format!(
            r#"
name = "plugin-one"

[[hooks]]
name = "write1"
event = "claude:pre-tool-use"
command = "sh -c 'echo plugin-one > {}'"
"#,
            out1.display()
        );

        let p2 = format!(
            r#"
name = "plugin-two"

[[hooks]]
name = "write2"
event = "claude:pre-tool-use"
command = "sh -c 'echo plugin-two > {}'"
"#,
            out2.display()
        );

        fs::write(plugins_dir.join("plugin-one.toml"), p1).expect("write plugin1");
        fs::write(plugins_dir.join("plugin-two.toml"), p2).expect("write plugin2");

        // Run the hook event. This will spawn the commands which create the files.
        let _ = dispatch_hook(HookEvent::ClaudePreToolUse, "");

        // Verify files were created and contain expected contents.
        let got1 = fs::read_to_string(&out1).expect("read out1");
        let got2 = fs::read_to_string(&out2).expect("read out2");

        assert!(got1.contains("plugin-one"));
        assert!(got2.contains("plugin-two"));
    }
}
