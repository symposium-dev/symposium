//! Process boundary for hooks invoked directly by an agent.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::process::ExitCode;

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

use super::{Decision, HookAgent, HookEvent, HookHandler, Input, Output, is_empty_output};
use crate::hook_schema::agent_event;

const HELP: &str = "Portable Symposium hook\n\nUsage: <hook> [--agent <agent>] [--event <event>]\n\nAgents: guess (default), claude, codex, kiro, copilot, antigravity, symposium\nEvents: pre-tool-use, post-tool-use, user-prompt-submit, session-start, stop\n\nReads one JSON event from stdin. Without --agent, tries Claude Code, Codex,\nthen Kiro. The selected adapter also determines the output protocol.\nUse --event when the input omits the event name. Native event names are also accepted.\n";

/// Run a portable hook directly under an agent, using the existing [`HookHandler`].
///
/// Reads process arguments and one JSON event from stdin. `--agent=claude`,
/// `codex`, `kiro`, `copilot`, or `antigravity` selects a native adapter;
/// `--agent=symposium` accepts the canonical format used by [`super::run`].
/// Without `--agent` (or with `--agent=guess`), tries Claude Code, Codex, then
/// Kiro, in that order. Overlapping inputs intentionally select Claude Code.
/// `--event=pre-tool-use` (and the other [`HookEvent`] names) supplies an event
/// name when the input omits it; a conflicting input name is an error.
/// Both `--flag=value` and `--flag value` are supported.
///
/// Fallback only decodes input: the handler runs at most once, and a handler
/// error never retries another adapter. Diagnostics go to stderr. Empty
/// responses produce no stdout; native responses use the selected agent's
/// protocol, including Kiro's plain text and exit status 2 for denied tools.
/// Kiro and Copilot input replacement is unsupported and returns an error.
///
/// Like [`super::run`], this synchronous process entry point owns its Tokio
/// runtime and should be called from `fn main() -> ExitCode`.
pub fn hook_main(handler: impl HookHandler) -> ExitCode {
    match run_process(handler) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("symposium-hook: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run_process(handler: impl HookHandler) -> Result<ExitCode> {
    let options = Options::parse(std::env::args_os().skip(1))?;
    if options.help {
        std::io::stdout().lock().write_all(HELP.as_bytes())?;
        return Ok(ExitCode::SUCCESS);
    }
    let mut payload = String::new();
    std::io::stdin()
        .read_to_string(&mut payload)
        .context("reading hook input")?;
    let decoded = decode(&options, &payload)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting hook runtime")?;
    let response = runtime.block_on(dispatch(&handler, decoded))?;
    std::io::stdout()
        .lock()
        .write_all(&response.stdout)
        .context("writing hook response")?;
    std::io::stderr()
        .lock()
        .write_all(&response.stderr)
        .context("writing hook diagnostic")?;
    Ok(ExitCode::from(response.status))
}

#[derive(Clone, Copy, Debug, Default)]
enum Protocol {
    #[default]
    Guess,
    Symposium,
    Native(HookAgent),
}

#[derive(Debug, Default)]
struct Options {
    protocol: Protocol,
    event: Option<HookEvent>,
    help: bool,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let mut options = Self::default();
        let mut seen_agent = false;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let arg = arg
                .into_string()
                .map_err(|_| anyhow::anyhow!("hook arguments must be UTF-8"))?;
            if arg == "--help" || arg == "-h" {
                options.help = true;
                continue;
            }
            let (flag, inline_value) = arg
                .split_once('=')
                .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
            ensure!(
                matches!(flag, "--agent" | "--event"),
                "unknown hook argument {flag:?}"
            );
            let value = match inline_value {
                Some(value) => value.to_owned(),
                None => args
                    .next()
                    .context(format!("missing value for {flag}"))?
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("hook arguments must be UTF-8"))?,
            };
            match flag {
                "--agent" => {
                    ensure!(!seen_agent, "--agent may only be supplied once");
                    seen_agent = true;
                    options.protocol = match value.as_str() {
                        "guess" => Protocol::Guess,
                        "symposium" => Protocol::Symposium,
                        _ => {
                            let agent: HookAgent =
                                serde_json::from_value(Value::String(value.clone()))
                                    .with_context(|| format!("unknown hook agent {value:?}"))?;
                            ensure!(
                                !matches!(agent, HookAgent::Goose | HookAgent::OpenCode),
                                "{} has no hook adapter",
                                agent.as_str()
                            );
                            Protocol::Native(agent)
                        }
                    };
                }
                "--event" => {
                    ensure!(options.event.is_none(), "--event may only be supplied once");
                    options.event = Some(parse_event_hint(&value)?);
                }
                _ => unreachable!(),
            }
        }
        Ok(options)
    }
}

