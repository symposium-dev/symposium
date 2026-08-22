# `cargo agents uninstall`

> Proposed user documentation for the [Managed Symposium uninstall RFD](../README.md). It does not describe released behavior yet.

This command removes agent integrations and private local data managed by Symposium. The [cleanup-engine contract](../cleanup-engine/README.md) defines its internal behavior.

## Recommended workflow

1. Quit coding agents that may have loaded Symposium configuration.
2. Run:

   ```console
   cargo agents uninstall
   ```

3. Resolve reported blockers and rerun as directed.
4. Run:

   ```console
   cargo uninstall symposium
   ```

5. Start the agents again.

Quitting first prevents an agent from restoring cached settings after cleanup. Restarting last reloads the cleaned configuration.

## Usage

```console
cargo agents uninstall [--dry-run] [--include-tracked]
                       [--acknowledge <BLOCKER-ID>]...
                       [--quiet] [--json]
```

The command examines known global and recorded project scopes. It works inside or outside a Cargo workspace and never scans the entire filesystem.

`--quiet` suppresses progress, not errors or the final assessment. The existing global `--json` selects machine-readable output.

## Preview cleanup

`--dry-run` performs the same bounded discovery, path validation, Git classification, and ownership checks as cleanup without writing or deleting. It reports `Would remove`, `Preserved`, and `Blocked` items.

```console
$ cargo agents uninstall --dry-run
Would remove
  Global
    Claude hooks
      ~/.claude/settings.json: Symposium hook entries

Preserved
  ~/.symposium/config.toml: user configuration

Preview complete. Apply with `cargo agents uninstall`.
```

A preview with blockers exits 3; an unreliable preview exits 1. Apply rechecks every target because configuration may change after preview.

## What cleanup removes

When ownership is verified, cleanup removes:

- hook registrations and managed MCP entries;
- generated or mirrored skills, files, and directories;
- private plugin and installation caches;
- telemetry, logs, and runtime state; and
- cleanup records after external cleanup succeeds.

From a shared configuration file, it removes only the managed entry. Goose YAML uses verified block removal so surrounding comments and formatting remain unchanged.

Cleanup preserves:

- `config.toml`, custom plugin sources, and user-authored skills;
- entries changed or replaced by another program;
- tools installed through Cargo or another package manager;
- arbitrary legacy script effects that cannot be identified safely;
- tracked project configuration unless `--include-tracked` is used; and
- ambiguous artifacts.

If blockers remain, private discovery state stays available for repair or rerun.

## Tracked files

A hook in Git-tracked project configuration is preserved by default as “committed by your project.” `--include-tracked` authorizes removal of only the verified Symposium entry, not the surrounding file or unrelated entries.

If no ancestor contains a `.git` file or directory, uninstall treats the project as outside Git and does not require a Git executable.

## Acknowledging a blocker

`--acknowledge <BLOCKER-ID>` preserves an artifact and transfers responsibility to you. The report gives its location and a redacted manual edit. This is not `--force` and never weakens ownership checks.

The ID stays stable for the same artifact kind, adapter, normalized target, and structural location. Changing the artifact invalidates the acknowledgement.

An unguarded hook or MCP server that still launches `cargo-agents` cannot be acknowledged into a ready result. Remove that live reference before uninstalling the package.

## Results

A successful run reports actual removals:

```text
Removed
  Global
    Claude hooks
      ~/.claude/settings.json: Symposium hook entries
  Workspaces
    /work/example
      Codex skills
        .agents/skills/example-skill

Preserved
  ~/.symposium/config.toml: user configuration
  cargo-binstall: shared Cargo tool

No remaining live Symposium integrations in recorded scopes.
Next: run `cargo uninstall symposium`, then start your coding agents.
```

For an upgraded installation that may contain unrecorded projects, the result instead says:

```text
No remaining Symposium integrations in known scopes.
Older unrecorded project integrations may still exist; see the preserved items above.
```

Incomplete cleanup lists every live reference or operational failure under `Blocked`; `Next steps` gives the exact flag, sync, acknowledgement, rerun, or manual edit. Completed removals remain complete.

## JSON and exit status

The existing global `--json` emits one versioned document with `mode`, `binary_removal_assessment`, actions, preserved and acknowledged items, blockers, and next steps. The assessment is `ready`, `ready-for-known-scopes`, or `blocked`. Items carry stable reason codes and a live-reference flag. Useful paths are included; secret values and plugin source contents are omitted.

| Status | Meaning |
| --- | --- |
| 0 | Preview or cleanup completed with no live blockers in its assessment boundary |
| 1 | An operational failure prevented reliable planning or verification |
| 2 | Command-line usage error |
| 3 | Preview or cleanup completed, but live blockers remain |

Missing artifacts are already absent. Invalid receipts, unsafe paths, identity conflicts, indeterminate tracking, persistent locks, unsupported schemas, and failed verification do not become silent success.

## Cloned, moved, or containerized projects

Project registrations activate only for a locally synchronized registration-owning root. After cloning, copying, moving, or opening a project in a new container, run:

```console
cargo agents sync
```

An inactive SessionStart may show that command once without loading plugins or executing project code. Other inactive events exit successfully without output. Global hooks remain global; this activation model is not an untrusted-workspace boundary for them.

Older project registrations are a transition case. Historical state finds some roots, not every manual pre-receipt sync. Before removing an old installation, sync any known dormant project checkouts.

## Interrupted cleanup

Rerun `cargo agents uninstall` to resume. If Symposium should remain installed, run `cargo agents sync` to restore matching registrations left retiring. `cargo agents status` reports inactive, retiring (with repair guidance), corrupt, unavailable, degraded-global, or cleanup-in-progress state and names the next command.
