# Hook activation

> Normative contract for generated hook guards, project activation, global retirement, degraded behavior, and hook-path cost proposed by this RFD.

## Summary

Two failures shape this contract:

- after `cargo uninstall symposium`, a stale hook must not fail because `cargo-agents` is absent;
- a committed project hook copied to another checkout must not run merely because its public managed ID is known.

The outer shell guard solves the first. A local permit for the exact registration-owning root solves the second. Global hooks instead run until uninstall writes a retirement tombstone.

This is not general workspace trust. A global hook intentionally runs in every working directory and may discover workspace plugin configuration. Gating workspace plugins for global users requires a separate design.

## Runtime flow

```text
agent invokes hook
        -> outer guard locates cargo-agents
        -> preflight classifies project or global scope
        -> permit or tombstone decision
        -> ordinary startup and plugin dispatch
```

The outer guard handles binary absence. In-process preflight decides scope and retirement before plugin loading, registry refresh, or auto-sync.

## Scope classification

New invocations carry `--managed-id <UUID>`, not an authoritative scope. Preflight uses a directly addressed receipt, project permit, or global tombstone. Invocation text alone is insufficient.

When state for the ID is missing or unavailable, bounded degraded classification checks only:

- the registration-owning-root walk; and
- the adapter's known global target.

It requires an exact released signature containing that ID. One project match stays inactive and may show the sync hint. One global match runs and reports degraded health. Zero or multiple matches deny plugin dispatch and report ambiguity. This path opens only known adapter files and never invokes Cargo.

## Project activation

A project permit binds:

```text
managed ID + normalized registration-owning root
```

Starting at the process working directory, preflight walks a bounded number of ancestors. The owning root is the nearest ancestor whose adapter project configuration contains an exact released registration for this managed ID. A nearer configuration without that registration is ignored.

Execution requires all four conditions:

1. trusted state or exact degraded classification identifies project scope;
2. a registration-owning root exists;
3. the permit carries the same managed ID; and
4. normalized permit and owning roots are equal.

An ancestor permit alone is insufficient. A nested checkout containing the copied registration resolves to its own root and is denied. Sync refuses to permit the filesystem root or the user's home directory.

Adapters must invoke project hooks with a working directory inside the checkout. An adapter unable to meet this contract cannot offer project-scoped guarded registrations. Tests cover launches from the checkout root and nested directories.

A move, clone, dev container, WSL environment, or Windows host is a separate permit environment when its normalized root differs. Each needs `cargo agents sync`.

## Global activation

Project and global registrations fail in opposite directions:

| Scope | Active when |
| --- | --- |
| Project | A positive permit matches the ID and owning root |
| Global | No valid retirement tombstone exists for the ID |

Losing a project permit disables that checkout. Losing the managed-state directory does not disable the recommended global installation. A global registration runs after receipt proof or one exact degraded signature match, recording degraded health where possible.

Uninstall writes a global tombstone immediately before mutating its registration. A valid tombstone exits successfully and quietly. A corrupt tombstone denies dispatch and surfaces repair guidance.

## Preflight outcomes

| Classification and state | Behavior |
| --- | --- |
| Project permit matches ID and owning root | Continue to ordinary startup |
| Project permit missing or non-matching in a readable store | Stay inactive; SessionStart may give one sync hint |
| Project state corrupt or unavailable | Deny plugin dispatch and surface repair guidance |
| Valid global tombstone | Exit successfully and quietly |
| Corrupt global tombstone | Deny plugin dispatch and surface repair guidance |
| Global registration proven, no tombstone | Continue; report degraded health when state was missing |
| Missing state with zero or multiple exact matches | Deny plugin dispatch and report ambiguity |

Correct inactivity is quiet except for bounded SessionStart guidance. Unavailable or indeterminate state is not silently treated as correct inactivity.

## Inactive SessionStart

An inactive project SessionStart may run preflight and emit static `additionalContext` naming `cargo agents sync`. It may not read workspace plugin configuration, refresh a registry, run plugin code, or auto-sync.

A per-root notice suppresses repeat guidance. Records are capped at 64 roots per managed ID. Beyond that cap, new roots receive no stored or repeated hint, and status reports the suppressed count. Other inactive events return success without output.

If a writable managed store contains corrupt state, preflight records a health flag and one best-effort log line. The next explicit sync quarantines corrupt records and recreates derived state. If the store itself is unavailable, status detects it on demand and SessionStart provides the only guaranteed warning; Symposium creates no fallback state directory.

## Status and repair

Today `cargo agents status` reports plugin enablement for the active workspace. This proposal adds a read-only, versioned managed-health snapshot. The same command can then report inactive, retiring, corrupt, unavailable, degraded-global, and cleanup-in-progress states with stable reason codes and commands.

Status never repairs state. It reads under the shared installation barrier. While uninstall holds the barrier exclusively, status reports cleanup in progress instead of inspecting partial mutation. Corrupt or unreadable state produces a diagnostic snapshot, not a panic.

