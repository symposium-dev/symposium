# `cargo agents status`

Show installed crates and active plugins for the current workspace.

## Usage

```bash
cargo agents status
```

## Behavior

Displays a summary of the current Symposium state:

1. **Installed crates** — Lists each installed plugin crate, its version (or tracking policy), and how many plugins/skills it contributes.
2. **Active in this workspace** — Shows which plugins/skills are active for the current workspace, grouped by source crate. Includes predicate evaluation results (why something is or isn't active).
3. **Inactive (predicates not met)** — Shows plugins/skills that are installed but not active because their predicates don't match the current workspace.
4. **Configured agents** — Which agents are registered and receiving skills.

## Example output

```
Installed crates:
  symposium-recommendations  v0.4.2 (latest)  — 12 plugins
  my-org-plugins             v1.0.0 (pinned)  —  3 plugins

Active in this workspace:
  symposium-recommendations:
    serde-skill          (crate: serde >=1.0 ✓)
    tokio-skill          (crate: tokio ✓)
    testing-skill        (unconditional)
  my-org-plugins:
    internal-api-skill   (crate: my-internal-api ✓)

Inactive (predicates not met):
  symposium-recommendations:
    diesel-skill         (crate: diesel — not in workspace)
    rocket-skill         (crate: rocket — not in workspace)

Agents: claude, gemini
```

## Options

| Flag | Description |
|------|-------------|
| `--json` | Output structured JSON instead of human-readable format |
| `-v`, `--verbose` | Show full predicate evaluation details |

## See also

- [`cargo agents sync`](./cargo-agents-sync.md) — synchronize skills with workspace dependencies
- [`cargo agents install`](./cargo-agents-install.md) — install a plugin crate
