# Proposed: Native marketplace integration

This is an adapter plan, not a record of successful end-to-end validation. The supplied integration notes suggest the mechanisms below. Official documentation was checked on 2026-11-04; supported host versions and runtime behavior must still be recorded by implementation experiments.

## Claude Code

Generate a directory marketplace with `.claude-plugin/marketplace.json` and register it at the appropriate user or project scope. The [marketplace documentation](https://code.claude.com/docs/en/plugin-marketplaces) describes the catalog mechanism. Prefer local user-specific project configuration for generated workspace registration; do not overwrite shared project settings.

The implementation notes propose scoped CLI installation and enablement. Validate noninteractive behavior before invoking these operations from synchronization, particularly from a hook. Merely writing an enablement setting must not be treated as downloading an absent plugin.

Use stable workspace-specific internal marketplace names. The work-organization discussion reports earlier trouble with shared names, cache refresh, and project settings. Reusing the display name Symposium (Local) must not make one workspace's catalog replace another's.

## Codex

The [plugin packaging documentation](https://developers.openai.com/plugins/build/plugins) describes `.agents/plugins/marketplace.json` for a repository and `~/.agents/plugins/marketplace.json` for a personal catalog. Project enablement uses `.codex/config.toml`:

```toml
[plugins."rust-helper@symposium-local-example"]
enabled = true
```

Local `source.path` values resolve against the marketplace root, not its `.agents/plugins/` directory. Project configuration requires project trust. The documented refresh behavior can install or refresh configured plugins even when disabled, so downloaded files are not evidence of authorization or activation.

These mechanisms are candidates for the adapter. Test them with the supported Codex clients before depending on automatic installation. Existing personal/repository catalogs must be preserved; if a separate registered Symposium source cannot coexist, merge only owned entries or report a collision. Never replace an unrelated catalog wholesale.

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
| Denied version, including a formerly enabled global plugin | No new installation; existing violating state is disabled or reported as unenforceable |
| Required plugin disabled through native controls | Independent Symposium hook detects the violation and its actual blocking behavior is verified |
| Warning without acknowledgment | Native browsing/installation cannot silently bypass the gate; otherwise document unsupported enforcement |
| Revoked acknowledgment or changed audit evidence | No stale approval authorizes subsequent activation |
| Unreadable source, policy-service failure, interrupted/concurrent sync | No destructive empty-catalog cleanup or accidental new authorization |

The session-start ordering deserves its own check: the Zulip discussion reports agents loading plugins before Symposium's `SessionStart` sync. Verify actual first-prompt visibility after a fresh clone and after an update. A reload notice is an acceptable initial limitation for optional activation; an unloaded required plugin cannot be reported as compliant.
