# PM interface

## TL;DR

- Define an operation set (`initialize`, `active_plugins`, `load_plugin`, `list_deps`, `project_info`, `search`, `fetch`, `refresh`, `canonical_name`) that all package managers implement. `initialize` is the handshake that constructs the PM; the rest are what Symposium asks of it.
- A PM is a binary speaking newline-delimited JSON-RPC over stdio, one long-lived process per PM per Symposium invocation. A PM written against the SDK can also run in Symposium's own process, which is how the cargo PM runs by default.
- A PM returns plugin *manifests*, not just directories, so it can synthesize a plugin for a package with no `Symposium.toml`, or one whose manifest is in another ecosystem's format.
- Trust is assigned by Symposium, never claimed by the PM.

## Motivation

Symposium needs to fetch plugins from multiple ecosystems without hard-coding each one. The PM interface is the seam: implement a handful of operations and your ecosystem becomes a plugin source. Cargo, npm, pypi, and an enterprise's internal registry all arrive the same way — all without changing core.

## Change in a nutshell

A PM is a separate binary that speaks JSON-RPC over stdio. Here's the cargo PM responding to `load_plugin`:

```toml
# User writes in Symposium.toml:
[[plugins]]
source.cargo = "serde-skills>=1"
```

Symposium sends `load_plugin` with the id `(cargo, serde-skills, >=1)`. The cargo PM resolves the requirement, obtains the crate source, and returns the exact id, the content directory, and the plugin manifest it read (or synthesized) from that directory:

```json
{ "result": { "plugins": [{
    "id": { "pm": "cargo", "name": "serde-skills", "version": "1.2.3" },
    "root": "/home/user/.cargo/registry/src/index.crates.io-.../serde-skills-1.2.3",
    "manifest": { "skills": [{ "source": { "path": "skills" } }] }
}] } }
```

Symposium validates that manifest, applies its own defaults and trust rules, and resolves the skill group against `root`.

## Detailed plans

### Package-ids

A **package-id** is a tuple `(pm, name, version)` where all three components are PM-defined strings. There is no mandated string-serialized format — the tuple is the identity.

Examples:
- `(cargo, serde, 1.0.210)`
- `(git, git@github.com:rtk-ai/rtk#main, abc123def)`
- `(recommendations, cargo/serde, 0.1.0)`

In the JSON-RPC protocol, a package-id is represented as:

```json
{ "pm": "cargo", "name": "serde", "version": "1.0.210" }
```

### The protocol

A PM speaking the protocol is a binary invoked by Symposium. Communication uses JSON-RPC 2.0 over stdio, **newline-delimited**: one JSON object per line, no `Content-Length` framing. Nothing in the payloads needs an embedded newline, so the simpler framing is enough. Each PM binary is long-lived: Symposium spawns it once per invocation and sends it requests one at a time.

#### `initialize`

```json
// Request
{ "method": "initialize", "params": {
    "protocol_version": 1,
    "workspace": "/home/user/projects/my-app",
    "cache_dir": "/home/user/.symposium/cache"
} }

// Response
{ "result": { "protocol_version": 1, "name": "cargo" } }
```

Sent once, before any other method. Carries the per-invocation context the PM needs; environment variables travel the ordinary way, in the process environment the PM is spawned with. The PM answers with the name it owns (the `pm` component of every id it mints).

There is no capability list. A method a PM has no use for answers with nothing (an empty list, no workspace), which is what the SDK's server harness does by default, so Symposium has nothing to skip.

A PM is otherwise **self-contained**: it holds whatever it needs to resolve its own ecosystem, so no later method takes ambient context. This is why `workspace` lives here rather than on `list_deps` as originally proposed: with a long-lived process the workspace is fixed for the connection's lifetime.

Version negotiation is strict for now: a PM reporting a `protocol_version` Symposium doesn't know is refused with a warning, and its plugins are simply absent.

#### `active_plugins`

```json
// Request
{ "method": "active_plugins", "params": { "deps": [{ "pm": "cargo", "name": "serde", "version": "1.0.210" }] } }

// Response
{ "result": { "plugins": [{ "id": {...}, "root": "...", "manifest": {...} }], "errors": [] } }
```

The plugins this PM activates for the workspace's dependency set. The two shapes it covers:

- A **registry** instance lists its own entries and ignores `deps`.
- An **ecosystem transport** (cargo) surfaces the plugins its dependencies embed.

