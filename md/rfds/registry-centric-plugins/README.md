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

Users could also edit their config.toml to define their specific predicates for when they want plugins to be activated (e.g., when a certain file is present in the workspace, for Rust workspaces only, etc).


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
        Symposium.toml         <-- defines `[[plugins]] source.cargo = { widget-symposium = "1" }`
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
      widget-lib/              <-- defines `[[plugins]] source.cargo = { widget-symposium = "1" }`
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

The plugin itself and each of its subsections can be gated with a `predicates = [...]` field. When a plugin is installed, the content is only *activated* if the predicate matches.

Common predicates include:

* `workspace()`, true if this plugin is part of the active workspace;
* `used()`, true if this plugin was explicitly used by the user;
* `workspace-dependency()`, true if plugin is a dependency of some project in the current workspace;
* `env(FOO=BAR)`, true if the environment variable `FOO` is set to `BAR`;
* `file-exists(path)`, true if the given file exists;
* `depends-on(package-id)`, true if some project in the workspace depends on `package-id`;
    * This is delegated to the installed Symposium package managers. By default it refers to any of them, but you can be more specific, e.g., `depends-on(cargo, serde, 1.0)` or `depends-on(npm, leftpad)`.
* `shell(command)`, true if the given command exits with code 0;
* `workspace-directory(/home/ferris/dev/rust)`, true if the active workspace is a subdirectory of the given path.

Note that the first three predicates are not mutually exclusive. A given plugin may (a) appear in the workspace; (b) appear in the dependencies of a project in the workspace; and (c) be explicitly used all at the same time.

There is also a shorthand for `depends-on` that reuses the PM's `resolve` format:

```toml
[depends-on]
cargo = { tokio = "1", serde = "1" }
```

This is equivalent to `predicates = ["depends-on(cargo, tokio, 1)", "depends-on(cargo, serde, 1)"]` — the value under `depends-on.$pm` is passed to that PM's `resolve` to determine what packages are referenced.

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

A package manager (PM) is a pluggable backend that knows how to find, resolve, fetch, and enumerate plugins from a particular ecosystem. Each PM is a separate binary that Symposium invokes — installed as an `[[installable]]` from either the recommendations repository or the user's root config. The `path` PM is built into the Symposium binary itself (since it just reads local directories), but `cargo`, `git`, and any future PMs (npm, pypi, etc.) are separate binaries.

Every PM implements four operations:

| Operation | Input | Output | Used by |
|-----------|-------|--------|---------|
| `resolve` | opaque TOML value (from `source.<pm>`) | set of package-ids | manifest processing |
| `search` | partial query string | set of package-ids + metadata | `symposium use` |
| `fetch` | package-id | directory with plugin content | sync/install |
| `list-deps` | workspace directory | set of package-ids | auto-discovery |

A **package-id** is a tuple `(pm, name, version)` where all three components are PM-defined strings. Examples: `(cargo, serde, 1.0.210)`, `(git, github.com/rtk-ai/rtk, abc123def)`, `(recommendations, cargo/serde, 0.1.0)`. There is no mandated string-serialized format — the tuple is the identity.

See the [PM interface sub-RFD](./pm-interface/README.md) for full protocol details.

#### Example: The recommendations manager

The recommendations PM is provided by the `symposium-recommendations` crate. It operates over a repository of curated plugin directories, organized by the PM namespace they relate to:

```
symposium-recommendations/
  cargo/
    serde/
      Symposium.toml
    tokio/
      Symposium.toml
  symposium/
    yolo-skills/
      Symposium.toml
```

It defines the core operations as follows:

| Operation | Definition |
|-----------|------------|
| `resolve` | accepts a string `"foo"` or a list of strings `["foo", "bar"]` and treats them as in search |
| `search` | if PM is specified, search the `pm/name` directory; otherwise, search all directories |
| `fetch` | load the plugin from `pm/name` directory |
| `list-deps` | returns empty set |

Note: the recommendations PM participates in discovery not via `list-deps` but via `search`. The discovery flow calls `list-deps` on all PMs (e.g., cargo returns `(cargo, serde, 1.0.210)`), then for each dependency calls `search` on all PMs with the full tuple. The recommendations PM matches on `(pm, name)` and ignores the version component. This is where the recommendations PM gets to offer advice for other PMs' dependencies.

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
- **PM interface + Cargo PM** — the JSON-RPC protocol for PM binaries, error semantics, caching contract. The cargo PM specifically: `resolve` schema, `fetch` via cargo toolchain, `list-deps` from `Cargo.lock`.
- **Discovery & sync** — the two-phase discovery algorithm (`list-deps` on all PMs, then `search` on all PMs for each dep), hook-triggered notification, prompt UX, auto-install configuration.
- **User-managed plugins** — `symposium use`/`remove`/`status` commands, config file format, version requirement syntax, global vs. workspace-local scoping.

### Future work

- **Fixed-point resolution** — a convergence loop for when plugins define custom predicates that other plugins depend on. Needed once custom predicates are used across plugin boundaries.
- **Custom PMs** — allowing plugins to define new PM types (npm, pypi, internal). The PM interface is the seam; registration and discovery mechanism TBD.
- **Policy plugins** — org-level enforcement (deny-lists, approval gates). Separate extension point, design TBD.

## Implementation order

1. **Plugin model** — what a plugin is, defaults, predicates, chained plugins.
2. **PM interface + Cargo PM** — the foundation. Identity tuple, four operations, JSON-RPC protocol, first real PM.
3. **Discovery & sync** — PM `list-deps` + `search`, prompt UX. Enables "plugins from your dependencies."
4. **User-managed plugins** — `use`/`remove`/`status` UX.
5. **Future work** — fixed-point, custom PMs, policy — as demand arises.
