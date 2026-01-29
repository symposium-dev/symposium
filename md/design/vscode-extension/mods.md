# Mod UI (VSCode)

This chapter covers VSCode-specific UI for managing mods. For general mod concepts, see [Agent Mods](../mods.md).

## Configuration Storage

Mods are configured via the `symposium.mods` VS Code setting:

```json
"symposium.mods": [
  { "id": "sparkle", "_enabled": true, "_source": "built-in" },
  { "id": "ferris", "_enabled": true, "_source": "built-in" },
  { "id": "cargo", "_enabled": true, "_source": "built-in" }
]
```

Custom mods include their distribution:

```json
{
  "id": "my-mod",
  "_enabled": true,
  "_source": "custom",
  "name": "My Mod",
  "distribution": {
    "npx": { "package": "@myorg/my-mod" }
  }
}
```

**Default behavior** - when no setting exists, all built-in mods are enabled. If the user returns to the default configuration, the key is removed from settings.json entirely.

## Settings UI

The Settings panel includes a Mods section where users can:

- **Enable/disable** mods via checkbox
- **Reorder** mods by dragging the handle
- **Delete** mods from the list
- **Add** mods via the "+ Add mod" link, which opens a QuickPick dialog

### Add Mod Dialog

The QuickPick dialog shows three sections:

1. **Built-in** - sparkle, ferris, cargo (greyed out if already added)
2. **From Registry** - mods from the shared registry with `type: "mod"`
3. **Add Custom Mod**:
   - From executable on your system (local command/path)
   - From npx package
   - From pipx package
   - From cargo crate
   - From URL to mod.json (GitHub URLs auto-converted to raw)

## Spawn Integration

When spawning an agent, the extension builds `--proxy` arguments from enabled mods:

```bash
symposium-acp-agent run-with --proxy sparkle --proxy ferris --proxy cargo --agent '...'
```

Only enabled mods are passed, in their configured order.