Whether the result may run without the user's consent is Symposium's decision, not the PM's: see [Enablement](#enablement).

`errors` (empty when left out) names each entry the PM could not read, and why. Such an entry is left out of `plugins`, and Symposium reports the error and keeps what that entry installed rather than reading its absence as its removal.

#### `load_plugin`

```json
// Request
{ "method": "load_plugin", "params": { "id": { "pm": "cargo", "name": "serde-skills", "version": ">=1" }, "update": "none" } }

// Response
{ "result": { "plugins": [{ "id": {...}, "root": "...", "manifest": {...} }] } }
```

The plugin(s) a *specific* id maps to: a `[[plugins]]` chained reference, or a crate the user enabled by name. Resolves the version requirement, obtains the content, and returns the plugin(s) found there. Returning zero plugins is not an error.

Under `"update": "none"` (the default) the PM serves only what is already on disk, so a hook event never reaches the network; Symposium asks with `"check"` when it syncs. This is the method the original `resolve` folded into. The version component of the request id may be a requirement (`">=1"`, or `"*"` for none); the response id always names the exact resolved version.

#### `list_deps`

```json
// Response
{ "result": { "deps": [{ "pm": "cargo", "name": "serde", "version": "1.0.210" }, { "pm": "cargo", "name": "tokio", "version": "1.38.0" }] } }
```

The dependencies of the workspace given at `initialize`, in this PM's ecosystem. PMs with no workspace notion return empty.

Contract:
- Direct dependencies only (not transitive).
- Must be fast: this is on the hook path. Read lockfiles, don't query the network, cache on the lockfile's mtime.

#### `project_info`

```json
// Response
{ "result": { "project": {
    "root": "/home/user/projects/my-app",
    "local_packages": ["/home/user/projects/my-app/crates/core"],
    "inputs": ["/home/user/projects/my-app/Cargo.lock"]
} } }
```

The project this PM finds from the directory given at `initialize`. Symposium's workspace is that project: where a project is rooted is each ecosystem's own rule, cargo's being the workspace root above any member, so only a PM can say.

- `root`: the project's root, where Symposium installs project configuration. Absent when this PM finds no project there. Every PM that finds one must report the same root; Symposium treats a disagreement as an error rather than choosing.
- `local_packages`: the packages that are part of the project rather than dependencies of it (cargo workspace members, npm workspace packages). Each may define a workspace plugin, which is trusted, so Symposium uses only those inside the root: a PM cannot point it at content from anywhere else.
- `inputs`: the files whose change means this PM's answers may have changed, such as its lockfile. Symposium records their modification times when it syncs, and decides whether a later hook event needs to sync from those times alone, so an unchanged workspace starts no PM.

A PM with no notion of a project answers with no root and both lists empty.

#### `search`

```json
// Request
{ "method": "search", "params": { "query": "serde" } }

// Response
{ "result": { "plugins": [{ "id": { "pm": "cargo", "name": "serde-skills", "version": "1.2.3" }, "description": "..." }] } }
```

Find packages matching a partial query string; backs `cargo agents use` and `cargo agents search`. PMs without a searchable registry return empty.