`cargo agents sync` is the repair path. If a retiring receipt still matches an applied registration, sync returns it to `applied`, republishes its project permit, or removes its global tombstone.

## Generated outer guards

The guard's only job is to make an absent binary exit 0 without output. Scope-aware preflight remains inside the binary.

Machine-local global registrations try the absolute path resolved at registration, then `PATH`. Portable committed project forms cannot contain another user's path, so they try the Cargo home convention, then `PATH`.

These are the versioned single-line command values before host serialization.

POSIX machine-local global:

```sh
if [ -x <ABSOLUTE_PATH_POSIX_LITERAL> ]; then exec <ABSOLUTE_PATH_POSIX_LITERAL> hook <AGENT> <EVENT> --managed-id <UUID>; elif command -v cargo-agents >/dev/null 2>&1; then exec cargo-agents hook <AGENT> <EVENT> --managed-id <UUID>; else exit 0; fi
```

POSIX portable project:

```sh
if [ -x ${CARGO_HOME:-$HOME/.cargo}/bin/cargo-agents ]; then exec ${CARGO_HOME:-$HOME/.cargo}/bin/cargo-agents hook <AGENT> <EVENT> --managed-id <UUID>; elif command -v cargo-agents >/dev/null 2>&1; then exec cargo-agents hook <AGENT> <EVENT> --managed-id <UUID>; else exit 0; fi
```

PowerShell machine-local global:

```powershell
$symposiumBin = <ABSOLUTE_PATH_POWERSHELL_LITERAL>; if (-not (Test-Path -LiteralPath $symposiumBin -PathType Leaf)) { $symposiumCommand = Get-Command cargo-agents -CommandType Application -ErrorAction SilentlyContinue; if ($null -eq $symposiumCommand) { exit 0 }; $symposiumBin = $symposiumCommand.Source }; $global:LASTEXITCODE = $null; & $symposiumBin hook <AGENT> <EVENT> --managed-id <UUID>; if ($null -eq $LASTEXITCODE) { exit 1 }; exit $LASTEXITCODE
```

PowerShell portable project:

```powershell
$cargoHome = $env:CARGO_HOME; if ([string]::IsNullOrWhiteSpace($cargoHome)) { $cargoHome = Join-Path $HOME '.cargo' }; $symposiumBin = Join-Path $cargoHome 'bin/cargo-agents.exe'; if (-not (Test-Path -LiteralPath $symposiumBin -PathType Leaf)) { $symposiumCommand = Get-Command cargo-agents -CommandType Application -ErrorAction SilentlyContinue; if ($null -eq $symposiumCommand) { exit 0 }; $symposiumBin = $symposiumCommand.Source }; $global:LASTEXITCODE = $null; & $symposiumBin hook <AGENT> <EVENT> --managed-id <UUID>; if ($null -eq $LASTEXITCODE) { exit 1 }; exit $LASTEXITCODE
```

The POSIX encoder single-quotes paths and escapes apostrophes as `'\''`. The PowerShell encoder single-quotes paths and doubles apostrophes. Placeholders above are encoded literals, never raw paths.

Copilot publishes both shell forms. Versioned adapter fixtures assert decoded commands and exact JSON, TOML, or YAML serialization. They cover absent binaries, stripped `PATH`, spaces and metacharacters, launch failures, status propagation, payload preservation, and unchanged working directories.

## Legacy registrations

An invocation without a managed ID keeps legacy behavior. On-disk hot-path verification would create a new failure mode. Exact historical signatures are used during bounded migration and cleanup; the next `init` or `sync` may rewrite them into guarded form.

## Agent plugin boundary

Project permits govern Symposium-dispatched hooks. A native agent-plugin directory bypasses hook preflight. Its scope must come from project placement, a workspace-scoped agent registration, or a project-safe fallback.

## Performance

Preflight performs no directory-wide scan, network access, registry refresh, plugin loading, Cargo metadata query, or subprocess. It reads directly addressed state, normalizes the working directory, and performs bounded adapter-configuration signature checks.

Guarded hooks become the default only while p95 added preflight latency is at most:

```text
max(2 ms, 5% of baseline hook-dispatch latency)
```

The baseline uses the same outer guard without managed-state preflight. CI records p50 and p95 for active, inactive, missing-store, and nested-checkout cases on Linux, macOS, and Windows. Receipt-store size must not change hot-path read or path-probe counts.

## Verification

Tests cover:

- positive project permits and global retirement tombstones;
- clones, moves, monorepos, unrelated nested configuration, and nested checkouts;
- missing, corrupt, unavailable, and ambiguous state;
- one-time and dismissed SessionStart guidance without plugin execution;
- status snapshots before, during, and after cleanup;
- exact POSIX and PowerShell fixtures for every supported adapter;
- adapter working-directory contracts from root and nested directories;
- legacy runtime behavior and migration; and
- the numeric latency budget.
