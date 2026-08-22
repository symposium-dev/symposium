# Managed Symposium uninstall

## TL;DR

- Add `cargo agents uninstall` to remove Symposium-managed integrations before Cargo removes the package.
- Use receipts for bounded discovery and current artifact identity for deletion authority.
- Make new hooks harmless when `cargo-agents` is absent and retire integrations one at a time.
- Preserve user-authored, tracked, shared, modified, and ambiguous state by default.
- Report what is clean in known scopes without claiming knowledge of unknown pre-receipt projects.

## Motivation

Today, `cargo uninstall symposium` removes the binaries Cargo installed. It does not remove hooks, MCP entries, generated skills, caches, or workspace state written by Symposium.

An unguarded global hook can therefore survive the binary and fail in every project:

```text
/usr/bin/bash: line 1: cargo-agents: command not found
PostToolUse:Bash hook error
```

Current init code can unregister known hook forms when an agent is removed from Symposium's configuration. It cannot inventory every managed side effect, prove who owns the current entry, or rediscover every project after the package is gone.

Three cases define the required behavior:

| Case | Required property |
| --- | --- |
| A global hook survives the binary | An absent binary is a quiet success |
| The user or another program changes an entry | Preserve the current entry |
| A project moves, disappears, or predates receipts | Use bounded discovery and qualify the result |

The design cannot prove the absence of an unknown pre-receipt project without scanning arbitrary disks. It reports that limit instead.

## Change in a nutshell

The normal workflow is:

1. Quit agents that may have loaded Symposium-managed configuration.
2. Run `cargo agents uninstall`.
3. Resolve reported blockers and rerun until known scopes are clean.
4. Run `cargo uninstall symposium`.
5. Restart the agents.

```console
$ cargo agents uninstall
Removed
  claude global hook
  codex project skills in /work/reporter

No remaining Symposium integrations in known scopes.
Next: cargo uninstall symposium
```

`--dry-run` performs the same discovery and ownership checks without changing state:

```console
$ cargo agents uninstall --dry-run
Would remove
  claude global hook

Blocked
  .claude/settings.json
    hook differs from a released Symposium registration

No files changed.
```

A blocker is preserved. The user may restore Symposium with `cargo agents sync`, remove the entry manually, authorize a verified tracked-file edit with `--include-tracked`, or acknowledge responsibility for a preserved artifact. Interrupted cleanup is safe to rerun.

## Detailed plans

### Decision sought

Accepting this RFD means agreeing that:

1. All new Symposium-owned integration writes record durable discovery evidence.
2. Discovery evidence and current identity evidence must both support deletion.
3. New hooks use absent-binary guards and scope-aware activation.
4. Cleanup is bounded, ownership-preserving, and resumable.
5. Cargo, not Symposium, removes the installed package.

Exact lifecycle and evidence rules, shell fixtures, adapter behavior, and cleanup algorithms live in the supporting contracts. Internal Rust type and module organization remain implementation choices.

### Design model

Four concepts answer different questions:

| Term | Question answered |
| --- | --- |
| Managed ID | Which records belong to one logical registration? |
| Receipt | Where should Symposium look? |
| Signature, fingerprint, marker, or manifest | Is the current artifact still Symposium-managed? |
| Project permit or global tombstone | May this hook execute here now? |

The [ownership contract](./ownership/README.md) is authoritative for receipts and identity. The [hook-activation contract](./hook-activation/README.md) defines execution. The [cleanup-engine contract](./cleanup-engine/README.md) defines planning, mutation, recovery, output, and exit status.

### Ownership and managed writes

The managed-mutation layer records a pending receipt before an external write and records it as applied only after verification. A new hook, skill, MCP server, or plugin package using an existing artifact type inherits this behavior. A genuinely new side-effect type needs an ownership adapter.

