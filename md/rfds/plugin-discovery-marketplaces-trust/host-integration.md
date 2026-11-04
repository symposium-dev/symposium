# Appendix: Claude Code and Codex marketplaces

This appendix describes the native mechanisms behind the proposal and the adapter we intend to build. Documentation was checked on 2026-11-04. The examples are proposed Symposium output, not evidence of successful end-to-end integration; supported host versions still need the admission experiments below.

## Configuration files and Git behavior

| Host and scope | Catalog or registration | Enablement | Effect on the checkout |
|---|---|---|---|
| Claude, user | Directory catalog registered in `~/.claude/settings.json` | User-scoped `enabledPlugins` | User settings are outside the checkout |
| Claude, local | `extraKnownMarketplaces` in `.claude/settings.local.json` | Local-scoped `enabledPlugins` in that file | Claude excludes the file when it creates it; a direct Symposium write needs an exclusion check |
| Claude, shared project | Registration in `.claude/settings.json` | Project-scoped `enabledPlugins` | Can modify tracked team configuration; avoid for generated personal choices |
| Codex, personal | `~/.agents/plugins/marketplace.json` | `~/.codex/config.toml` | Outside the checkout |
| Codex, project | `.agents/plugins/marketplace.json` | `.codex/config.toml` | Neither generated path can be assumed ignored; tracked settings can become dirty |

Codex's documented local enablement file is `.codex/config.toml`. We have not verified `.codex/agents.json` as an alternative. The issue is the need to edit project configuration, regardless of the filename. [Codex packaging reference](https://developers.openai.com/plugins/build/plugins).

Claude's distinction is conditional: when Claude first writes `.claude/settings.local.json`, it arranges a global Git exclusion. Its documentation explicitly says manually created files need an ignore rule. Thus using native local-scope commands may avoid checkout noise, but direct generation by Symposium must not assume the same result. Already tracked files remain tracked despite ignore rules. [Claude settings reference](https://code.claude.com/docs/en/settings#keep-personal-settings-out-of-a-repository).

### Possible mitigation: exclude files Symposium creates

Symposium could add repository-local Git exclusions for newly created, machine-local files. For example, if it creates `.codex/config.toml` or `.agents/plugins/marketplace.json`, it could add the corresponding root-relative path to the repository's local exclude file. This applies the same idea as Claude's exclusion while keeping the effect local to that repository rather than changing global Git configuration.

Only exclude exact paths to files Symposium created, preserving existing exclude rules. Do not blanket-ignore `.codex/` or hide pre-existing user configuration. Check ownership and tracking before writing: an already tracked file will still produce a diff, and ignoring it cannot hide just the Symposium-generated keys. Report that remaining limitation to the user.

The implementation should locate the exclude file through Git rather than assuming `.git` is a directory, and test linked worktrees explicitly. Record which exclusions Symposium added so cleanup can preserve user-owned rules. This is a proposed mitigation to validate, not a change to Git settings made by this RFD.

## Claude Code

### Catalog and registration

A directory marketplace holds `.claude-plugin/marketplace.json`. For example, Symposium could generate this under `.symposium/claude-marketplace/`:

```json
{
  "name": "symposium-local-example",
  "owner": { "name": "Symposium" },
  "plugins": [
    { "name": "rust-helper", "source": "./plugins/rust-helper" }
  ]
}
```

