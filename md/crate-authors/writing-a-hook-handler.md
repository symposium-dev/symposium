# Writing a hook handler

This guide walks through writing a symposium hook handler in Rust using the `symposium-hook` crate.

## Step 1. Create a new binary crate

Create your new crate:

```bash
cargo new my-hook-handler
cd my-hook-handler
```

And then add symposium-hook to your dependencies:

```bash
cargo add symposium-hook
```

## Step 2. Write the handler

A hook handler is a program that reads a JSON event on stdin and writes a JSON response to stdout. The `symposium-hook` crate provides a `HookHandler` trait and a `run()` harness that handles the plumbing.

Implement `HookHandler` and override the methods for the events you care about:

```rust
// src/main.rs
use std::process::ExitCode;
use symposium_hook::{HookHandler, PreToolUseInput, PreToolUseOutput, run};

struct MyHook;

impl HookHandler for MyHook {
    fn pre_tool_use(&self, event: &PreToolUseInput) -> anyhow::Result<PreToolUseOutput> {
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

In your `SYMPOSIUM.toml`, reference the built binary as a hook command:

```toml
name = "my-crate"
crates = ["my-crate"]

[[hooks]]
name = "check-usage"
event = "PreToolUse"
command = { source = "cargo", crate = "my-hook-handler", executable = "my-hook-handler" }
```

## Output types

Each handler method returns its event-specific output type:

| Method | Return type | Key fields |
|--------|-------------|------------|
| `pre_tool_use` | `PreToolUseOutput` | `additional_context`, `updated_input` |
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
    fn handle_event(&self, input: &Input) -> anyhow::Result<Output> { /* dispatches */ }
    fn pre_tool_use(&self, event: &PreToolUseInput) -> anyhow::Result<PreToolUseOutput> { /* default */ }
    fn post_tool_use(&self, event: &PostToolUseInput) -> anyhow::Result<PostToolUseOutput> { /* default */ }
    fn user_prompt_submit(&self, event: &UserPromptSubmitInput) -> anyhow::Result<UserPromptSubmitOutput> { /* default */ }
    fn session_start(&self, event: &SessionStartInput) -> anyhow::Result<SessionStartOutput> { /* default */ }
}
```

Override `handle_event` only if you need custom dispatch logic (e.g., shared state across events). Otherwise, just override the per-event methods.

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
{{#include ../../symposium-hook/examples/block_destructive.rs}}
```

## Example: injecting context on session start

```rust
{{#include ../../symposium-hook/examples/inject_context.rs}}
```