fn parse_event_hint(name: &str) -> Result<HookEvent> {
    match name {
        "pre-tool-use" | "PreToolUse" | "preToolUse" => Ok(HookEvent::PreToolUse),
        "post-tool-use" | "PostToolUse" | "postToolUse" => Ok(HookEvent::PostToolUse),
        "user-prompt-submit"
        | "UserPromptSubmit"
        | "userPromptSubmit"
        | "userPromptSubmitted"
        | "PreInvocation" => Ok(HookEvent::UserPromptSubmit),
        "session-start" | "SessionStart" | "sessionStart" | "agentSpawn" => {
            Ok(HookEvent::SessionStart)
        }
        "stop" | "Stop" => Ok(HookEvent::Stop),
        _ => bail!("unsupported hook event {name:?}"),
    }
}

fn native_event_name(agent: HookAgent, event: HookEvent) -> &'static str {
    match (agent, event) {
        (HookAgent::Kiro | HookAgent::Copilot, HookEvent::PreToolUse) => "preToolUse",
        (HookAgent::Kiro | HookAgent::Copilot, HookEvent::PostToolUse) => "postToolUse",
        (HookAgent::Kiro, HookEvent::UserPromptSubmit) => "userPromptSubmit",
        (HookAgent::Copilot, HookEvent::UserPromptSubmit) => "userPromptSubmitted",
        (HookAgent::Kiro, HookEvent::SessionStart) => "agentSpawn",
        (HookAgent::Copilot, HookEvent::SessionStart) => "sessionStart",
        (HookAgent::Antigravity, HookEvent::UserPromptSubmit) => "PreInvocation",
        (_, HookEvent::PreToolUse) => "PreToolUse",
        (_, HookEvent::PostToolUse) => "PostToolUse",
        (_, HookEvent::UserPromptSubmit) => "UserPromptSubmit",
        (_, HookEvent::SessionStart) => "SessionStart",
        (_, HookEvent::Stop) => "Stop",
    }
}

struct Decoded {
    agent: Option<HookAgent>,
    input: Input,
    should_dispatch: bool,
}

fn decode(options: &Options, payload: &str) -> Result<Decoded> {
    let value: Value = serde_json::from_str(payload).context("parsing hook input JSON")?;
    ensure!(value.is_object(), "hook input must be a JSON object");
    match options.protocol {
        Protocol::Symposium => {
            let input: Input = serde_json::from_value(value).context("decoding Symposium event")?;
            ensure!(
                options.event.is_none_or(|hint| hint == input.event()),
                "--event conflicts with the input event"
            );
            Ok(Decoded {
                agent: None,
                input,
                should_dispatch: true,
            })
        }
        Protocol::Native(agent) => decode_native(agent, options.event, &value)
            .with_context(|| format!("decoding {} hook input", agent.as_str())),
        Protocol::Guess => {
            let mut failures = Vec::new();
            for agent in [HookAgent::Claude, HookAgent::Codex, HookAgent::Kiro] {
                match decode_native(agent, options.event, &value) {
                    Ok(decoded) => return Ok(decoded),
                    Err(error) => failures.push(format!("{}: {error:#}", agent.as_str())),
                }
            }
            bail!(
                "no hook adapter accepted the input; supply --agent and, if needed, --event ({})",
                failures.join("; ")
            )
        }
    }
}

