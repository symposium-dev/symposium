# Writing a hook handler

This guide walks through writing a symposium hook handler in Rust using the `symposium-sdk` crate.

## Step 1. Create a new binary crate

Create your new crate:

```bash
cargo new my-hook-handler
cd my-hook-handler
```

And then add symposium-sdk to your dependencies:

```bash
cargo add symposium-sdk
```

## Step 2. Write the handler

A hook handler is a program that reads a JSON event on stdin and writes a JSON response to stdout. The `symposium-sdk` crate provides a `HookHandler` trait and a `run()` harness that handles the plumbing.

Implement `HookHandler` and override the methods for the events you care about:

```rust
// src/main.rs
use std::process::ExitCode;
use symposium_sdk::hook::{HookHandler, PreToolUseInput, PreToolUseOutput, run};

struct MyHook;

impl HookHandler for MyHook {
    async fn pre_tool_use(&self, event: &PreToolUseInput) -> anyhow::Result<PreToolUseOutput> {
        if event.tool_name == "Bash" {
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

## Step 3. Register it in your plugin manifest

In your `Symposium.toml`, reference the built binary as a hook command:

```toml
name = "my-crate"
depends-on = ["my-crate"]

[[hooks]]
name = "check-usage"
event = "PreToolUse"
command = { source = "cargo", crate = "my-hook-handler", executable = "my-hook-handler" }
```

## Output types

Each handler method returns its event-specific output type:

| Method | Return type | Key fields |
|--------|-------------|------------|
| `pre_tool_use` | `PreToolUseOutput` | `decision`, `additional_context`, `updated_input` |
| `post_tool_use` | `PostToolUseOutput` | `additional_context` |
| `user_prompt_submit` | `UserPromptSubmitOutput` | `additional_context` |
| `session_start` | `SessionStartOutput` | `additional_context` |

Each output type has convenience constructors:

- `::default()` — empty output, no-op.
- `::context("...")` — inject text into the agent's context.
- `PreToolUseOutput::with_updated_input(value)` — replace the tool input.
- `PreToolUseOutput::deny("reason")` — block the tool call with a reason.

Return `Err(...)` from any method to report an error (exit code 1, message on stderr).

## The `HookHandler` trait

```rust
pub trait HookHandler {
    async fn handle_event(&self, input: &Input) -> anyhow::Result<Output> { /* dispatches */ }
    async fn pre_tool_use(&self, event: &PreToolUseInput) -> anyhow::Result<PreToolUseOutput> { /* default */ }
    async fn post_tool_use(&self, event: &PostToolUseInput) -> anyhow::Result<PostToolUseOutput> { /* default */ }
    async fn user_prompt_submit(&self, event: &UserPromptSubmitInput) -> anyhow::Result<UserPromptSubmitOutput> { /* default */ }
    async fn session_start(&self, event: &SessionStartInput) -> anyhow::Result<SessionStartOutput> { /* default */ }
}
```

Override `handle_event` only if you need custom dispatch logic (e.g., shared state across events). Otherwise, just override the per-event methods.

## Running directly under an agent

To use the same handler in a standalone binary that an agent invokes directly, replace the call to `run` with `hook_main`:

```rust
fn main() -> std::process::ExitCode {
    symposium_sdk::hook::hook_main(MyHook)
}
```

`hook_main` translates agent-native input into the same `Input` your handler already uses and translates its output back. With no arguments, it tries Claude Code, then Codex, then Kiro, choosing the first adapter that accepts the input. Overlapping inputs use Claude's protocol. Only input decoding is retried; the handler runs once.

An agent's hook registration can select a protocol explicitly:

```text
my-hook-handler --agent=claude
my-hook-handler --agent=codex
my-hook-handler --agent=kiro
my-hook-handler --agent=copilot --event=pre-tool-use
my-hook-handler --agent=antigravity --event=pre-tool-use
```

`--event` supplies a missing event name; canonical names such as `pre-tool-use` and agent-native names are accepted. If the input already names a different event, the invocation fails. Explicit `--agent` selection does not fall back on invalid input. `--agent=guess` is equivalent to omitting the option.

To invoke this binary from a Symposium-format plugin hook, include `--agent=symposium` in its command arguments. Existing binaries using `run` continue to read canonical input without any flags.

Context and pre-tool decisions use each agent's output convention. In particular, Kiro context is plain stdout, and a denied tool call exits with code 2 and the reason on stderr. Input replacement is unsupported by the Kiro and Copilot adapters; requesting that operation reports an error. Keep diagnostic messages on stderr so they do not corrupt the response.

The SDK does not register the binary with agents or provide a shared hook installation directory. Tool names and arguments also remain agent-specific: the example's `Bash` check only matches agents that use that tool name. See [Hooks](../design/hooks.md#standalone-portable-hooks) for the adapter contract.

## Testing locally

You can test your handler by piping JSON directly:

```bash
cargo build
echo '{"PreToolUse":{"tool_name":"Bash","tool_input":{"command":"rm -rf /"},"session_id":null,"cwd":"/tmp"}}' \
  | ./target/debug/my-hook-handler
```

Or via the symposium CLI:

```bash
echo '{"PreToolUse":{"tool_name":"Bash","tool_input":{"command":"rm -rf /"},"session_id":null,"cwd":"/tmp"}}' \
  | cargo agents hook symposium pre-tool-use
```

## Example: blocking destructive commands

```rust
{{#include ../../symposium-sdk/examples/block_destructive.rs}}
```

## Example: injecting context on session start

```rust
{{#include ../../symposium-sdk/examples/inject_context.rs}}
```
