# VSCode and VSCode-based editors

## Step 1: Install the extension

Install the Symposium extension from:

- [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=symposium-dev.symposium)
- [Open VSX Registry](https://open-vsx.org/extension/symposium/symposium)

<img src="./artwork/screenshots/vscode-install-panel.jpg" alt="Installing Symposium extension" class="screenshot-centered"/>

## Step 2: Activate the panel and start chatting

<div class="panel-guide">
<img src="./artwork/screenshots/screenshot-vscode-panel.jpg" alt="Symposium panel in VSCode"/>
<div class="panel-legend">

1. **Activity bar icon** — Click to open the Symposium panel
2. **New tab** — Start a new conversation with the current settings
3. **Settings** — Expand to configure agent and extensions
4. **Agent selector** — Choose which agent to use (Claude Code, Gemini CLI, etc.)
5. **Extensions** — Enable MCP servers that add capabilities to your agent
6. **Add extension** — Add custom extensions
7. **Edit all settings** — Access full settings

</div>
</div>

## Custom Agents

The agent selector shows agents from the Symposium registry. To add a custom agent not in the registry, use VS Code settings.

Open **Settings** (Cmd/Ctrl+,) and search for `symposium.agents`. Add your custom agent to the JSON array:

```json
"symposium.agents": [
  {
    "id": "my-custom-agent",
    "name": "My Custom Agent",
    "distribution": {
      "npx": { "package": "@myorg/my-agent" }
    }
  }
]
```

### Distribution Types

| Type | Example | Notes |
|------|---------|-------|
| `npx` | `{ "npx": { "package": "@org/pkg" } }` | Requires npm/npx installed |
| `pipx` | `{ "pipx": { "package": "my-agent" } }` | Requires pipx installed |
| `executable` | `{ "executable": { "path": "/usr/local/bin/my-agent" } }` | Local binary |

Your custom agent will appear in the agent selector alongside registry agents.
