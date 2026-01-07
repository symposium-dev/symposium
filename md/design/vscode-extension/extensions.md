# Agent Extensions

Agent extensions are proxy components that enrich the agent's capabilities. The VS Code extension allows users to configure which extensions are active, their order, and to add custom extensions.

## Built-in Extensions

| ID | Name | Description |
|----|------|-------------|
| `sparkle` | Sparkle | AI collaboration identity and embodiment |
| `ferris` | Ferris | Rust development tools (crate sources, rust researcher) |
| `cargo` | Cargo | Cargo build and run tools |

## Extension Sources

Extensions can come from three sources:

- **built-in**: Bundled with Symposium (sparkle, ferris, cargo)
- **registry**: Installed from the shared agent registry
- **custom**: User-defined via executable, npx, pipx, or URL

## Configuration

Extensions are configured via the `symposium.extensions` VS Code setting:

```json
"symposium.extensions": [
  { "id": "sparkle", "_enabled": true, "_source": "built-in" },
  { "id": "ferris", "_enabled": true, "_source": "built-in" },
  { "id": "cargo", "_enabled": true, "_source": "built-in" }
]
```

Custom extensions include their distribution:

```json
{
  "id": "my-extension",
  "_enabled": true,
  "_source": "custom",
  "name": "My Extension",
  "distribution": {
    "npx": { "package": "@myorg/my-extension" }
  }
}
```

**Order matters** - extensions are applied in the order listed. The first extension in the list is closest to the editor, and the last is closest to the agent.

**Default behavior** - when no setting exists, all built-in extensions are enabled. If the user returns to the default configuration, the key is removed from settings.json entirely.

## Settings UI

The Settings panel includes an Extensions section where users can:

- **Enable/disable** extensions via checkbox
- **Reorder** extensions by dragging the handle
- **Delete** extensions from the list
- **Add** extensions via the "+ Add extension" link, which opens a QuickPick dialog

### Add Extension Dialog

The QuickPick dialog shows three sections:

1. **Built-in** - sparkle, ferris, cargo (greyed out if already added)
2. **From Registry** - extensions from the shared registry with `type: "extension"`
3. **Add Custom Extension**:
   - From executable on your system (local command/path)
   - From npx package
   - From pipx package
   - From URL to extension.json (GitHub URLs auto-converted to raw)

## CLI Interface

The VS Code extension passes extension configuration to `symposium-acp-agent` via `--proxy` arguments:

```bash
symposium-acp-agent --proxy sparkle --proxy ferris --proxy cargo -- npx @zed-industries/claude-code-acp
```

Only enabled extensions are passed, in their configured order.

## Registry Format

The shared registry includes both agents and extensions:

```json
{
  "date": "2026-01-07",
  "agents": [...],
  "extensions": [
    {
      "id": "some-extension",
      "name": "Some Extension",
      "version": "1.0.0",
      "description": "Does something useful",
      "distribution": {
        "npx": { "package": "@example/some-extension" }
      }
    }
  ]
}
```

## Architecture

```
┌─────────────────────────────────────────────────┐
│  VS Code Extension                              │
│  - Reads symposium.extensions setting           │
│  - Fetches registry extensions                  │
│  - Renders UI in Settings panel                 │
│  - Shows QuickPick for adding extensions        │
│  - Builds --proxy args for agent spawn          │
└─────────────────┬───────────────────────────────┘
                  │
┌─────────────────▼───────────────────────────────┐
│  symposium-acp-agent                            │
│  - Parses --proxy arguments                     │
│  - Validates proxy names                        │
│  - Builds proxy chain in specified order        │
└─────────────────┬───────────────────────────────┘
                  │
┌─────────────────▼───────────────────────────────┐
│  symposium-acp-proxy (Symposium struct)         │
│  - from_proxy_names() creates config            │
│  - build_proxies() instantiates components      │
│  - Conductor orchestrates the chain             │
└─────────────────────────────────────────────────┘
```

## Future Work

- **Per-extension configuration**: Add sub-options for extensions (e.g., which Ferris tools to enable)
- **Extension updates**: Check for and apply updates to registry-sourced extensions
