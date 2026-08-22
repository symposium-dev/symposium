# Ownership and managed state

> Normative contract for discovery evidence, artifact identity, managed-write lifecycle, and safe mutation proposed by this RFD.

## Summary

Suppose Symposium records a Claude hook in `~/.claude/settings.json`, then the user replaces that hook. The receipt still identifies the slot, but no longer proves that Symposium owns its contents. Cleanup must preserve the replacement.

This model therefore keeps two forms of evidence:

| Evidence | Question |
| --- | --- |
| Receipt or bounded legacy record | Where should Symposium look? |
| Signature, fingerprint, marker, or manifest | Is the current artifact still the one Symposium manages? |

Both must agree before external state is removed. Receipts are versioned, private, non-executable, and secret-free. Changed or ambiguous artifacts are preserved.

## Ownership model

- A **managed ID** is a stable UUID for one logical registration. All event entries in one agent-and-scope hook registration share it.
- An **ownership receipt** records one intended mutation: target, adapter, scope, artifact type, identity evidence, and lifecycle.
- A **static signature** is a versioned structural description of a form emitted by a released Symposium version.
- A **dynamic fingerprint** records the non-secret identity of an instance derived from plugin configuration.

A managed ID only correlates records. It proves neither ownership, execution permission, nor scope.

## Path identity

Receipt targets, project roots, and runtime comparisons use one normalization function:

1. make the path absolute;
2. canonicalize the existing portion;
3. strip the Windows verbatim-path prefix;
4. apply Windows filesystem case rules; and
5. compare components, not string prefixes.

Resolved paths, not inode or file IDs, identify targets. Re-cloning a dotfile repository therefore does not create an unrecoverable inode conflict. Every mutation revalidates the path.

## Managed writes

All externally visible agent-state writes go through one managed-mutation layer. Callers declare an artifact type and desired value; the layer provides receipts, lifecycle, identity evidence, collision checks, target validation, atomic replacement, and cleanup behavior.

```text
record pending intent
        -> write external artifact
        -> verify current structure
        -> record applied state
```

The receipt becomes durable before the external mutation. Recovery inspects the target rather than assuming an interrupted write succeeded.

New hooks, skills, MCP servers, or generated plugin packages using an existing artifact type inherit cleanup automatically. A new external side-effect type needs an ownership adapter because its identity and deletion rules differ.

## Receipt lifecycle

| State | Meaning |
| --- | --- |
| `pending` | Intent is durable; the external write is unconfirmed |
| `applied` | The artifact matches its recorded identity |
| `retiring` | Removal started or was interrupted |
| `acknowledged` | The user accepted responsibility for a preserved artifact |

Project permits are published only after the write is `applied`. Completed receipts remain until the whole uninstall finalizes, preserving discovery evidence after a crash or blocker.

`cargo agents sync` can restore a matching applied registration left `retiring`. Acknowledgement transfers ownership without deletion; changing the artifact invalidates it.

## Identity by artifact type

| Artifact | Required identity evidence and mutation |
| --- | --- |
| Static hook or built-in MCP registration | Receipt plus exact released signature; remove only the owned structural entry or dedicated file |
| Dynamic MCP or registered plugin path | Receipt plus exact non-secret fingerprint; remove only the recorded structural entry |
| Goose MCP block | Receipt, unique marker pair, indentation, and fingerprint; remove the verified byte extent |
| Generated file | Receipt plus marker or released content signature |
| Generated skill, plugin package, or mirror | Receipt, marker, and manifest; remove the directory only when every entry is accounted for |
| Symposium-private state | Containment below the fixed private root and successful external finalization |

`config.toml`, custom plugin sources, and externally authored packages remain user-owned. Reading `plugin.json` never authorizes source deletion. Compiled packages, copies, path registrations, and enablement entries written by Symposium are managed.

A `.symposium` marker is not enough for recursive deletion. The receipt finds the target; the marker and manifest must account for its contents.

## Static signatures

Symposium keeps a versioned catalog of every released static form, current and historical:

- generated hook commands and their containing structure;
- dedicated generated agent files;
- static built-in MCP registrations; and
- generated markers and manifests.

Signatures compare parsed structure, normalized executable identity, fixed arguments, and managed-ID placement. They never match only a broad key, event name, or the presence of `cargo-agents`.

Changing a released form adds a fixture; it does not replace old evidence. This compatibility cost permits cleanup after receipts are lost or an older release is removed.

## Dynamic fingerprints

Plugin-provided MCP values are not a finite catalog, so Symposium records their identity when written.

| Adapter | Structural container |
| --- | --- |
| Claude, Gemini, Kiro | `mcpServers.<name>` |
| GitHub Copilot | Top-level MCP server map entry |
| Codex | `mcp_servers.<name>` |
| Goose | `extensions.<name>` |
| OpenCode | `mcp.<name>` |

The fingerprint includes adapter, normalized target, structural container, entry name, transport, and command plus arguments or URL. It excludes environment values, headers, tokens, and other secrets.

Every recorded field must match. A changed field creates a conflict. A dynamic entry without its receipt is preserved because resemblance is not proof.

Goose is an editing exception: its YAML is not round-tripped through a serializer. Symposium writes behavior-neutral managed-ID comment markers, verifies one unique marker pair and the enclosed mapping, then removes that exact byte extent. Missing, duplicate, malformed, or mismatched markers preserve the block.

## Collisions and concurrent changes

Init and sync do not adopt or overwrite an occupied slot without matching `pending` or `applied` evidence. Exact released-signature migration is a separate path.

Collision detection inspects the occupied structure, not a retained acknowledgement. Reinstallation therefore detects a preserved collision even after finalization removes old records.

If the target changes between read and replacement, Symposium replans it instead of overwriting the edit.

## Filesystem safety

Receipts are untrusted input. Before mutation, Symposium validates:

- schema version, artifact kind, UUIDs, and enums;
- containment below an allowlisted adapter or private root;
- expected file or directory shape;
- component-wise ancestor relationships;
- the artifact's link policy; and
- current identity evidence.

Cleanup never follows a symlink or junction while deleting a generated tree. A target replaced by a link is preserved. A directory is removed only when its manifest accounts for every remaining entry.

## Missing receipts and historical state

At known locations, exact released signatures can identify static current and legacy forms without a receipt. The next `init` or `sync` may migrate them; uninstall may remove them after the same identity check.

Dynamic entries without receipts stay preserved. Unknown pre-receipt project roots cannot be rediscovered without a filesystem scan.

The first receipt-aware release records a durable coverage origin:

- `managed-only`: no earlier or unexplained integration exists;
- `pre-receipt`: an older release or exact legacy artifact exists; or
- `unknown`: provenance for existing state is missing or corrupt.

The origin is never promoted automatically. Cleanup uses it to qualify the final assessment.

## Storage and cost

Receipts live below a versioned directory in the resolved Symposium configuration home. They contain paths and identity metadata, never executable instructions, tokens, environment values, or headers. Writes are atomic and permissions private.

Storage grows linearly with integrations and checkouts. Successful finalization removes completed records. Recorded paths are visible only to principals already able to read Symposium's private configuration and are never uploaded as telemetry.

## Verification

Tests cover:

- schema evolution, truncation, corruption, and every lifecycle crash point;
- path normalization, containment, symlinks, junctions, and manifests;
- current and historical static signatures;
- dynamic fingerprints with secret fields excluded;
- collisions, concurrent changes, and acknowledgement invalidation;
- Goose editing with surrounding formatting preserved;
- generated skill and plugin-package directories; and
- missing receipts and historical coverage.