The query is a fragment of a name a person typed, never a package-id: the cargo
PM queries crates.io with it, a registry PM substring-matches its entry names.
Discovery does not use `search`: it works from `list_deps` and `active_plugins`
(see [discovery](../discovery-sync/README.md#the-discovery-algorithm)), so a PM
that implements nothing but `active_plugins` still participates fully in it.

#### `fetch`

```json
// Request
{ "method": "fetch", "params": { "id": {...}, "update": "none" } }

// Response
{ "result": { "id": {...}, "root": "/home/user/.cargo/registry/src/.../serde-skills-1.2.3" } }
```

Acquire a package's content and report where it landed, canonicalizing the id's version. `update` is `none` (serve from cache, never touch the network), `check`, or `fetch` (force).

Contract:
- The same package-id always produces the same content.
- The PM owns the directory and guarantees it stays valid for the connection's lifetime.
- `update: "none"` must not make a network call. This is what keeps per-event hook dispatch offline.

#### `refresh`

```json
// Request
{ "method": "refresh", "params": { "update": "check" } }

// Response
{ "result": { "refreshed": true } }
```

Pull the PM's backing source onto disk, for a git registry the repository, and say whether a remote source was pulled. Symposium sends it to each configured registry when it starts and on `cargo agents plugin sync`, skipping a registry whose `auto-update` is off unless the user names it. A PM with nothing to pull answers `false`, which is what the SDK's server harness does by default.

#### `canonical_name`

```json
// Request
{ "method": "canonical_name", "params": { "name": "widget_lib" } }

// Response
{ "result": { "name": "widget_lib" } }
```

The form in which two of this PM's names for the same package compare equal. Cargo treats `widget-lib` and `widget_lib` as one crate, so the cargo PM answers `widget_lib` for either; a PM whose names are exact answers with the name it was given, which is what the SDK's server harness does by default.

Symposium asks only about a name a person typed (a `[plugins]` entry, a `cargo agents use` argument), and asks about each name once per connection. Every id a PM answers with must spell its package the same way, so ids compare as they are, never through this method.

### What crosses the wire

A PM returns a **plugin manifest**, not merely a directory:

```json
{ "id": {...}, "root": "/path/to/content", "manifest": { /* Symposium.toml schema, as JSON */ } }
```

Returning a manifest rather than only a path is what lets a PM **synthesize** a plugin: for a package with no manifest at all, or one whose configuration lives in a different ecosystem's format (an npm PM reading `package.json`, say). A PM that does nothing special just parses the `Symposium.toml` it found and hands it back.

The manifest on the wire is the **raw, unvalidated** schema: the same shape a `Symposium.toml` deserializes into. Validation is Symposium's. Its steps live in the SDK, so a PM can check its own output with the same code, but only what Symposium runs counts:

| Concern | Owner |
|---------|-------|
| Producing a manifest (parse, synthesize, translate) | PM |
| Schema validation, inline-installation promotion | Symposium |
| Defaults (`skills/`, `.agents/skills/`), `[defaults]` handling | Symposium |
| Activation roots, trust, consent | Symposium |
| Resolving `source.path` against `root` | Symposium |

This split keeps policy in one place. A PM reports which plugins exist and what
they contain; which of them are enabled is decided from configuration and from
the source the plugin came from, neither of which is anything the PM says.

The schema is published as a Rust crate that both Symposium and PM authors depend on, so a Rust PM builds the manifest as a typed value rather than assembling JSON by hand. PMs in other languages target the JSON shape directly.

**Future optimization.** A PM could answer with `{"manifest_path": "Symposium.toml"}` instead of an inline manifest, letting Symposium read the file itself and skipping a serialize/deserialize round trip for the common case. Not needed to start.

### Enablement

A PM reports what is available. Symposium decides what runs, from two inputs:
the user's `[plugins]` configuration, and which source the plugin came from.

Some sources are trusted, meaning a plugin from them is enabled without the user
being asked:

- the **recommendations registry**,
- the **current workspace** (its root and members),
- the configured `[[registry]]` entries the user added by hand.

A plugin embedded in a dependency is not: depending on a package should not let
its author add to your agent's context, so it runs only once the user consents.

### Naming a plugin in configuration

To enable or disable a specific plugin, the user has to be able to name it, and
the name has to survive across runs. So every plugin has a **canonical name**,
supplied by the PM that offers it, and configuration entries are the pair
`(pm, canonical-name)`:

```toml
[plugins]
# Turn off one recommendation, overriding the registry's trusted-by-default
# status.
disable = [{ pm = "symposium-recommendations", name = "rtk" }]

# Consent to a plugin embedded in a dependency.
auto-enable = [{ pm = "cargo", name = "my-internal-crate" }]
```

Each PM picks names that are stable and meaningful for its ecosystem. The cargo
PM uses the crate name. A registry PM uses the plugin's own `name`, which its
manifest is required to declare.
The pair is qualified by PM so that two ecosystems using the same word do not
collide, and so that a name always identifies exactly one thing.

The `pm` half may be left out. A name on its own usually identifies one plugin,
and when it does it should just work: `use = ["rtk"]` wakes the one plugin
called `rtk`. Only a name that could mean plugins from more than one PM needs
qualifying, and Symposium says so rather than guessing which was meant.

This is what makes a trusted source overridable. Recommendations are enabled
without asking, which is the point of them, but a user who does not want a
particular one names it and turns it off. `disable` beats every other entry,
including a `use` naming the same plugin: see
[precedence](../discovery-sync/README.md#precedence).

### Error handling

Errors use JSON-RPC error codes:

| Code | Meaning | Symposium behavior |
|------|---------|-------------------|
| -32001 | Not found | Skip gracefully, report in `status` |
| -32002 | Network error | Retry with backoff, fall back to cache |
| -32003 | Invalid input | Hard error at parse time |
| -32004 | Auth required | Report to user with setup instructions |
| -32603 | Internal error: anything the PM did not classify | Contributes nothing, logged |

Beyond named codes, plugin loading is **best-effort** and must stay that way across the process boundary. A PM that errors, hangs past its timeout, crashes, or fails its `initialize` handshake degrades to "contributes no plugins," logged as a warning. One broken PM never aborts a sync or a hook: the same contract the in-process layer already holds, where a plugin that fails to load is dropped rather than surfaced.

Anything written to a PM's stderr is captured and logged at debug level, so a PM can be diagnosed without disturbing the protocol on stdout.

### PM lifecycle

Symposium manages PM binaries as follows:

1. On first use, Symposium spawns the PM binary, connects via stdio, and sends `initialize`.
2. The PM stays alive for the rest of the Symposium invocation, and is shut down when `PmRegistry` drops.
3. Spawning is **lazy**: a PM whose operations are never needed is never started.
4. Symposium may have several requests in flight (the PM handles this or serializes internally).

A PM binary is found one of three ways:

1. **Built in.** The PMs Symposium ships with are located by name, with no configuration required.
2. **Config-declared.** A `[[package-manager]]` section names the PM and points at an installation source, acquired through the same machinery hook binaries already use. This is the bootstrap channel: it cannot depend on plugins being loaded, since loading plugins is what needs PMs.
3. **Plugin-vended.** A plugin registers a new PM type, per the parent RFD's [future work](../README.md#future-work). The `initialize` handshake is designed so this needs no protocol change.

### Cache layout

Symposium hands each PM a `cache_dir` in the `initialize` handshake and the PM
caches whatever it likes underneath it. What goes there, and how it is arranged,
is entirely the PM's business: Symposium never reads or interprets the contents.

The trade runs both ways. A PM gets one canonical place to write, so it does not
have to invent a location or ask the user to configure one, and everything
Symposium caused to be downloaded is in one place. In exchange, Symposium may
delete that directory at any time, so a PM must treat it as a cache and never as
storage: anything it cannot rebuild does not belong there.

A PM is free to serve content from outside `cache_dir` when its ecosystem
already has a cache worth reusing. The cargo PM does exactly this, serving
sources out of `~/.cargo/registry/src/`, which is why the directory is offered
rather than imposed.

`fetch` returns a `root` the PM guarantees valid for the connection's lifetime.
Symposium reads it and never writes to it.

### Built-in PMs

`path` and `git` are built into the Symposium binary, for one reason: bootstrap.
A configured PM is a binary that has to be acquired, and acquiring anything means
reading a registry first. `path` and `git` are what make that first read possible,
so they cannot themselves be things you acquire. The default recommendations
registry is git-sourced, so a fresh install has to be able to read a git registry
before it has acquired anything at all.

Neither is built in because a separate process would be technically awkward.
`git` in particular does need the network, and the fetching and caching it needs
already exist in Symposium for git skill-group sources and hook binaries, so
building it in reuses machinery rather than adding any. If the bootstrap
constraint ever went away, either could become an ordinary PM binary without a
protocol change.

Every other PM needs ecosystem tooling that Symposium has no reason to carry, and
is a separate binary, with one exception: `cargo`.

#### `cargo`

Its own crate, run in Symposium's process by default and as the
`symposium-pm-cargo` binary when a `[[package-manager]]` entry named `cargo` asks
for it. Both run the same implementation. Symposium is a cargo tool, so this is
the one PM it cannot work without: it should not depend on finding a second
binary, and the hook path should not pay a spawn for it. See the
[cargo PM sub-RFD](../cargo-pm/README.md) for details.

#### `git` as a chained source

`source.git` on a `[[plugins]]` chained reference is still rejected. Git *registries* and git *skill-group* sources both work today through the built-in reader; what's missing is naming a git repository as a chained plugin. That does not obviously need a separate binary either, and is left open.

## Frequently asked questions

### Why JSON-RPC over stdio?

It's the same pattern used by MCP servers and LSP: well-understood, language-agnostic, and debuggable. It also means PMs can be written in any language.

### Why not compile PMs into the binary?

Language-agnosticism. We want npm/pypi PMs eventually, and those may be best written in JS/Python. Even for Rust-based PMs, the binary boundary keeps the core small and lets PMs be updated independently.

The cargo PM is the exception, compiled in behind the same trait: every workspace Symposium runs in needs it, so it should not be one more thing to install or spawn.

### Why is the manifest on the wire instead of a directory?

So a PM can describe a package that doesn't describe itself. A crate with a bare `skills/` directory has no manifest; an npm package's configuration would live in `package.json`. If the wire form were a path, every such case would need Symposium to learn that ecosystem's conventions, which is exactly what the PM boundary exists to avoid.

### Doesn't returning a manifest let a PM claim anything it likes?

It describes content, which is its job. What it does not decide is whether any
of that runs: validation, defaults, and enablement are applied by Symposium
after the manifest arrives, from configuration and from the source the plugin
came from. See [Enablement](#enablement).

### Who resolves version requirements — Symposium or the PM?

The PM. Symposium sends `load_plugin` with the requirement in the id's version component; the PM interprets the range for its ecosystem and answers with the exact version.

### What does this cost on the hook path?

On most hook events, nothing: whether to sync is decided from the modification times of the files each PM named as `project_info` inputs at the last sync, so no PM is asked. When one is, cargo costs nothing beyond the in-process work, since it runs in Symposium's process by default, and any other PM costs a process spawn per invocation. The property worth protecting is not "no subprocess" but "no `cargo metadata`": that's the expensive part, since it reads and resolves the whole graph. The `update: "none"` contract keeps `fetch` offline, `list_deps` caches on the lockfile mtime, and lazy spawning means an invocation that asks a PM nothing never starts it.

If spawn cost does turn out to matter for the configured PMs, the answer is a daemon mode for PMs (and for Symposium). That's a larger change and not proposed here.

## Implementation plan and status

### Step 1: Extract the manifest schema into a shared crate

Move the raw `Symposium.toml` schema and the predicate *syntax* types (parsing, `Display`, serde, not evaluation) into a crate both Symposium and PM authors depend on. Add `Serialize` alongside the existing `Deserialize`.

Tests: round-trip every manifest fixture in the repo through JSON and assert the validated `Plugin` is identical.

- [x] PR: manifest schema crate


### Step 2: Reshape the in-process trait to the wire shape

`active_plugins` / `load_plugin` return `{id, root, manifest}` instead of an already-validated plugin. Manifest *production* moves to the PM side; validation, defaults, and trust move to a single core seam. Still fully in-process: this is a refactor with no protocol involved, and it is what de-risks step 3.

Tests: the existing suite passes unchanged.

- [x] PR: unvalidated-plugin PM trait


### Step 3: PM process management

Newline-delimited JSON-RPC client, the `initialize` handshake, lazy spawn, lifecycle and shutdown, timeout and crash handling. A server harness in the SDK so a Rust PM is a `main` plus a trait impl.

Tests: a fixture PM binary that returns canned manifests, driven end to end; plus failure cases: a PM that exits immediately, one that returns malformed JSON, one that never answers.

- [x] PR: PM process manager + SDK harness
- [ ] PR: act on error codes (retry with backoff, `status` report, setup instructions)


### Step 4: Implement `symposium-pm-cargo`

Port workspace resolution, crate fetching, and crate-manifest merging into the binary.

Concretely:

1. Split the cargo PM into a library the binary wraps, so unit tests can keep driving it in-process through the trait.
2. Describe the workspace over the protocol. Core reads the workspace root and member directories in a dozen places; the cargo PM reports them, and its lockfile, through `project_info`, and its dependencies through `list_deps`. Loading plugins *from* those directories stays in Symposium: they are local directory reads, and the workspace is a trust root whose policy core owns.
3. Set `SYMPOSIUM_CARGO` in the child's environment, since the test harness's fake cargo is configured on Symposium's directories rather than in its process environment.

Tests: the cargo PM driven end to end through the real binary.

- [x] PR: cargo PM binary + tests


### Step 5: Configuration surface

The `[[package-manager]]` section and acquisition through the existing installation machinery, replacing the hard-coded lookup from step 3.

- [x] PR: `[[package-manager]]` configuration, `command` found on `PATH`
- [ ] PR: acquisition through the installation machinery

### Step 6: A non-Rust-ecosystem reference PM

One PM that synthesizes manifests from a foreign format, proving the boundary carries an ecosystem Symposium knows nothing about.

- [ ] PR: reference PM
