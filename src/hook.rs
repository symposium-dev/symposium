use std::io::{Read, Write};
use std::process::{Command, ExitCode, Stdio};

use serde::{Deserialize, Serialize};

use crate::config::Symposium;
use crate::plugins::ParsedPlugin;

#[derive(Debug, Clone, clap::ValueEnum, Serialize, Deserialize, PartialEq, Eq)]
pub enum HookEvent {
    #[value(name = "pre-tool-use")]
    #[serde(rename = "PreToolUse")]
    PreToolUse,

    #[value(name = "post-tool-use")]
    #[serde(rename = "PostToolUse")]
    PostToolUse,

    #[value(name = "user-prompt-submit")]
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPayload {
    #[serde(flatten)]
    pub sub_payload: HookSubPayload,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "hook_event_name")]
pub enum HookSubPayload {
    #[serde(rename = "PreToolUse")]
    PreToolUse(PreToolUsePayload),

    #[serde(rename = "PostToolUse")]
    PostToolUse(PostToolUsePayload),

    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit(UserPromptSubmitPayload),
}

impl HookSubPayload {
    pub fn hook_event(&self) -> HookEvent {
        match self {
            HookSubPayload::PreToolUse(_) => HookEvent::PreToolUse,
            HookSubPayload::PostToolUse(_) => HookEvent::PostToolUse,
            HookSubPayload::UserPromptSubmit(_) => HookEvent::UserPromptSubmit,
        }
    }