Here the plugin lives at `.symposium/claude-marketplace/plugins/rust-helper/`; the source is relative to the marketplace root. Registration makes the catalog available; installation selects an entry. [Creating a marketplace](https://code.claude.com/docs/en/plugin-marketplaces).

The proposed local registration in `.claude/settings.local.json` is:

```json
{
  "extraKnownMarketplaces": {
    "symposium-local-example": {
      "source": {
        "source": "directory",
        "path": "/absolute/workspace/.symposium/claude-marketplace"
      }
    }
  }
}
```

A directory source names the directory containing `.claude-plugin/marketplace.json`. An absolute path here avoids assuming how relative settings paths resolve; its machine-specific nature is another reason to keep this registration local. [Marketplace source reference](https://code.claude.com/docs/en/plugins/marketplace-reference).

### Installation, enablement, and refresh

The native CLI offers separate operations with explicit scope:

```bash
claude plugin marketplace add /absolute/workspace/.symposium/claude-marketplace --scope local
claude plugin install rust-helper@symposium-local-example --scope local
claude plugin enable rust-helper@symposium-local-example --scope local
claude plugin disable rust-helper@symposium-local-example --scope local
```

Use `--scope user` for global selection; `--scope project` affects shared project settings. These commands are an adapter candidate, subject to noninteractive and hook-path validation. [Plugin command reference](https://code.claude.com/docs/en/plugins/cli-reference).

The corresponding enablement setting is separate from registration:

```json
{
  "enabledPlugins": {
    "rust-helper@symposium-local-example": true
  }
}
```

Writing this setting alone is not proof of installation or loading. Users can browse through `/plugin`. For directory marketplaces with relative plugin sources, Claude documents reading the local plugin directly and picking up edits on a new session or `/reload-plugins`; hosted marketplace installs use cached copies. [Marketplace development behavior](https://code.claude.com/docs/en/plugin-marketplaces).

Use stable workspace-specific internal marketplace names. The Zulip discussion reports earlier trouble with shared names, cache refresh, and project settings. A common display label must not make one workspace replace another's catalog.

## Codex

### Catalog and registration

Symposium would generate the project catalog at `.agents/plugins/marketplace.json`:

```json
{
  "name": "symposium-local-example",
  "interface": { "displayName": "Symposium (Local)" },
  "plugins": [
    {
      "name": "rust-helper",
      "source": {
        "source": "local",
        "path": "./.symposium/plugins/rust-helper"
      },
      "policy": { "installation": "AVAILABLE", "authentication": "ON_INSTALL" },
      "category": "Development"
    }
  ]
}
```

The relative path starts at the repository's marketplace root, not `.agents/plugins/`. For personal discovery, use `~/.agents/plugins/marketplace.json`. These host `policy` fields do not encode Symposium's four vouch levels. [Packaging and marketplaces](https://developers.openai.com/plugins/build/plugins).

### Enablement, installation, and refresh

Project enablement is declarative:

```toml
[plugins."rust-helper@symposium-local-example"]
enabled = true
```

The key couples the plugin and marketplace names. Global settings use `~/.codex/config.toml`. Project configuration requires trust. Refresh can fetch configured plugins even when disabled, so installation and activation remain distinct. The documented local plugin flow loads a cached copy; source edits need refresh/restart validation. [Local marketplace behavior](https://developers.openai.com/plugins/build/plugins).

The background notes also identify marketplace-management commands:

```bash
codex plugin marketplace add /absolute/path/to/marketplace-root
codex plugin marketplace list
codex plugin marketplace upgrade symposium-local-example
codex plugin marketplace remove symposium-local-example
```

These manage catalogs; they do not establish a scoped per-plugin installation equivalent to Claude's command. Validate fresh automatic activation from a generated catalog plus TOML before relying on it.

Existing personal/repository catalogs must be preserved. If a separate Symposium source cannot coexist, merge only owned entries or report a collision. Never replace an unrelated catalog wholesale.

## Shared adapter requirements

The global and local marketplaces are conceptual scopes; hosts may need different physical representations. Store materialized packages and generated catalogs in Symposium-owned locations. Generated settings outside those locations need explicit ownership records and narrow edits. Do not gitignore an entire user configuration directory to hide a few generated entries.

Map a host's plugin identity back to the canonical PM/source identity and version. Keep native user intent separate from the last generated configuration. Use the same effective policy decision for catalog publication, installation, and hook validation.

Keep host-specific plugin metadata separate from authored content. A plugin's unsupported components need diagnostics, not silent omission or an undocumented expansion of scope. Installation may require authentication or host approval; report a pending action rather than bypassing it.

## Admission experiments

Run in temporary homes/configuration directories and disposable workspaces. Record host/client version, starting directory, commands, generated configuration, observed native choices, and actual loaded components. A plausible JSON or TOML file is not sufficient evidence.

| Experiment | Required observation |
|---|---|
| Fresh workspace, allowed discovery, auto-enable on | Plugin is discoverable, installed, and loaded, or explicitly pending reload |
| Same discovery, auto-enable off | Browser offers the plugin without activating it |
| Native optional enable/disable followed by two syncs and restart | Explicit choice persists and is not mistaken for missing configuration |
| Global selection, then alternating matching/nonmatching workspace syncs | No workspace removes or rewrites the global selection based on its predicates |
| Two local catalogs with identically named plugins | No cross-project cache or registration collision |
| Same plugin selected globally and locally, including different versions | One effective instance or an explicit conflict, never duplicate execution |
| Start at repository root and at a nested package | Documented workspace visibility; no assumed parent-directory discovery |
| Add a dependency during a session | Record whether refresh works or restart is required |
| Existing user catalog and unrelated config keys | Both remain intact after sync and removal |
| Clean Git checkout, with generated settings both untracked and already tracked | Record `git status` before/after; verify Claude-created versus Symposium-created local settings exclusions, test exact-path exclusions for newly created Codex files in ordinary repositories and linked worktrees, preserve pre-existing files/rules, and report unavoidable tracked-file changes |
| Denied version, including a formerly enabled global plugin | No new installation; existing violating state is disabled or reported as unenforceable |
| Required plugin disabled through native controls | Independent Symposium hook detects the violation and its actual blocking behavior is verified |
| Warning without acknowledgment | Native browsing/installation cannot silently bypass the gate; otherwise document unsupported enforcement |
| Revoked acknowledgment or changed audit evidence | No stale approval authorizes subsequent activation |
| Unreadable source, policy-service failure, interrupted/concurrent sync | No destructive empty-catalog cleanup or accidental new authorization |

The session-start ordering deserves its own check: the Zulip discussion reports agents loading plugins before Symposium's `SessionStart` sync. Verify actual first-prompt visibility after a fresh clone and after an update. A reload notice is an acceptable initial limitation for optional activation; an unloaded required plugin cannot be reported as compliant.
