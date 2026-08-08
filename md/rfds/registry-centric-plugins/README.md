# Registry-centric plugin distribution

## TL;DR

Generalize Symposium's plugin system around *package managers* (PMs). A plugin is identified by a canonical tuple `(pm, name, version)`, fetched by its PM, and unpacked into a cached directory. Users install plugins with `symposium use`, projects auto-discover them via their dependencies, and predicates gate activation without changing what's installed.

## Motivation

**Leverage existing package managers.** Registries like crates.io already handle versioning, distribution, authentication, and mirroring. Enterprises already integrate them into their workflows. Rather than building our own distribution mechanism, we treat existing PMs as the delivery channel for plugins — keeping things simple for users and ops-free for us.

**Integrate across ecosystems.** Today Symposium only works with crates.io. We want to extend support to npm, PyPI, and beyond (including internal/proprietary registries). The PM abstraction makes each ecosystem a plug-in capability: implement four operations and your ecosystem's packages become plugin sources.

**Bundle executable code with plugin configuration.** Plugins can define hooks and MCP servers, but these need supporting binaries — a custom linter, a token-reduction tool like [RTK](https://github.com/rtk-ai/rtk/), a code generation tool. Today there's no clean way to distribute an executable alongside the TOML that references it. By connecting plugins to PMs, binaries and configuration ship together. The PM handles building and versioning; Symposium just fetches the directory and scans it.

## As a user

To start, users install symposium:

```bash
cargo install symposium
symposium init
```

### Dependency discovery

Symposium will automatically scan the dependencies of your project to find relevant plugins. This scan is done by executing `symposium sync`. Users can also configure Symposium to automatically sync every time an agent executes in their workspace.

When users run `symposium sync`, Symposium will scan their dependencies and look for eligible plugins. If it finds plugins that the user has not yet installed, it will prompt them to confirm installation. Users can approve the plugins or else decline; these choices are recorded in the Symposium configuration. We can expand these options later to e.g. permit "accept this automatically across all workspaces in the future" etc.

If auto-sync is not enabled, Symposium still checks to see if there are new plugins (or new versions of plugins) available since the user last synchronized. If there are, then a hint is added to the agent to prompt the user to run `symposium sync`.

### Workspace-local extensions

Projects can also define plugins that should be made available whenever that project is part of the user's active workspace (i.e., the user is hacking on that project). For example, consider a Rust project like `widget`, which has a workspace with two crates, `widget-lib` and `widget-test`:

```
widget/
  Cargo.toml <-- defines the workspace
  crates/
    widget-lib/
      Cargo.toml <-- defines the `widget-lib` crate
    widget-test/
      Cargo.toml <-- defines the `widget-test` crate
```

The user could add plugins alongside any of those `Cargo.toml` files and they'll be picked up by Symposium. We always activate all plugins for any project in the workspace, so you would get plugins from both `widget-lib` and `widget-test` regardless of which specific crate you are working on.

There are two ways to define a plugin. The simplest is to follow *common conventions* that Symposium supports:

* If you add skills into `.agents/skills`, they will be installed for anyone working in that workspace.
* If you add skills into `skills`, they will be installed for anyone working in that workspace *and* through dependency discovery.

You can also define a `Symposium.toml` that contains other kinds of plugins and extensions (e.g., mcp servers). We may add additional conventions in the future (e.g., apm, openplugin standard, etc).

To continue the `widget` example:


```
widget/
  Cargo.toml
  Symposium.toml                 <-- defines add'l plugins loaded when working in this workspace
  crates/
    widget-lib/
      Cargo.toml
      Symposium.toml         <-- defines add'l plugins loaded in this workspace; can also define
      skills/                      plugins for workspaces that depend on widget-lib
        widget-test-skill/ <-- available when working in the workspace 
          SKILL.md             *and* to other workspaces that depend on widget-lib
    widget-test/
      Cargo.toml
      .agents/
        skills/
          widget-test-skill/ <-- available when working in the workspace only
            SKILL.md
```

### Explicit use

Users can also explicitly install plugins with the `use` command. The default is to install the plugin locally for the current workspace.

```bash
symposium use X
```

This will search across all registries for a package named `X` and show the matches to the user. So, if `X` is a plugin name, it would show the most recent plugin; if there is an entry in the recommendations repository, that would also be shown. Users can pick the one(s) they wish to install. This will add the entries into `~/.symposium/config.toml` along with the workspace directory so that they are known to be activated.

Users can also install plugins globally:

```bash
symposium use --global X
```

This works the same way but activates those plugins across all workspaces.

`use` is also how a user reaches a plugin that nothing about the workspace implies: a curated plugin that names no dependency, so no [activation root](#activation-roots) would ever pick it up on its own.

Users could also edit their config.toml to define their specific predicates for when they want plugins to be activated (e.g., when a certain file is present in the workspace, for Rust workspaces only, etc).

### Turning plugins off

`disable` is the off switch, listing plugins that must not run whatever else says otherwise:

```toml
[plugins]
disable = [{ pm = "symposium-recommendations", name = "rtk" }]
```

It is deliberately the last word. Every other mechanism (a trusted registry, `auto-enable`, an explicit `use`) says a plugin *may* run; `disable` is the single place that says it may not. So a plugin that is both `use`d and disabled stays off, and re-enabling it means dropping the `disable` entry. `symposium use --remove` does not do that: it removes a `use` entry, so it cannot cancel a decision the user made in the other direction.

A decline at the discovery prompt is recorded here too, which is the same rule seen from the other side: having said "never ask again" about a dependency's plugin, the user does not get asked again, and does not silently get the plugin either.

Unlike `use`, a `disable` entry carries no workspace scope: it is global. See [enablement configuration](./discovery-sync/README.md#enablement-configuration) for the full precedence rules.


## As a crate author

The core workflows for *publishing plugins* via Symposium are as follows. We use Rust crates as an example but everything we say about cargo applies equally to other supported package registries like PyPI, npm, etc.

### Publishing in your crate

Rust crates (and packages in other languages) can package extensions within their sources that are distributed inline. Simply add skills or plugins directly into your repository and Symposium will pick them up.

Publishing plugins directly with your crate has the advantage that they are versioned together. But you may wish to be able to update plugins independently. In that case, you can have your crate's plugin redirect Symposium to load a chained plugin with another crate name, such as `widget-symposium`. This way you can publish `widget-symposium` as often as you like.

The conventions for publishing in your own crate are the same as when defining plugins for your workspace. Recalling our `widget` example:

```
widget/
    Cargo.toml
    crates/
      widget-lib/
        Cargo.toml
        Symposium.toml         <-- defines `[[plugins]] source.cargo = "widget-symposium>=1"`
      widget-test/
        Cargo.toml
      widget-symposium/
        Cargo.toml
```

### Publishing for someone else's crate

You can also add a plugin into the central symposium recommendations repository. This uses the "recommendations" package manager. Our convention is that the `symposium-recommendations` repository contains a subdirectory structure with directories named for other package managers:

```
symposium-recommendations/
    ...
    cargo/
      widget-lib/              <-- defines `[[plugins]] source.cargo = "widget-symposium>=1"`
        Symposium.toml
```

So you can add a new plugin in a subdirectory of `cargo` (e.g., `cargo/widget-lib`) that adds a plugin for that crate. When a project in the workspace has a dependency on a crate `widget-lib=1.2`, we will search for plugins that match `cargo:widget-lib:1.2` for all registered package managers. The cargo package manager uses this to find the source for `widget-lib` at version `1.2` and look for embedded plugins. The *recommendations* package manager looks for a directory `cargo/widget-lib` (the version is ignored) and returns a match.

### Publishing a plugin not associated with a crate

The `symposium-recommendations` repository can also be used to publish centralized plugins that don't have an associated crate or whatever. For example, this might be used to distribute a collection of skills from a github repository or to distribute a tool whose installation is not managed by Symposium. To do that, you simply add to the directory called `symposium`:

```
symposium-recommendations/
  symposium/
    yolo-skills/
      Symposium.toml             <-- defines whatever
```

## Key concepts

### Plugins

A *plugin* is defined by a directory with an optional `Symposium.toml` file. The directory is typically the root directory of a workspace or a project in the workspace, but it could also be specified via a path or be found in a cloned github repository or other means. If there is no `Symposium.toml` file, that is equivalent to having an empty file.

#### Plugin identifier

Every plugin has a canonical identifier — a tuple `(pm, name, version)` — as described in the [package managers](#package-managers) section.

#### Agentic extensions

`Symposium.toml` files contain the following kinds of content:

* `[[plugins]]` defines a set of additional *chained plugins*. If a plugin X defines a chained plugin Y, then whenever X is loaded, Y will be loaded.
* `[[skills]]` identifies directories where we should search for skills. Any skills found in there will be installed into the user's workspace in the appropriate place(s) for the agent(s) they've selected.
* `[[mcp]]` identifies mcp-servers.
* `[[hooks]]` identifies hooks. Symposium allows you to define vendor-neutral hooks that work for any vendor or vendor-specific hooks that target a particular agent (e.g., Claude Code or Codex).
* `[[installable]]` identifies *installable content*, which can be referenced by MCP servers or hooks (which need an executable). An easy option is to package your content as a cargo package that will be cargo-install'd and managed by Symposium, but there are other options.

#### Predicates

The plugin itself and each of its subsections can be gated with a `predicates = [...]` field (plus the `depends-on` shorthand). When a plugin is installed, the content is only *activated* if the predicates match. The full model is in the [predicates reference](../../reference/predicates.md); the functions are:

* `depends-on(<name>)`, true if some project in the workspace depends on `<name>`. A version requirement is allowed (`depends-on(serde>=1.0)`), and `depends-on(*)` matches any workspace.
* `workspace-member()`, true if the plugin this predicate belongs to is defined by a member of the active workspace.
* `env(FOO)` / `env(FOO=BAR)`, true if the environment variable is set (to `BAR`).
* `path_exists(<arg>)`, true if the argument resolves to an existing path — checked on the filesystem, then on `$PATH` for a bare name (so it matches a local file or an installed binary).
* `shell(<command>)`, true if `<command>` run via `sh -c` exits `0`.
* the combinators `not(<p>)`, `any(<p>, …)`, `all(<p>, …)`, which together give full boolean logic.

`depends-on` is sugar for the common dependency case: `depends-on = ["serde", "tokio"]` lowers to `any(depends-on(serde), depends-on(tokio))`, ANDed with any `predicates`.

Whether a plugin was **explicitly used** and whether it is a **workspace dependency** are *not* predicates. "Used" is the enablement axis, a `[plugins] use` entry (see [Explicit use](#explicit-use)), which is also the only [activation root](#activation-roots) available to a plugin that names no dependency; dependency presence is `depends-on(<name>)`. These are not mutually exclusive: a plugin can be a workspace member, a dependency, and explicitly used all at once.

#### Activation roots

Predicates say *when* a plugin applies, not why it was in play at all. That is a separate question, and every active plugin answers it with an **activation root**. There are three:

* **Workspace membership**, for a plugin defined by the workspace root or one of its members.
* **A dependency**, either one the plugin is embedded in, or one it names with `depends-on` and the workspace has (`depends-on = ["*"]` names every workspace).
* **An explicit `use` entry**, which needs nothing from the workspace at all.

`symposium status` reports the root each plugin came in on. A registry entry that names no dependency has none of the three until a `use` entry gives it one, so it loads and is listed but contributes nothing.

#### Default content

Finally, plugins have some default content that is added automatically unless it is disabled via a `[defaults]` section. Currently we have one default, `default.skills = (true|false)`. Assuming the default is not set to false, then the following is added to the plugin.

```toml
[[skills]]
source.path = "skills"

[[skills]]
predicates = ["workspace()"]
source.path = ".agents/skills"
```

These defaults establish the skills conventions described earlier. For example, the `widget-test` crate had skills defined in `.agents/skills`. If you were to depend on `widget-test`, but you don't have it in your workspace, those skills would *not* be added to your workspace, because they are gated behind a predicate.

### Package managers

A package manager (PM) is a pluggable backend that knows how to find, fetch, and enumerate plugins from a particular ecosystem. A PM may run in Symposium's own process or as a separate binary it speaks to over stdio; both implement the same operations, so nothing above the PM layer knows which it is talking to.

`path` and `git` are built in, since both only read local directories. `cargo` is a crate of its own that can run either way, and any other ecosystem (npm, pypi, an internal registry) arrives as a binary named by a `[[package-manager]]` config entry.

Every PM implements these operations:

| Operation | Input | Output | Used by |
|-----------|-------|--------|---------|
| `active_plugins` | the workspace's dependency ids | set of unvalidated plugins | discovery, sync |
| `load_plugin` | package-id | set of unvalidated plugins | chained references, `use` |
| `search` | partial query string | set of package-ids + metadata | `symposium use`, `symposium search` |
| `fetch` | package-id | directory with plugin content | sync/install |
| `list_deps` | (none) | set of package-ids | auto-discovery |
| `workspace_info` | (none) | workspace root and members | workspace plugins, scoping |
| `refresh` | update level | whether content was pulled | registry sync |

What a PM returns is an *unvalidated plugin*: a resolved id, a content directory, and an unvalidated manifest. Returning a manifest rather than only a directory is what lets a PM synthesize a plugin for a package that ships no manifest, or translate one from its own ecosystem's format, without Symposium learning that ecosystem's conventions. Validation and defaults are applied by Symposium once the manifest arrives. Which plugins actually run is a separate decision, made from the user's `[plugins]` configuration and from the source the plugin came from.

A **package-id** is a tuple `(pm, name, version)` where all three components are PM-defined strings. Examples: `(cargo, serde, 1.0.210)`, `(git, github.com/rtk-ai/rtk, abc123def)`, `(recommendations, cargo/serde, 0.1.0)`. There is no mandated string-serialized format — the tuple is the identity.

See the [PM interface sub-RFD](./pm-interface/README.md) for full protocol details.

#### Example: The recommendations registry

The `symposium-recommendations` repository is an ordinary flat registry, read by
the built-in `path` PM once its content has been fetched. Each entry is a plugin
directory that declares which crates activate it with its own `depends-on`:

```
symposium-recommendations/
  serde-guidance/
    Symposium.toml     # depends-on = ["serde"]
  tokio-guidance/
    Symposium.toml     # depends-on = ["tokio>=1"]
```

No dedicated PM and no namespace convention are involved, because none are
needed: a recommendation is just a plugin that activates when certain
dependencies are present, which the ordinary `depends-on` predicate already
expresses. The layout therefore carries no dependency information of its own,
and a recommendations entry is validated and gated exactly like any other
registry plugin.

#### Example: The cargo manager

The cargo package manager works with Symposium packages embedded within crates or cargo workspaces.

It defines package-ids like `(cargo, $crate-name, $version)`.

It defines the core operations as follows:

| Operation | Definition |
|-----------|------------|
| `resolve` | accepts a object like `{foo = "1"}` using the same format as expected by cargo; resolves per cargo algorithm |
| `search` | if PM = `cargo`, search cargo registry for matching crates; otherwise, return empty |
| `fetch` | creates a dummy project to populate the cargo cache and returns the crate source directory from there |
| `list-deps` | returns direct dependencies from the workspace `Cargo.toml` and all workspace members |

#### Example: The git manager

The git package manager works with Symposium packages found in git repositories.

It defines package-ids like `(git, $git-url, $sha-hash)`. The `git-url` component uses a URL fragment to encode the ref (following npm's convention), e.g., `git@github.com:rtk-ai/rtk#main`. The `version` is always the resolved commit SHA.

It defines the core operations as follows:

| Operation | Definition |
|-----------|------------|
| `resolve` | accepts an object like `{url = "...", branch = "...", rev = "..." }` and resolves to a commit SHA |
| `search` | returns empty (git repos aren't a searchable registry) |
| `fetch` | clones/fetches the repo at the specified commit SHA and returns the directory |
| `list-deps` | returns empty (no concept of "workspace depends on a git repo") |

## Frequently asked questions

### How does Symposium work in the enterprise?

Symposium routes all plugin distribution through existing package registries (crates.io, npm, PyPI, etc). Enterprises already operate internal mirrors and proxies for these registries — Symposium inherits that infrastructure automatically.

The primary control point is the **recommendations repository**. Companies supply their own `symposium-recommendations` crate (or override the default) to curate which plugins are offered to their developers. In the future, the recommendations repository may also supply allow/deny lists and other centralized controls (e.g., "these plugins are approved for production use," "these plugins require security review before installation"). This is left for future design.

Companies can also disable specific PMs entirely — for example, disabling the `git` PM to prevent developers from installing unvetted plugins from arbitrary repositories, restricting installs to only those that flow through a scanned registry.

### Why route through existing registries?

Routing through existing registries gives enterprises central scanning (malware, license, vulnerability), access control, audit trails, and air-gapped environment support — all using tooling they already have.

The tradeoff is that some plugins don't have a natural "home" in a language-specific registry (e.g., a collection of general-purpose agent skills not tied to any library). For these, the recommendations repository or a dedicated "symposium plugins" crate serves as the packaging vehicle — slightly artificial but consistent with the model.

## Detailed design

We plan follow-up RFDs with more details on each component:

- **[Plugin model](./plugin-model/README.md)** — what a plugin is, `Symposium.toml` structure, defaults (skill discovery, implicit installations), predicates, chained plugins, installed vs. active.
- **[PM interface](./pm-interface/README.md) + [Cargo PM](./cargo-pm/README.md)** — the JSON-RPC protocol for PM binaries, error semantics, caching contract. The cargo PM specifically: `resolve` schema, `fetch` via cargo toolchain, `list-deps` from `Cargo.lock`.
- **[Discovery & sync](./discovery-sync/README.md)** — the two-phase discovery algorithm (`list-deps` on all PMs, then `search` on all PMs for each dep), hook-triggered notification, prompt UX, auto-install configuration.
- **[User-managed plugins](./user-managed-plugins/README.md)** — `symposium use`/`remove`/`status` commands, config file format, version requirement syntax, global vs. workspace-local scoping.

### Future work

The remaining work, roughly in dependency order:

- **Acquiring a PM binary**: a `[[package-manager]]` entry names a command that must already exist. Running it through the existing installation machinery (`source = "cargo"` / `"github"`, as hooks and subcommands do) would let an entry install what it names. Plugin-vended PMs layer on after that.
- **Registries as PMs over the wire**: `path` and `git` registries stay in-process, since both only read local directories. Nothing stops a registry from being a PM binary too; there has just been no reason yet.
- **PMs defined by plugins** — letting a plugin *register a new PM type* (so an org can ship an internal-registry PM, or an ecosystem PM like npm/pypi, as an ordinary plugin). Depends on the out-of-process protocol above; the registration and discovery mechanism is TBD.
- **Additional built-in ecosystems** — there is no `git` PM yet (git *sources* for skill groups and installations exist, but a chained `source.git` is rejected); npm/pypi are unstarted.
- **Custom predicate dispatch across plugins (fixed-point)** — a crate-embedded plugin can *define* a custom predicate, but its definition is not yet registered, so it cannot be evaluated (only registry plugins' custom predicates are). Wiring a crate's *own* custom predicates into its facet evaluation is tractable; the general case — one plugin defines a predicate that another plugin's gate references — needs a convergence loop, since the definition must be loaded before the gate that uses it can be evaluated.
- **Chained-edge version enforcement** — `[[plugins]] source.cargo = "widget>=1"` records the version requirement but does not enforce it: expansion enqueues the crate with no version, so it resolves against the workspace pin regardless. Enforcement would compare the resolved version to the recorded requirement and warn/skip on mismatch.
- **Workspace-scoped `disable`**: a `use` entry can be scoped to one workspace; a `disable` entry cannot, so turning a plugin off in one project turns it off everywhere. The scoping machinery already exists on the `use` side, so this is mostly a matter of deciding how a scoped "off" and a global "on" compose.
- **Policy plugins** — org-level enforcement (deny-lists, approval gates). Separate extension point, design TBD.

## Implementation status

1. **Plugin model.** Plugins, `[defaults]`, predicates, chained plugins, activation roots.
2. **PM interface and the cargo PM.** The identity tuple, the operation set, the JSON-RPC transport (`symposium_sdk::pm::protocol` and `pm::server` on the PM's side, `pm::RemotePm` on Symposium's), and `symposium-pm-cargo` as a standalone crate and binary. A PM answers with an `UnvalidatedPlugin`: an id, a content directory, and an unvalidated manifest, so it can synthesize a plugin for a package that has none. `[[package-manager]]` config entries add ecosystems beyond cargo.
3. **Discovery and sync.** Dependency-embedded plugin discovery, the consent prompt, and the `[plugins]` config. Recommendations are a flat registry rather than a `search` result (see the note under [the recommendations registry](#example-the-recommendations-registry)).
4. **User-managed plugins.** `use` / `remove` / `status`, workspace vs. global scope.
5. **Remaining** — see [Future work](#future-work).
