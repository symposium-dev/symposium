# VSCode and VSCode-based editors

## Step 1: Install the extension

Install the Symposium extension from:

- [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=symposium-dev.symposium)
- [Open VSX Registry](https://open-vsx.org/extension/symposium/symposium)

<img src="./artwork/screenshots/vscode-install-panel.jpg" alt="Installing Symposium extension" class="screenshot-centered"/>

## Step 2: Activate the panel and start chatting

Click the Symposium icon in the activity bar to open the panel, then click **New tab** to start a conversation.

On first run, Symposium will guide you through selecting an agent. After that, you can change your agent or mods at any time by typing `/symposium:config` in the chat.

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