    #[tracing::instrument(ret)]
    pub fn matches_matcher(&self, matcher: &str) -> bool {
        // TODO: I'm not sure what exactly Claude's rules are, but this is fine for now
        if matcher == "*" {
            return true;
        }
        match self {
            HookSubPayload::PreToolUse(payload) => matcher.contains(&payload.tool_name),
            HookSubPayload::PostToolUse(payload) => matcher.contains(&payload.tool_name),
            HookSubPayload::UserPromptSubmit(_) => true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreToolUsePayload {
    pub tool_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToolUsePayload {
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_response: serde_json::Value,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPromptSubmitPayload {
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// Structured output from built-in hook logic.
///
/// Serialized to JSON on stdout for Claude Code to consume.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookOutput {
    /// If set, injected into the LLM conversation as additional context.
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<HookSpecificOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
}

impl HookOutput {
    /// Create a HookOutput with additional context for the given event.
    pub fn with_context(event_name: &str, context: String) -> Self {
        Self {
            hook_specific_output: Some(HookSpecificOutput {
                hook_event_name: event_name.to_string(),
                additional_context: Some(context),
            }),
        }
    }

    /// Create an empty HookOutput (no additional context).
    pub fn empty() -> Self {
        Self::default()
    }
}

/// CLI entry point: read payload from stdin, dispatch, print output.
pub async fn run(sym: &Symposium, event: HookEvent) -> ExitCode {
    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        tracing::warn!(?event, error = %e, "failed to read hook stdin");
        return ExitCode::SUCCESS;
    }

    let payload = serde_json::from_str::<HookPayload>(&input);
    let Ok(payload) = payload else {
        tracing::warn!(
            ?event,
            error = "invalid hook payload",
            "failed to parse hook stdin as JSON"
        );
        return ExitCode::FAILURE;
    };

    if payload.sub_payload.hook_event() != event {
        tracing::warn!(?event, payload_event = ?payload.sub_payload.hook_event(), "hook event mismatch between CLI arg and payload");
        return ExitCode::FAILURE;
    }

    // Run built-in hook logic
    let hook_output = dispatch_builtin(sym, &payload).await;

    // Run plugin hooks (for PreToolUse only, for now)
    dispatch_plugin_hooks(sym, &payload);

    // Emit structured output if there's anything to say
    if hook_output.hook_specific_output.is_some() {
        if let Ok(json) = serde_json::to_string(&hook_output) {
            println!("{json}");
        }
    }

    ExitCode::SUCCESS
}

/// Built-in hook logic. Returns typed `HookOutput` for the integration test harness.
pub async fn dispatch_builtin(_sym: &Symposium, payload: &HookPayload) -> HookOutput {
    tracing::info!(?payload, "hook invoked (builtin)");

    match &payload.sub_payload {
        HookSubPayload::PreToolUse(_) => {
            // No built-in logic for PreToolUse — plugin hooks only.
            HookOutput::empty()
        }
        HookSubPayload::PostToolUse(_post) => {
            // TODO (Step 7): activation recording
            HookOutput::empty()
        }
        HookSubPayload::UserPromptSubmit(_prompt) => {
            // TODO (Step 8): nudge logic
            HookOutput::empty()
        }
    }
}

/// Dispatch plugin hooks (spawn subprocesses).
fn dispatch_plugin_hooks(sym: &Symposium, payload: &HookPayload) {
    let plugins = crate::plugins::load_all_plugins(sym);
    let hooks = hooks_for_payload(&plugins, payload);

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
                    if let Err(e) =
                        stdin.write_all(serde_json::to_string(&payload).unwrap().as_bytes())
                    {
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
}

/// Return all hooks (with their plugin name) that match the event in `payload`.
fn hooks_for_payload(
    plugins: &[crate::plugins::ParsedPlugin],
    payload: &HookPayload,
) -> Vec<(String, crate::plugins::Hook)> {
    tracing::debug!(?payload);

    let mut out = Vec::new();

    for ParsedPlugin { path: _, plugin } in plugins {
        let name = plugin.name.clone();
        for hook in &plugin.hooks {
            tracing::debug!(?hook);
            if hook.event != payload.sub_payload.hook_event() {
                continue;
            }
            if let Some(matcher) = &hook.matcher {
                if !payload.sub_payload.matches_matcher(matcher) {
                    tracing::info!(
                        ?payload,
                        ?matcher,
                        "skipping hook due to non-matching matcher"
                    );
                    continue;
                }
            }
            out.push((name.clone(), hook.clone()));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use indoc::formatdoc;

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

    #[tokio::test]
    async fn plugin_hooks_run_and_create_files() {
        setup_tracing();

        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();

        let sym = Symposium::from_dir(home);

        // Ensure plugins dir exists and get its path.
        let plugins_dir = sym.plugins_dir();

        // Prepare two output files that the hooks will create.
        let out1 = home.join("out1.txt");
        let out2 = home.join("out2.txt");
        let out3 = home.join("out3.txt");
        let out4 = home.join("out4.txt");
        let out5 = home.join("out5.txt");

        // Create two plugin TOML files that run simple echo commands.
        let p1 = formatdoc! {r#"
            name = "plugin-one"

            [[hooks]]
            name = "write1"
            event = "PreToolUse"
            command = "sh -c 'echo plugin-one-write1 > {out1}'"
        "#, out1 = out1.display()};

        let p2 = formatdoc! {r#"
            name = "plugin-two"

            [[hooks]]
            name = "write2"
            event = "PreToolUse"
            matcher = "*"
            command = "sh -c 'echo plugin-two-write2 > {out2}'"

            [[hooks]]
            name = "write3"
            event = "PreToolUse"
            matcher = "Bash"
            command = "sh -c 'echo plugin-two-write3 > {out3}'"

            [[hooks]]
            name = "write4"
            event = "PreToolUse"
            matcher = "Bash|Read"
            command = "sh -c 'echo plugin-two-write4 > {out4}'"

            [[hooks]]
            name = "write4"
            event = "PreToolUse"
            matcher = "Read|Write"
            command = "sh -c 'echo plugin-two-write5 > {out5}'"
        "#,
            out2 = out2.display(),
            out3 = out3.display(),
            out4 = out4.display(),
            out5 = out5.display(),
        };

        fs::write(plugins_dir.join("plugin-one.toml"), p1).expect("write plugin1");
        fs::write(plugins_dir.join("plugin-two.toml"), p2).expect("write plugin2");

        // Run the hook event via dispatch_plugin_hooks.
        let payload = HookPayload {
            sub_payload: HookSubPayload::PreToolUse(PreToolUsePayload {
                tool_name: "Bash".to_string(),
            }),
            rest: serde_json::Map::new(),
        };
        dispatch_plugin_hooks(&sym, &payload);

        // Verify files were created and contain expected contents.
        let got1 = fs::read_to_string(&out1).expect("read out1");
        let got2 = fs::read_to_string(&out2).expect("read out2");
        let got3 = fs::read_to_string(&out3).expect("read out3");
        let got4 = fs::read_to_string(&out4).expect("read out4");

        assert!(got1.contains("plugin-one-write1"));
        assert!(got2.contains("plugin-two-write2"));
        assert!(got3.contains("plugin-two-write3"));
        assert!(got4.contains("plugin-two-write4"));

        // No file created, matcher doesn't match
        assert!(fs::read_to_string(&out5).is_err());
    }

    #[tokio::test]
    async fn builtin_pre_tool_use_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let payload = HookPayload {
            sub_payload: HookSubPayload::PreToolUse(PreToolUsePayload {
                tool_name: "Bash".to_string(),
            }),
            rest: serde_json::Map::new(),
        };
        let output = dispatch_builtin(&sym, &payload).await;
        assert!(output.hook_specific_output.is_none());
    }

    #[tokio::test]
    async fn builtin_post_tool_use_returns_empty_for_now() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let payload = HookPayload {
            sub_payload: HookSubPayload::PostToolUse(PostToolUsePayload {
                tool_name: "Bash".to_string(),
                tool_input: serde_json::json!({"command": "ls"}),
                tool_response: serde_json::json!({"stdout": "file.rs"}),
                session_id: Some("test-session".to_string()),
                cwd: Some("/tmp".to_string()),
            }),
            rest: serde_json::Map::new(),
        };
        let output = dispatch_builtin(&sym, &payload).await;
        assert!(output.hook_specific_output.is_none());
    }

    #[tokio::test]
    async fn builtin_user_prompt_submit_returns_empty_for_now() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let payload = HookPayload {
            sub_payload: HookSubPayload::UserPromptSubmit(UserPromptSubmitPayload {
                prompt: "Use tokio for async".to_string(),
                session_id: Some("test-session".to_string()),
                cwd: Some("/tmp".to_string()),
            }),
            rest: serde_json::Map::new(),
        };
        let output = dispatch_builtin(&sym, &payload).await;
        assert!(output.hook_specific_output.is_none());
    }

    #[test]
    fn hook_output_serializes_with_additional_context() {
        let output = HookOutput::with_context("UserPromptSubmit", "Load tokio guidance".to_string());
        let json = serde_json::to_value(&output).unwrap();
        assert_eq!(
            json["hookSpecificOutput"]["hookEventName"],
            "UserPromptSubmit"
        );
        assert_eq!(
            json["hookSpecificOutput"]["additionalContext"],
            "Load tokio guidance"
        );
    }

    #[test]
    fn hook_output_empty_serializes_without_hook_specific() {
        let output = HookOutput::empty();
        let json = serde_json::to_value(&output).unwrap();
        assert!(json.get("hookSpecificOutput").is_none());
    }
}
