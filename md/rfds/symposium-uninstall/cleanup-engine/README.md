# Cleanup engine

> Normative contract for uninstall discovery, planning, mutation, recovery, reporting, and finalization proposed by this RFD.

## Summary

One rule controls the algorithm: never discard evidence needed to finish or repair cleanup.

A normal run discovers known targets, prints its plan, retires and removes each external artifact separately, verifies absence, then deletes private state. If one target is blocked or the process stops halfway, completed work stays complete while receipts and discovery state remain for a rerun.

## Command surface

```text
cargo agents uninstall [--dry-run] [--include-tracked]
                       [--acknowledge <BLOCKER-ID>]...
                       [--quiet] [--json]
```

- `--dry-run` uses the apply planner but creates no receipt, tombstone, or target change.
- `--include-tracked` permits removal of a proven Symposium structure from tracked configuration, never whole-file deletion.
- `--acknowledge` preserves a blocker and transfers responsibility without weakening deletion proof.
- `--quiet` suppresses progress, not errors or the final assessment.
- The existing global `--json` emits one versioned stdout document; diagnostics stay on stderr.

| Exit code | Meaning |
| --- | --- |
| `0` | Preview or cleanup completed reliably with no live blockers |
| `1` | An operational failure prevented reliable planning or verification |
| `2` | Command-line usage error |
| `3` | Preview or cleanup completed reliably, but live blockers remain |

## Bounded discovery

Uninstall never crawls the home directory or disks. It examines only:

1. known global targets for supported adapters;
2. the current workspace when explicitly available;
3. targets and roots in receipts;
4. roots or targets in permits, notices, tombstones, or acknowledgements;
5. legacy workspace-state files that contain a root; and
6. fixed Symposium-private directories.

A deleted, moved, or renamed root costs one failed bounded lookup. A project at a new path becomes a separate scope after `cargo agents sync`.

Historical workspace state is incomplete because earlier versions did not record every manually synchronized root. The final assessment reports this limit instead of claiming knowledge of an unknown pre-receipt checkout.

## Planning

Every candidate receives one disposition:

| Disposition | Meaning |
| --- | --- |
| Removable | Discovery and current identity evidence agree |
| Already absent | The recorded artifact no longer exists |
| Preserved | Policy excludes it from automatic cleanup |
| Acknowledged | The user accepted responsibility for the preserved artifact |
| Conflicting | The artifact no longer matches Symposium's evidence |
| Operationally unverifiable | Ownership or absence cannot be decided reliably |

The engine prints the complete plan before mutation. Ownership follows [Ownership and managed state](../ownership/README.md).

Dry-run stops after planning. It takes shared locks for a consistent snapshot and writes no recovery state. A dry run with blockers exits `3`.

## Tracked project configuration

Before planning a project-file mutation, uninstall checks ancestors for a `.git` file or directory without launching Git. If none exists, Git is not needed.

When a repository exists:

- tracked configuration is preserved as project-committed by default;
- `--include-tracked` authorizes removal only of proven Symposium structure;
- unavailable or indeterminate Git tracking preserves the file as a blocker; and
- reports identify the file and structural locator without secrets.

Acknowledgement may transfer an entry to the user. It cannot make a live unguarded `cargo-agents` hook or MCP invocation safe for package removal. That reference must be removed manually or with `--include-tracked`.

## Applying the plan

Apply runs these phases:

1. Acquire the exclusive installation barrier.
2. Reconcile receipts, activation records, acknowledgements, signatures, and bounded legacy roots.
3. Discover, classify, and print the complete plan.
4. For each removable external artifact:
   1. mark only its receipt `retiring`;
   2. retire its project permit or create its global tombstone;
   3. lock, reread, and revalidate the target;
   4. remove only the verified structure;
   5. verify absence; and
   6. keep the completed receipt until finalization.
5. If blockers remain, keep every receipt, activation record, cache, workspace record, log, and telemetry file needed for repair or rerun.
6. Otherwise finalize telemetry and Symposium-private state.
7. Recompute the assessment, delete completed lifecycle records, release locks, and report.

Retirement happens per artifact immediately before mutation; uninstall never disables all hooks up front. Discovery state remains while any live blocker exists.

