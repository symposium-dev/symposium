# Configuration

Your agent choice is global across all workspaces. Agent mods are configured per-workspace. You can update both by running `/symposium:config` in the chat.

## Using `/symposium:config`

Running `/symposium:config` shows your current configuration:

```
Configuration

**Agent:** claude-code-acp

**Mods for workspace `/Users/nikomat/dev/symposium`:**
  1. symposium-cargo
  2. symposium-rust-analyzer
  3. sparkle-mcp

# Commands

- `AGENT` - Change agent (affects all workspaces)
- `1` through `3` - Toggle mod enabled/disabled in this workspace
- `SAVE` - Save for future sessions
- `CANCEL` - Exit without saving
```

### Commands

| Command | Description |
|---------|-------------|
| `AGENT` | Select a different agent from the registry. Your choice applies to all workspaces. |
| `1`, `2`, etc. | Toggle a mod on or off. Disabled mods stay in the list but aren't loaded. |
| `SAVE` | Write changes to disk. They persist across sessions. |
| `CANCEL` | Discard changes and exit. |

Changes take effect immediately for the current session. Use `SAVE` to keep them for future sessions.

## Configuration Location

Symposium stores configuration in platform-specific directories:

| Platform | Configuration Directory |
|----------|------------------------|
| Linux | `~/.config/symposium/` |
| macOS | `~/Library/Application Support/symposium/` |
| Windows | `%APPDATA%\symposium\` |

## Directory Structure

```
<config_dir>/
├── config/
│   ├── agent.json                    # Selected agent (global)
│   ├── recommendations.toml          # Your local recommendations
│   └── <workspace-hash>/
│       └── config.json               # Per-workspace mod configuration
├── cache/
│   └── recommendations.toml          # Cached remote recommendations
└── bin/
    └── <crate-name>/
        └── <version>/                # Downloaded binaries
```

## Adding Your Own Recommendations

You can add your own recommendations that Symposium suggests:

- **For all workspaces**: Create a `recommendations.toml` file in the `config/` directory
- **For a specific project**: Create a `.symposium/recommendations.toml` file in the project root

Workspace recommendations are useful for team projects where you want to suggest internal mods to collaborators. See [Recommending Mods](../mods/recommending-mods.md) for the file format.

## Binary Cache

When you select an agent or mod distributed via `cargo`, Symposium installs it to `bin/<crate-name>/<version>/`. This cache:

- Stores one version per crate (old versions are cleaned up automatically)
- Uses `cargo binstall` for pre-built binary downloads when available
- Falls back to `cargo install` (building from source) otherwise

To force a reinstall, delete the crate's directory in `bin/`.
