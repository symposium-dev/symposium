# Writing a hook handler

This guide walks through writing a symposium hook handler in Rust using the `symposium-sdk` crate.

## Step 1. Create a new binary crate

Create your new crate. A separate crate next to your library (for example `my-crate-hooks`) keeps the SDK out of your library's dependencies:

```bash
cargo new my-crate-hooks
cd my-crate-hooks
```

And then add symposium-sdk to your dependencies, along with `anyhow`, which the handler methods return:

```bash
cargo add symposium-sdk anyhow
```

## Step 2. Write the handler

A hook handler is a program that reads a JSON event on stdin and writes a JSON response to stdout. The `symposium-sdk` crate provides a `HookHandler` trait and a `run()` harness that handles the plumbing.

Implement `HookHandler` and override the methods for the events you care about:

```rust
// src/main.rs
use std::process::ExitCode;
use symposium_sdk::hook::{HookHandler, PreToolUseInput, PreToolUseOutput, run};

// The shell tool's name in Claude Code and Codex, Copilot, Antigravity, and Kiro.
const SHELL_TOOLS: &[&str] = &["Bash", "bash", "run_command", "execute_bash"];

struct MyHook;

impl HookHandler for MyHook {
    async fn pre_tool_use(&self, event: &PreToolUseInput) -> anyhow::Result<PreToolUseOutput> {
        if SHELL_TOOLS.contains(&event.tool_name.as_str()) {
            Ok(PreToolUseOutput::context("Remember: prefer non-destructive commands"))
        } else {
            Ok(PreToolUseOutput::default())
        }
    }
}

fn main() -> ExitCode {
    run(MyHook)
}
```

The `run()` function:

1. Reads symposium canonical JSON from stdin.
2. Deserializes it into an `Input` event.
3. Calls `handler.handle_event()`, which dispatches to the appropriate method.
4. Serializes the output to stdout.

You only need to override the methods you care about — unimplemented methods return the default (empty) output for their event type.

`tool_name` arrives exactly as the agent sent it, and `tool_input` keeps the agent's own argument names, so check every name you want to handle (Antigravity's shell, for example, takes `CommandLine` rather than `command`); see [Matcher](../reference/hook-events.md#matcher).

## Step 3. Register it in your plugin manifest

Publish the handler to crates.io, then reference it as a hook command in your plugin's `SYMPOSIUM.toml`:

```toml
[[hooks]]
name = "check-usage"
event = "PreToolUse"
matcher = "^(Bash|bash|run_command|execute_bash)$"
command = { source = "cargo", crate = "my-crate-hooks", executable = "my-crate-hooks" }
```

Symposium installs the binary (with `cargo binstall` or `cargo install`) the first time the hook runs. A manifest shipped in your crate needs nothing else; a recommendations-repository entry also sets `name` and `depends-on` (see [Authoring a plugin](./authoring-a-plugin.md)).

## Output types

Each handler method returns its event-specific output type:

| Method | Return type | Key fields |
|--------|-------------|------------|
| `pre_tool_use` | `PreToolUseOutput` | `decision`, `additional_context`, `updated_input` |
| `post_tool_use` | `PostToolUseOutput` | `additional_context` |
| `user_prompt_submit` | `UserPromptSubmitOutput` | `additional_context` |
| `session_start` | `SessionStartOutput` | `additional_context` |
| `stop` | `StopOutput` | `additional_context` |

Each output type has convenience constructors:

- `::default()`: empty output, no-op.
- `::context("...")`: inject text into the agent's context. On `Stop`, agents treat this output differently; see [`Stop` output](../reference/hook-events.md#stop-output).
- `PreToolUseOutput::with_updated_input(value)`: replace the tool input.
- `PreToolUseOutput::deny("reason")`: block the tool call with a reason.

Return `Err(...)` from any method to report an error (exit code 1, message on stderr).

## The `HookHandler` trait

```rust
pub trait HookHandler {
    async fn handle_event(&self, input: &Input) -> anyhow::Result<Output> { /* dispatches */ }
    async fn pre_tool_use(&self, event: &PreToolUseInput) -> anyhow::Result<PreToolUseOutput> { /* default */ }
    async fn post_tool_use(&self, event: &PostToolUseInput) -> anyhow::Result<PostToolUseOutput> { /* default */ }
    async fn user_prompt_submit(&self, event: &UserPromptSubmitInput) -> anyhow::Result<UserPromptSubmitOutput> { /* default */ }
    async fn session_start(&self, event: &SessionStartInput) -> anyhow::Result<SessionStartOutput> { /* default */ }
    async fn stop(&self, event: &StopInput) -> anyhow::Result<StopOutput> { /* default */ }
}
```

Override `handle_event` only if you need custom dispatch logic (e.g., shared state across events). Otherwise, just override the per-event methods.

## Testing locally

You can test your handler by piping JSON directly:

```bash
cargo build
echo '{"PreToolUse":{"tool_name":"Bash","tool_input":{"command":"rm -rf /"},"session_id":null,"cwd":"/tmp"}}' \
  | ./target/debug/my-crate-hooks
```

To run it through Symposium the way an agent does, see [Try it in a project](./authoring-a-plugin.md#step-4-try-it-in-a-project).

## Example: blocking destructive commands

This example reads Claude Code's `Bash` tool; to cover other agents, check their tool names as in Step 2 and read Antigravity's `CommandLine` as well.

```rust
{{#include ../../symposium-sdk/examples/block_destructive.rs}}
```

## Example: injecting context on session start

```rust
{{#include ../../symposium-sdk/examples/inject_context.rs}}
```
