# Audits file

The community **audits list** holds statements about the plugin content that a package ships. An audit says "this exact payload was reviewed"; a revocation says "this release, or this payload, must not load".

For any dependency plugin, the audits give exactly one answer:

| Answer | When |
|---|---|
| revoked | a `[[revoked]]` entry matches the version or the payload digest |
| audited | otherwise, the payload digest matches an `[[audit]]` entry |
| not audited | anything else, including a payload that cannot be digested |

A revoked plugin never loads. An audited plugin is otherwise treated like any other, so the consent prompt still asks about it.

## Where audits come from

The community list is the builtin `symposium-audits` registry, the [symposium-dev/audits](https://github.com/symposium-dev/audits) repository. It is an ordinary git registry with no plugins in it, only an `audits/` directory, and it is off by default until the community list exists; [`[defaults] symposium-audits = true`](./configuration.md#defaults) turns it on. It is fetched and refreshed exactly like the recommendations registry: Symposium checks it for changes when an agent session starts, and on every other invocation, agent hook events included, at most once a minute (a fresh cache skips the check). `cargo agents plugin sync` refreshes it on demand. When it cannot be fetched (offline, or the repository is unreachable), it behaves exactly like an unreachable recommendations registry: the same warning, and it contributes nothing, so every lookup answers "not audited".

It is the only place audits are read from. Reading them is cache-only: it never touches the network.

## Location

One file per package, at `audits/<package manager>/<name>.toml` in the repository's root. For a crate the package manager is `cargo`, and the name is the crate's name as published on crates.io, lowercase, with hyphens kept:

```text
audits/
  cargo/
    widget-lib.toml     # the crate `widget-lib`
    serde_json.toml     # the crate `serde_json`
```

The file names the package by its path, so it has no field for it, and `widget-lib.toml` and `widget_lib.toml` are different files. Looking up a package reads only its one file. Today only crates are looked up.

## Format

```toml
# audits/cargo/widget-lib.toml
format = 1

[[audit]]
digest   = "sha256:<64 lowercase hex characters>"
versions = ["1.4.2", "1.4.3"]

[[revoked]]
versions = ">=1.3.0, <1.4.2"
reason   = "remote-exec"

[[revoked]]
digest = "sha256:<64 lowercase hex characters>"
reason = "hidden-behavior"
```

| Key | Description |
|-----|-------------|
| `format` | The schema version, an integer. Required. This page describes `1`. |
| `[[audit]]` | One entry per distinct *payload* that was audited. |
| `[[audit]].digest` | The [payload digest](#payload-digest) of the audited payload. Required. This is what a package is matched on. |
| `[[audit]].versions` | The releases known to carry that payload, as exact versions. Bookkeeping for people and tooling: Symposium never matches on it. |
| `[[revoked]]` | A release or payload that must not load. Set **exactly one** of `versions` and `digest`. |
| `[[revoked]].versions` | A [semver requirement](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html) such as `>=1.3.0, <1.4.2`, matched against the resolved crate version. A version that is not valid semver never matches. A pre-release is also matched as its release, so `*` revokes `2.0.0-alpha.1` and `>=1.0.0` revokes `1.5.0-rc.1`: semver's own rule, that a pre-release meets only a requirement naming a pre-release of the same release, would let every pre-release escape a revocation. |
| `[[revoked]].digest` | A payload digest. Revokes every release carrying that payload. |
| `[[revoked]].reason` | Why, as one of the codes below. Required: an entry without one is dropped. A code this version of Symposium does not know still revokes, with a warning in the log, so a list that adds a code never lets a revoked crate load on older clients. |

A digest that is both audited and revoked is revoked.

### Reasons

`reason` is a code, never prose, so Symposium renders the sentence itself.

| Code | Rendered as |
|------|-------------|
| `remote-exec` | tells the agent to fetch and run remote code |
| `exfiltration` | tries to send secrets or source to a third party |
| `hidden-behavior` | hides what it does from the user |
| `off-task` | steers the agent away from the user's task |
| `malicious-crate` | comes from a crate marked as malicious |
| any other code | has a reason code this version of symposium does not know |

## Payload digest

An audit covers the *plugin payload* of a crate, not its code: the manifest that says what the crate contributes, and the skill content that manifest names. A release that changes only `src/` has the same payload and the same digest, so one audit covers it, while any change to skill text makes a new digest that nobody has audited yet. [`cargo agents plugin digest`](./cargo-agents-plugin.md#cargo-agents-plugin-digest) prints the digest of a crate or a directory.

The payload is these entries:

1. `Symposium.toml` at the crate root, byte for byte, when there is one.
2. `[package.metadata.symposium]` from `Cargo.toml`, when there is one, as canonical compact JSON (see below), under the entry name `Cargo.toml#package.metadata.symposium`. That name is a label, not a file path: no file is read from it, and a file with that exact name inside a skill directory makes the payload ineligible so it cannot collide. Nothing else in `Cargo.toml` is part of the payload.
3. Every file, of any type, in each *skill directory* the manifest names inside the crate. The manifest names them with its `[[skills]]` groups that set `source.path`, plus the default `skills/` group unless `[defaults] skills = false`. A skill directory is one that contains a `SKILL.md`, and everything beneath it counts, so a `SKILL.md` that points at a file in `resources/` is covered together with that file. A file reachable through two groups counts once. Group `predicates` are not consulted, so the digest does not depend on the workspace it is computed in.

Entries are named by their path relative to the crate root, with `/` separators, and sorted bytewise (by the UTF-8 bytes of the name). The digest is SHA-256 over the prefix `symposium-payload-v1\0` followed, for each entry, by the name length as 8 bytes big-endian, the name, the content length as 8 bytes big-endian, and the content. Bytes are hashed as published: line endings are not normalized. Symlinks are followed, and the entry is named by the link's path.

### Canonical JSON for the metadata entry

The table is written with no whitespace, and:

- Object keys are sorted bytewise by their UTF-8 bytes, at every nesting level. Arrays keep their order. An empty table is `{}` and an empty array `[]`.
- Strings are JSON strings as `serde_json` writes them (RFC 8259): `"` and `\` are escaped, the control characters U+0008, U+000C, U+000A, U+000D and U+0009 are written `\b`, `\f`, `\n`, `\r` and `\t`, every other character below U+0020 is `\u00XX` with lowercase hex digits, and everything else, including `/` and non-ASCII characters, is written as is in UTF-8.
- Integers are decimal. Booleans are `true` and `false`.
- Floats use the shortest representation that round-trips, as `serde_json` (Rust's `ryu`) writes it: `0.1`, `100.0`, `1.5`, and exponent form for very large and very small magnitudes (`1e+21`, `2.5e-7`). Python's `repr` differs for some values (Python writes `2.5e-07`), so do not use it for floats there.
- `nan`, `inf` and `-inf` have no JSON number, so they are written as the JSON strings `"NaN"`, `"inf"` and `"-inf"`.
- A datetime is the JSON string of its TOML text form, for example `"2024-01-02T03:04:05Z"`.

The digest tests carry two golden vectors, one with a manifest, skills and CRLF bytes, and one with a metadata table exercising nested tables, every scalar type, and a string that needs escaping. The second is a vector for the canonical JSON and the framing only: its table is not a valid plugin manifest, so that crate itself has no digest.

### When there is no digest

Some payloads cannot be pinned, so they have no digest and are never audited:

- a `[[skills]]` group with `source.git` (what it fetches can change under the same crate release);
- a skill path that leaves the crate directory;
- a broken symlink, a directory symlink that loops back, or a directory that cannot be read, anywhere under a skill group directory: the whole group directory is walked to find its skill directories, so this applies outside skill directories too (a dangling link in `skills/scratch/` with no `SKILL.md` counts), since any of these could hide a skill;
- a skill group directory that is a dangling link, or any file in the payload that cannot be read (a permission or I/O error is never read as "not there");
- an entry in a skill directory that is neither a file nor a directory, or a path that is not UTF-8;
- a manifest that does not parse as a plugin manifest: a TOML or schema error (an unknown key included) in `Symposium.toml` or in `[package.metadata.symposium]`. Loading drops such a layer and uses the rest, but the digest does not pin what the crate ships as if it were valid;
- more than 4 MiB of payload content.

`cargo agents plugin digest` prints which of these applies.

## Revocations

A revocation stops a crate's plugin from loading, on every route a crate's plugin can take: a dependency's embedded plugin, a crate reached through a `[[plugins]]` chained reference, and a crate named by [`[plugins] use`](./configuration.md#plugins) or `auto-enable`. It is checked every time the active plugins are resolved (sync, hooks, subcommands, help), against the crate version that resolved and, only when a `digest` revocation exists for that crate, the payload digest of the crate's source.

- It applies to **crates** only. It does not apply to registry plugins or workspace plugins, which are content you chose to trust.
- It outranks every grant: `use` and `auto-enable` do not load a revoked crate, and its own chained references are not followed.
- [`[plugins] disable`](./configuration.md#plugins) outranks it. A crate that is both revoked and disabled counts as declined.
- A payload with no [digest](#when-there-is-no-digest) cannot match a `digest` revocation; a `versions` revocation still applies to it.
- The check never prompts and never prints. A revoked plugin is skipped quietly and never offered for consent; `cargo agents sync -v` names it with the reason, as in ``revoked by registry `symposium-audits`: it tells the agent to fetch and run remote code``.
- Skills a crate installed before its revocation come off on the next sync, even one that otherwise keeps installed skills because a registry could not be read or a package manager failed. With [`auto-sync`](./configuration.md) off, they stay installed until you run `cargo agents sync`; hooks and subcommands stop at once, since they are resolved on every call. MCP servers it registered at project scope are removed by the next full sync.

## How Symposium reads it

Symposium is lenient where the list's own checks are strict:

- A malformed entry is dropped with a warning naming the file and the entry, and the rest of the file is used: a digest that is not `sha256:` plus 64 lowercase hex characters, a revocation that sets both or neither of `versions` and `digest`, a requirement that does not parse, or a missing `reason`. A dropped revocation does not apply, which is why the warning says exactly what was dropped. An unknown `reason` code is not malformed: the revocation applies, with a warning.
- An `audit` or `revoked` key that is present but not an array of tables (`[revoked]` instead of `[[revoked]]`, or `revoked = "x"`) is dropped with a warning that says so, so a missing bracket cannot silently lose revocations. The rest of the file is still used.
- A file whose `format` is missing, or is not one Symposium knows, is ignored whole, with a warning: it contributes no audits and no revocations.
- Unknown keys are ignored. An `[[audit]]` entry is read for its `digest` only, so a malformed `versions` does not drop it.
- A file larger than 1 MiB is ignored whole, with a warning, revocations included. The list's own checks should keep files well under that.
- Warnings go to the log, never to standard output, because the same reader runs inside agent hooks.

A list that is unreachable, or has not been fetched yet, contributes nothing.
