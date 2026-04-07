# Adding a Plugin

A plugin is a TOML manifest loaded from a configured plugin source.

Plugin sources can be:

- a local directory
- a GitHub repository cached under `~/.symposium/cache/plugin-sources/`

Local plugin manifests are usually placed in `~/.symposium/plugins/`.

## Minimal manifest

```toml
name = "example"

[[skills]]
crates = ["serde"]
source.path = "skills"
```

This declares one skill group for the `serde` crate. `source.path` is resolved relative to the manifest path.

## Hooks

Plugins can also declare hooks:

```toml
[[hooks]]
name = "inspect-tool"
event = "PreToolUse"
matcher = "Bash"
command = "./scripts/check-tool.sh"
```

When `symposium hook <agent> pre-tool-use` receives a matching payload, the command is started with the payload JSON on stdin.

Supported agent names: `claude`, `copilot`, `gemini`. Invoke the hook runner with the agent and event, for example:

```
symposium hook claude pre-tool-use
```

## Validation

Use:

```bash
symposium plugin validate path/to/plugin.toml
```

This parses the manifest and prints the normalized TOML.
