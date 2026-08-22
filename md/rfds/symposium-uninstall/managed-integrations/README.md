# Managed integrations

> Proposed user documentation for the [Managed Symposium uninstall RFD](../README.md). It does not describe released behavior yet.

Symposium writes derived state into agent configuration and project directories during init and sync. Managed integrations make those writes discoverable, identifiable, locally activatable, and removable without claiming ownership of surrounding user files.

The [ownership](../ownership/README.md), [hook-activation](../hook-activation/README.md), and [cleanup-engine](../cleanup-engine/README.md) pages are authoritative.

## Mental model

Consider a project hook written to `.claude/settings.json`:

1. A **receipt** records where Symposium wrote it.
2. A **released signature** proves the current entry still has the form Symposium wrote.
3. A **project permit** says this local checkout may execute it.

These answer different questions:

| Question | Evidence |
| --- | --- |
| Where should cleanup look? | Receipt and known adapter locations |
| Is this still Symposium's artifact? | Signature, fingerprint, marker, or manifest |
| May this project hook run here? | Root-bound permit |

The shared managed ID only connects these records. It is public, may be committed, and grants neither deletion nor execution authority.

## Receipts and identity

Every managed mutation gets a stable ID and a small private receipt containing artifact type, adapter, scope, normalized target, structural location, non-secret identity evidence, and lifecycle state: `pending`, `applied`, `retiring`, or `acknowledged`.

Receipts contain no project or plugin source, environment values, headers, tokens, or command output. They do not expire with inactivity. Missing or moved paths consume little storage and remain until cleanup can finalize safely.

A receipt discovers a target; it does not authorize deletion. Cleanup also requires artifact-specific evidence:

- released structural signatures for static hooks, files, and built-in MCP entries;
- secret-free fingerprints for dynamic plugin-provided MCP entries;
- markers and manifests for generated output; and
- a unique verified marker-delimited byte range for Goose YAML.

Changed entries, unknown generated contents, and targets replaced by links are preserved. Dynamic fingerprints exclude environment values, headers, and tokens; loss of the receipt therefore preserves a dynamic entry.

All writers use one managed-mutation layer. New instances of existing hook, skill, MCP, or generated-package types gain this evidence automatically. Only a new kind of external side effect needs a new ownership adapter.

An occupied structural slot is a collision unless matching pending or applied evidence exists. Init and sync do not adopt lookalike entries. Exact legacy migration is separate.

## Project hook activation

A project permit binds a managed ID to one normalized registration-owning root. Before plugin startup, Symposium walks upward from the hook's working directory to the nearest adapter configuration containing that exact released registration and requires its root to equal the permit root.

Knowing the ID or finding only an ancestor permit is insufficient. Nested checkouts cannot inherit a parent's permit. Sync refuses to permit a filesystem root or the user's home directory.

If a project is inactive, SessionStart may show `cargo agents sync` once for that root. It does not load workspace plugins, refresh registries, auto-sync, or run project code. Other inactive events exit successfully without output.

## Global hooks and binary guards

Global hooks do not use positive permits. They run unless uninstall writes a retirement tombstone. Deleting or losing the managed-state directory therefore does not silently disable a verifiable global registration.

This is cleanup coordination, not hostile-workspace protection: a global hook still runs in every project.

Generated shell commands first try the registered executable location, then `PATH`. Portable project forms try the Cargo home convention first. If `cargo-agents` is absent, the guard exits 0 without output. Missing internal state then permits only one exact degraded global match; project failure or ambiguous classification denies plugin dispatch.

`cargo agents status` continues to report workspace plugin enablement and gains read-only managed health: inactive, retiring, corrupt, unavailable, degraded-global, and cleanup-in-progress. Repair remains explicit through `cargo agents sync`.

## After cloning or moving

Run from the new checkout:

```console
cargo agents sync
```

Sync verifies the registration, records the normalized local root, and publishes its permit. Multiple clones may share an ID, but each root activates separately.

A moved checkout, container, WSL environment, and Windows host are separate permit environments when their normalized paths differ. Old records stay small and inert until cleanup confirms their paths absent; Symposium never scans for moved projects.

## Tracked project configuration

Some project hook files are intentionally committed. Uninstall checks Git outside the hook path:

- no `.git` ancestor means no Git executable is needed;
- tracked configuration is preserved by default;
- `cargo agents uninstall --include-tracked` permits removal only of verified Symposium structure; and
- unavailable or indeterminate tracking preserves the file as a blocker.

`--acknowledge <BLOCKER-ID>` can transfer a preserved artifact to the user. It cannot make a live unguarded hook or direct MCP reference to `cargo-agents` safe for binary removal.

## Legacy registrations

Legacy hooks keep their current runtime behavior. Exact historical signatures support bounded migration and cleanup without adding disk verification to the hot path.

Historical workspace state finds some old roots, but earlier manual syncs did not always record them. Unknown pre-receipt projects remain undiscoverable without a filesystem scan, so uninstall reports known-scope coverage rather than claiming universal cleanup.

## Removing Symposium

Quit running agents, then run:

```console
cargo agents uninstall
```

Cleanup retires and verifies artifacts one at a time. Blockers keep discovery state available for rerun or repair. After the command reports no live integrations in its stated scope, run `cargo uninstall symposium`, then restart the agents.

See the [`cargo agents uninstall` guide](../cargo-agents-uninstall/README.md) for flags, examples, exit codes, and interrupted-cleanup recovery.