Static integrations use released structural signatures. Dynamic MCP entries and plugin paths use secret-free fingerprints captured at write time. Generated directories require markers and manifests that account for every entry. Goose uses a verified marker-delimited byte block to preserve surrounding YAML formatting. The [ownership contract](./ownership/README.md#identity-by-artifact-type) specifies each form.

Receipts discover candidates; they do not authorize deletion. `config.toml`, custom and external plugin sources, shared tools, user content, and any mismatched artifact remain user-owned. A `.symposium` marker alone never permits recursive deletion.

### Hook safety and activation

New hook commands use versioned POSIX and PowerShell guards. An absent binary exits successfully without output. The exact command and serialization fixtures remain in the [hook contract](./hook-activation/README.md#generated-outer-guards).

A project hook runs only when a local permit matches its managed ID and exact registration-owning root. A copied ID grants no authority, and a nested checkout cannot inherit a parent's permit. A clone, moved checkout, container, WSL environment, or Windows host must run `cargo agents sync` for its own normalized root.

Global hooks use the opposite failure direction. A registration proven global runs unless uninstall has written its retirement tombstone; losing the managed-state store does not disable every global hook. Corrupt or ambiguous state still denies plugin dispatch and surfaces repair guidance.

This activation model protects project-scoped Symposium dispatch. It does not make global hooks a hostile-workspace boundary, and native agent-plugin directories that bypass hook preflight need their own scope mechanism. Legacy hooks remain unguarded until init or sync migrates an exact released signature.

### Cleanup and recovery

Uninstall examines known adapter targets, the current workspace when available, recorded targets and project roots, bounded historical state, and fixed Symposium-private directories. It never crawls the user's home or disks.

The engine plans the complete run before mutation. It then retires, revalidates, removes, and verifies one external artifact at a time. If a blocker or interruption occurs, receipts and repair state remain. Private caches, telemetry, logs, and lifecycle records are removed only after external integrations are absent or validly acknowledged.

Tracked project configuration is preserved unless `--include-tracked` authorizes removal of the verified Symposium structure. `--acknowledge` transfers responsibility without weakening identity checks. It cannot turn a live unguarded `cargo-agents` reference into a clean result.

The final assessment is:

| Assessment | Meaning |
| --- | --- |
| `ready` | Recorded and inspectable scopes are clean, with no historical-discovery limitation |
| `ready-for-known-scopes` | Known scopes are clean, but a pre-receipt project may be unrecorded |
| `blocked` | A live reference, ownership conflict, or operational verification failure remains |

The durable coverage origin, `managed-only`, `pre-receipt`, or `unknown`, bounds that assessment and is never promoted automatically. The existing `cargo agents status` command currently reports plugin enablement; this proposal extends it with read-only managed-health states. Sync, not status, performs repair.

### Scope and compatibility

This RFD covers hook and MCP registrations, generated skills and plugin packages, generated files and mirrors, caches, workspace records, logs, telemetry, and managed lifecycle state.

It does not reverse arbitrary installation scripts, uninstall shared packages, delete external plugin sources, restart agents, remove tracked project entries without explicit authorization, scan for unknown projects, or remove the package binary.

The static signature catalog retains current and historical released forms. Dynamic entries without receipts are preserved. Historical workspace state recovers some old roots, but users upgrading from pre-receipt versions should sync any known dormant checkout before uninstalling.

### Drawbacks and limitations

The strongest objection is proportionality: receipts, signatures, activation state, locks, and recovery machinery are substantial additions for uninstall cleanup. A smaller change could guard missing binaries and document manual removal.

That smaller design would stop the visible error but would not safely discover project integrations, identify dynamic entries, coordinate interrupted cleanup, or distinguish user replacements from Symposium output. This RFD accepts the larger mechanism because those are core cleanup requirements, not optional polish.

Other costs remain:

- Receipts consume small linear storage and retain local paths until finalization.
- Every released static form becomes a compatibility fixture; every new artifact type needs an adapter.
- Project activation adds bounded hook latency and cannot ship until its benchmark gate passes.
- Some pre-receipt installations can reach only `ready-for-known-scopes`.
- Users must quit agents before cleanup and sync projects after moves or clones.

### Rationale and alternatives

| Alternative | Why not chosen |
| --- | --- |
| Keep current behavior and document manual cleanup | Leaves stale hooks noisy and gives users no bounded inventory or ownership check |
| Add only an absent-binary hook guard | Stops the error but leaves managed configuration and private state behind |
| Delete by name, command, or marker | Resemblance does not prove current ownership, especially for dynamic or replaced entries |
| Scan the home directory or disks | Unbounded, privacy-invasive, and still incomplete across containers or removed media |
| Require positive permits for global hooks | Losing the state store would silently disable the recommended global installation |
| Retire every integration before mutation | A blocker would leave an otherwise usable installation completely inactive |
| Add `--force` | Bypassing identity checks could delete third-party state; acknowledgement gives a terminating path without claiming ownership |
| Add `cargo agents implode` to remove everything | Self-removal is not portable and duplicates Cargo's ownership of installed binaries |

### Prior art

1. **[`cargo uninstall`](https://doc.rust-lang.org/cargo/commands/cargo-uninstall.html).** Cargo removes packages recorded under its installation root. Symposium therefore cleans only the external state it understands before Cargo removes the package.

2. **[`rustup self uninstall`](https://rust-lang.github.io/rustup/installation/).** Cleanup happens while the owning executable remains available. Symposium also requires current identity proof because it edits shared agent configuration.

3. **[Homebrew Cask `uninstall` and `zap`](https://docs.brew.sh/Cask-Cookbook#stanza-zap).** Deeper cleanup is explicit and structured, while user-created files stay out of scope.

4. **[Kubernetes finalizers](https://kubernetes.io/docs/concepts/overview/working-with-objects/finalizers/).** Durable retiring state preserves recovery evidence until owned cleanup finishes.

These examples inform the workflow, but none establishes ownership of Symposium artifacts. Adapter-specific discovery and identity rules remain necessary.

### Questions and future work

| Class | Items |
| --- | --- |
| Required before acceptance | None |
| Implementation gates | Adapter working-directory conformance and hook latency benchmark |
| Bounded implementation choices | Rust type/module layout and exact JSON nesting within the documented stable fields |
| Future work | Native agent-plugin activation and new artifact adapters |

If an adapter cannot establish a working directory inside the checkout or meet the latency gate, it cannot enable project-scoped guarded hooks. Native agent-plugin support may change which artifacts Symposium writes, but not the receipt-plus-identity ownership rule.

### Proposed documentation

- [`cargo agents uninstall`](./cargo-agents-uninstall/README.md) defines the proposed workflow, flags, output, and recovery guidance.
- [Managed integrations](./managed-integrations/README.md) explains receipts, identity, activation, and lifecycle for users.

These remain proposed pages until implementation lands.

## Frequently asked questions

### Why does Cargo not perform this cleanup?

Cargo tracks installed package binaries. Symposium writes agent configuration and project artifacts that Cargo neither owns nor understands. Symposium cleans its domain first; Cargo then removes the package.

### Why is a receipt not enough?

A receipt proves that Symposium intended to write at a location. The user or another program may later replace that entry. Current identity evidence must still match before cleanup mutates it.

### Why must agents be quit first?

An agent may keep settings in memory and rewrite an old file on exit. Quitting first removes that race. Restarting after package removal reloads the cleaned configuration.

## Implementation plan and status

Implementation has not begun. The steps are dependency-ordered.

### Step 1: Establish ownership primitives

Add path identity, managed IDs, versioned receipts, lifecycle recovery, coverage origin, static signatures, dynamic fingerprints, and artifact validation. This step has no dependency and changes no production write or runtime behavior.

Verify schema evolution, crash recovery, path safety, secret exclusion, signatures, fingerprints, collisions, and generated-tree manifests.

- [ ] PR: ownership and managed-state primitives

### Step 2: Route managed writers

Depends on Step 1. Route hook, MCP, skill, generated-file, plugin-package, cache, and workspace-state writes through managed mutation. Writes gain receipts, but their external behavior remains unchanged; exact legacy migration does not change legacy hook execution.

Verify lifecycle recovery, concurrent edits, collision handling, Goose and surrounding-format preservation, and unchanged adapter behavior apart from managed identity.

- [ ] PR: managed writers and legacy migration

### Step 3: Add guarded hook activation

Depends on Step 2. Add exact shell fixtures, project permits, global tombstones, root resolution, degraded classification, SessionStart guidance, and managed health in status. New and migrated registrations change behavior here; unmigrated legacy registrations do not.

Verify every adapter's working-directory contract, scope isolation, missing and corrupt state, stripped `PATH`, shell escaping, one-time guidance, repair, status, and the platform latency budget.

- [ ] PR: guarded hooks, activation state, and status health

### Step 4: Add uninstall planning and cleanup

Depends on Steps 1-3. Add minimal-startup dispatch, bounded discovery, dry-run, tracked-file policy, acknowledgements, ordered mutation, recovery, locks, telemetry finalization, reporting, and exit codes. This step introduces `cargo agents uninstall`.

Verify preview/apply parity, tracked and read-only files, acknowledgement invalidation, every interruption boundary, idempotent reruns, contention, retry limits, finalization failures, all assessments, and the original stale-hook regression.

- [ ] PR: uninstall command and cleanup engine

### Step 5: Complete compatibility and documentation

Depends on Step 4. Run every supported adapter and both scopes on Linux, macOS, and Windows, then publish the proposed pages and update shipped hook, state, telemetry, module-structure, and important-flow documentation.

Verify exact adapter fixtures, concurrent agents, tracked repositories, stripped `PATH`, the deterministic integration harness, mdBook, formatting, clippy, and workspace tests.

- [ ] PR: adapter rollout, platform completion, and documentation

The plan extends the existing integration harness; it does not require rewriting it.