fn decode_native(agent: HookAgent, hint: Option<HookEvent>, value: &Value) -> Result<Decoded> {
    let event = match value.get("hook_event_name") {
        Some(name) => {
            let name = name.as_str().context("hook_event_name must be a string")?;
            let event = parse_event_hint(name)?;
            ensure!(
                native_event_name(agent, event) == name,
                "unrecognized {} event {name:?}",
                agent.as_str()
            );
            ensure!(
                hint.is_none_or(|hint| hint == event),
                "--event conflicts with hook_event_name"
            );
            event
        }
        None => hint.context("input has no hook_event_name; supply --event")?,
    };
    let adapter = agent_event(agent, event)
        .with_context(|| format!("{} does not support {event:?}", agent.as_str()))?;
    let mut value = value.clone();
    // Some agents omit the discriminator, but their typed parser still needs it.
    value["hook_event_name"] = native_event_name(agent, event).into();
    if matches!(
        agent,
        HookAgent::Claude | HookAgent::Codex | HookAgent::Kiro
    ) {
        if matches!(event, HookEvent::PreToolUse | HookEvent::PostToolUse) {
            ensure!(
                value.get("tool_input").is_some(),
                "tool event is missing tool_input"
            );
        }
        if event == HookEvent::UserPromptSubmit && agent != HookAgent::Kiro {
            ensure!(
                value.get("prompt").is_some_and(Value::is_string),
                "prompt event requires a string prompt"
            );
        }
    }
    let input = adapter.parse_input(&serde_json::to_string(&value)?)?;
    Ok(Decoded {
        agent: Some(agent),
        should_dispatch: input.should_dispatch(),
        input: input.to_symposium(),
    })
}

#[derive(Default)]
struct Response {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    status: u8,
}

async fn dispatch(handler: &impl HookHandler, decoded: Decoded) -> Result<Response> {
    if !decoded.should_dispatch {
        return Ok(Response::default());
    }
    let event = decoded.input.event();
    let output = handler
        .handle_event(&decoded.input)
        .await
        .context("running hook handler")?;
    let output_event = match &output {
        Output::PreToolUse(_) => HookEvent::PreToolUse,
        Output::PostToolUse(_) => HookEvent::PostToolUse,
        Output::UserPromptSubmit(_) => HookEvent::UserPromptSubmit,
        Output::SessionStart(_) => HookEvent::SessionStart,
        Output::Stop(_) => HookEvent::Stop,
    };
    ensure!(
        event == output_event,
        "handler returned {output_event:?} output for {event:?}"
    );
    encode(decoded.agent, event, &output)
}

