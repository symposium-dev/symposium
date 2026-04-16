# Plugin definition reference

A plugin is a TOML manifest loaded from a configured plugin source. It can be a standalone `.toml` file or a `symposium.toml` inside a directory.

## Minimal manifest

```toml
name = "example"

[[skills]]
crates = ["serde"]
source.path = "skills"
```

## Top-level fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Plugin name. Used in logs and CLI output. |
| `session-start-context` | string | no | Text injected into the agent's context at session start. See [Session start context](#session-start-context). |

## `[[skills]]` groups

Each `[[skills]]` entry declares a group of skills.

| Field | Type | Description |
|-------|------|-------------|
| `crates` | string or array | Which crates this group advises on. Accepts a single string (`"serde"`) or array (`["serde", "tokio>=1.0"]`). See [Skill matching](./skill-matching.md) for atom syntax. |
| `source.path` | string | Local directory containing skill subdirectories. Resolved relative to the manifest file. |
| `source.git` | string | GitHub URL pointing to a directory in a repository (e.g., `https://github.com/org/repo/tree/main/skills`). Symposium downloads the tarball, extracts the subdirectory, and caches it. |

A skill group must have exactly one of `source.path` or `source.git`.

## `[[hooks]]`

Each `[[hooks]]` entry declares a hook.

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Descriptive name for the hook (used in logs). |
| `event` | string | Event type to match (e.g., `PreToolUse`). |
| `matcher` | string | Which tool invocations to match (e.g., `Bash`). Omit to match all. |
| `command` | string | Command to run when the hook fires. Resolved relative to the plugin directory. |

## Session start context

The `session-start-context` field lets a plugin inject text into the agent's conversation context when a session begins. This is useful for critical guidance that the agent should see before doing any work.

```toml
name = "rust-guidance"
session-start-context = "**Critical:** Before authoring Rust code, run `cargo agents start` for instructions."
```

When multiple plugins provide `session-start-context`, all of their texts are combined (separated by blank lines) and returned to the agent as additional context.

This works via the `SessionStart` hook event. When the agent starts a session, Symposium collects `session-start-context` from all loaded plugins — including both user-level and project-level plugin sources — and returns the combined text.

## Example: full manifest

```toml
name = "widgetlib"

[[skills]]
crates = ["widgetlib=1.0"]
source.path = "skills/general"

[[skills]]
crates = ["widgetlib=1.0"]
source.git = "https://github.com/org/widgetlib/tree/main/symposium/serde-skills"

[[hooks]]
name = "check-widget-usage"
event = "PreToolUse"
matcher = "Bash"
command = "./scripts/check-widget.sh"
```

## Validation

```bash
cargo agents plugin validate path/to/symposium.toml
```

This parses the manifest and reports any errors. Use `--check-crates` to also verify that crate names exist on crates.io.