Shared configuration uses parse, revalidation, owned-entry editing, sibling temporary write, flush, atomic replacement, reopen, and absence verification. Goose uses its verified marker-delimited block editor. Concurrent change replans the target instead of overwriting it.

## Failure and recovery

Failure before external mutation restores that artifact to `applied` when the registration still matches, republishing its project permit or removing its global tombstone.

A crash after retirement leaves only that artifact inactive and repairable. Successful removals are not rolled back. Rerun uninstall to continue; `cargo agents sync` restores a matching applied registration.

Transient filesystem failures get one initial attempt and at most two bounded retries. Each retry reopens and revalidates. Permission failures, identity conflicts, unsafe links, indeterminate Git state, and concurrent changes become blockers, not retry loops.

The command is idempotent: verified absence is `Already absent`, and reruns do not recreate removed state.

## Concurrency and locks

Managed mutation uses this order:

```text
installation barrier
    -> managed-state lock
    -> global target when needed
    -> workspace targets sorted by normalized path
```

- Uninstall holds the installation barrier exclusively through planning, mutation, and verification.
- Dry-run holds the barrier and discovered targets in shared mode.
- Init, sync, and repair share installation access and exclusively lock changed state and targets.
- Hook auto-sync uses a non-blocking try-lock; contention skips that cache refresh and uses published state.

After dry-run locks its targets, it checks the managed-state generation. One change retries the snapshot; a second returns an operational failure.

Platforms use native advisory locking and multi-process tests. Diagnostics may include operation, process, and start metadata, but age alone never proves a lock stale.

## Blockers and acknowledgements

A blocker ID is stable over:

```text
artifact type + adapter + normalized target + structural locator
```

The acknowledgement stores that locator and the artifact's current identity, not only its display ID. Moving the locator creates a new blocker; changing the artifact invalidates acknowledgement.

Acknowledgement preserves the artifact, records user responsibility, retires Symposium's claim, prints a redacted manual edit, and makes later installation treat the occupied slot as a structural collision.

Successful finalization may delete acknowledgements because collision detection inspects the occupied entry. There is no `--force`: bypassing identity checks could delete user or third-party state.

## Minimal startup and finalization

Uninstall dispatches before ordinary startup. It initializes only arguments, managed-state path resolution, minimal diagnostics, locking, cleanup, and reporting. It does not refresh registries, load plugins, check for updates, auto-sync, or initialize ordinary telemetry recording.

Telemetry finalization uses the subsystem's supported coordination path. Failure retains discovery and recovery state and becomes a blocker. The uninstall result is not recorded as new telemetry.

Only after every external integration is absent or validly acknowledged does cleanup remove private caches, workspace state, telemetry, logs, receipts, permits, tombstones, notices, acknowledgements, and the empty managed-state directory. It verifies finalization before success.

## Reporting

Human output groups `Removed`, `Already absent`, `Preserved`, `Acknowledged`, `Blocked`, and `Next steps`.

Machine-readable items carry stable kind, adapter, scope, target, structural locator where applicable, disposition, reason code, and live-reference flag. Secret fields are redacted. The [command reference](../cargo-agents-uninstall/README.md) shows human and JSON output.

## Removal assessment

| Assessment | Meaning |
| --- | --- |
| `ready` | No live integration remains in recorded and inspectable scopes, with no historical limit |
| `ready-for-known-scopes` | Known scopes are clean, but a pre-receipt project may be unrecorded |
| `blocked` | A live reference, ownership conflict, or operational verification failure remains |

Coverage origin makes this decidable:

- only `managed-only` can produce `ready`;
- `pre-receipt` and `unknown` produce at best `ready-for-known-scopes`; and
- any live blocker produces `blocked`.

Origin is never promoted automatically. Output says known scopes are clean instead of claiming universal safety.

## Verification

The deterministic integration harness covers:

- bounded global, workspace, receipt, and historical discovery;
- dry-run/apply parity, output, and exit codes;
- tracked, untracked, read-only, and indeterminate-Git configuration;
- interruption at every lifecycle boundary and idempotent reruns;
- contention, shared previews, concurrent edits, and auto-sync try-locks;
- stable acknowledgements, changed artifacts, and reinstall collisions;
- two bounded retries and permanent failures;
- telemetry and private-state finalization failures;
- all three assessments; and
- the original stale-global-hook regression after package removal.