fn encode(agent: Option<HookAgent>, event: HookEvent, output: &Output) -> Result<Response> {
    if let Output::PreToolUse(output) = output {
        if let Some(agent @ (HookAgent::Kiro | HookAgent::Copilot)) = agent {
            ensure!(
                output.decision == Decision::Deny || output.updated_input.is_none(),
                "{} does not support tool input replacement",
                agent.as_str()
            );
        }
        if agent == Some(HookAgent::Kiro) && output.decision == Decision::Deny {
            let reason = output
                .additional_context
                .as_deref()
                .filter(|reason| !reason.trim().is_empty())
                .unwrap_or("Tool call denied by hook.");
            return Ok(Response {
                stderr: format!("{reason}\n").into_bytes(),
                status: 2,
                ..Response::default()
            });
        }
    }
    if is_empty_output(output) {
        return Ok(Response::default());
    }
    let stdout = match agent {
        None => serde_json::to_vec(output)?,
        Some(agent) => {
            let adapter =
                agent_event(agent, event).context("selected hook adapter is unavailable")?;
            adapter.serialize_output(&adapter.translate_output(output).to_hook_output())
        }
    };
    Ok(Response {
        stdout,
        ..Response::default()
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use serde_json::json;

    use super::*;
    use crate::hook::{PreToolUseOutput, SessionStartOutput};

    fn options(args: &[&str]) -> Options {
        Options::parse(args.iter().map(OsString::from)).unwrap()
    }

    #[test]
    fn overlapping_inputs_intentionally_choose_claude() {
        let decoded = decode(
            &Options::default(),
            &json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": {"command": "pwd"},
                "turn_id": "codex-extension",
                "extra": true,
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(decoded.agent, Some(HookAgent::Claude));
    }

    #[test]
    fn kiro_event_names_survive_fallback() {
        for (name, event) in [
            ("agentSpawn", HookEvent::SessionStart),
            ("userPromptSubmit", HookEvent::UserPromptSubmit),
        ] {
            let decoded = decode(
                &Options::default(),
                &json!({"hook_event_name": name}).to_string(),
            )
            .unwrap();
            assert_eq!(decoded.agent, Some(HookAgent::Kiro));
            assert_eq!(decoded.input.event(), event);
        }
    }

    struct FailingHook(Cell<usize>);

    impl HookHandler for FailingHook {
        async fn handle_event(&self, _: &Input) -> Result<Output> {
            self.0.set(self.0.get() + 1);
            bail!("handler failed after a side effect")
        }
    }

    #[tokio::test]
    async fn handler_failure_does_not_retry_overlapping_adapters() {
        let hook = FailingHook(Cell::new(0));
        let decoded = decode(&Options::default(), r#"{"hook_event_name":"SessionStart"}"#).unwrap();
        assert!(dispatch(&hook, decoded).await.is_err());
        assert_eq!(hook.0.get(), 1);
    }

    #[tokio::test]
    async fn antigravity_later_invocations_do_not_dispatch() {
        let hook = FailingHook(Cell::new(0));
        let decoded = decode(
            &options(&["--agent=antigravity", "--event=PreInvocation"]),
            r#"{"invocationNum":1}"#,
        )
        .unwrap();
        let response = dispatch(&hook, decoded).await.unwrap();
        assert!(response.stdout.is_empty());
        assert_eq!(response.status, 0);
        assert_eq!(hook.0.get(), 0);
    }

    struct WrongEvent;

    impl HookHandler for WrongEvent {
        async fn handle_event(&self, _: &Input) -> Result<Output> {
            Ok(Output::SessionStart(SessionStartOutput::context(
                "wrong event",
            )))
        }
    }

    #[tokio::test]
    async fn wrong_output_event_is_an_error_instead_of_an_adapter_panic() {
        let decoded = decode(&Options::default(), r#"{"hook_event_name":"Stop"}"#).unwrap();
        assert!(dispatch(&WrongEvent, decoded).await.is_err());
    }

    #[test]
    fn denial_without_context_is_not_an_empty_output() {
        let output = Output::PreToolUse(PreToolUseOutput::new(Decision::Deny, None, None));
        assert!(!is_empty_output(&output));
        let response = encode(None, HookEvent::PreToolUse, &output).unwrap();
        let canonical: Output = serde_json::from_slice(&response.stdout).unwrap();
        let Output::PreToolUse(canonical) = canonical else {
            panic!("wrong output event")
        };
        assert_eq!(canonical.decision, Decision::Deny);
        let response = encode(Some(HookAgent::Kiro), HookEvent::PreToolUse, &output).unwrap();
        assert_eq!(response.status, 2);
        assert!(!response.stderr.is_empty());
    }

    #[test]
    fn denial_wins_over_unsupported_replacement() {
        let output = Output::PreToolUse(PreToolUseOutput::new(
            Decision::Deny,
            Some("Do not run this tool".into()),
            Some(json!({"command": "pwd"})),
        ));
        let kiro = encode(Some(HookAgent::Kiro), HookEvent::PreToolUse, &output).unwrap();
        assert_eq!(kiro.status, 2);
        assert!(kiro.stdout.is_empty());
        assert_eq!(kiro.stderr, b"Do not run this tool\n");
        let copilot = encode(Some(HookAgent::Copilot), HookEvent::PreToolUse, &output).unwrap();
        assert_eq!(copilot.status, 0);
        let json: Value = serde_json::from_slice(&copilot.stdout).unwrap();
        assert_eq!(json["permissionDecision"], "deny");
        assert_eq!(json["permissionDecisionReason"], "Do not run this tool");
    }

    #[test]
    fn canonical_input_requires_explicit_selection() {
        let payload = r#"{"SessionStart":{"session_id":null,"cwd":null}}"#;
        assert!(decode(&Options::default(), payload).is_err());
        assert!(decode(&options(&["--agent=symposium"]), payload).is_ok());
        assert!(decode(&options(&["--agent=symposium", "--event=stop"]), payload).is_err());
    }

    #[test]
    fn duplicate_flags_and_non_hook_agents_are_rejected() {
        for args in [
            vec!["--agent=claude", "--agent=kiro"],
            vec!["--event=stop", "--event=session-start"],
            vec!["--agent=goose"],
            vec!["--agent=opencode"],
        ] {
            assert!(Options::parse(args.iter().map(OsString::from)).is_err());
        }
    }
}
