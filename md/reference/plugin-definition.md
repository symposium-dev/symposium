# Plugin definitions

A **symposium plugin** collects together all the extensions offered for a particular crate. Plugins are directories containing a `SYMPOSIUM.toml` manifest file that references skills, hooks, MCP servers, and other resources relevant to your crate. These extensions can be packaged within the plugin directory or the plugin can contain pointers to external repositories.

Plugins enable capabilities beyond standalone skills — they're needed when you want to add hooks or MCP servers. For simple skill publishing, see [Authoring a plugin](../crate-authors/authoring-a-plugin.md) instead.

## Example: a plugin definition with inline skills

You could define a plugin definition with inline skills by having a directory struct like this:

```
myplugin/
  SYMPOSIUM.toml
  skills/
    skill-a/
      SKILL.md
    skill-b/
      SKILL.md
```

where `myplugin/SYMPOSIUM.toml` is as follows:

```toml
name = "example"
depends-on = ["*"]

[[skills]]
source.path = "skills"
```

## Top-level fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | registry manifests only | Plugin name. Used in logs and CLI output. Required in a registry manifest. A crate manifest defaults it to the crate name, and a workspace manifest to the name of its directory. |
| `depends-on` | string or array | no | Which crates this plugin applies to. Use `["*"]` for all crates. See [Plugin-level filtering](#plugin-level-filtering). |
| `predicates` | array of strings | no | Predicates (`depends-on`, `shell`, `path_exists`, `env`, `workspace-member`, `not`, `any`, `all`) that must all hold for the plugin to apply. See [Predicates](./predicates.md). |
| `installations` | array of tables | no | Named installation declarations (`[[installations]]`). Hooks, subcommands, and custom predicates reference these by name. See [Installations](#installations). |
| `skills` | array of tables | no | Skill groups (`[[skills]]`). See [`[[skills]]` groups](#skills-groups). |
| `hooks` | array of tables | no | Hooks (`[[hooks]]`). See [`[[hooks]]`](#hooks). |
| `predicate` | array of tables | no | Custom predicate definitions (`[[predicate]]`). See [Custom predicates](#predicate). |
| `mcp_servers` | array of tables | no | MCP server registrations (`[[mcp_servers]]`). See [`[[mcp_servers]]`](#mcp_servers). |
| `plugins` | array of tables | no | Chained plugin references (`[[plugins]]`): other plugins that load whenever this one is active. See [Chained plugins](#chained-plugins). |
| `subcommand` | table of tables | no | Commands added to `cargo agents` (`[subcommand.<name>]`). See [Subcommands](#subcommandname). |
| `defaults` | table | no | `[defaults] skills = <bool>` (default `true`): whether to append the default skill groups: `source.path = "skills"`, plus the `.agents/skills` group in a workspace manifest. Accepted only in crate and workspace manifests; a registry manifest that contains `[defaults]` is rejected. |

A registry plugin gets no default skill group: its skills load only through the `[[skills]]` groups it declares, so a registry plugin that ships a `skills/` directory needs `[[skills]] source.path = "skills"`. Crate and workspace plugins get the default `skills/` group unless `[defaults] skills = false` (workspace plugins also get a `.agents/skills` group by default, see [Workspace skills](../workspace-skills.md)).

**Note**: A registry plugin that references no dependency anywhere is **dormant**: it loads, but it never activates until the user enables it by name in the [`[plugins] use`](./configuration.md#plugins) config (for example with [`cargo agents use`](./cargo-agents-use.md)). Use `depends-on = ["*"]` for a plugin that should always be active. These count as referencing a dependency:

- a `depends-on` field, or a `depends-on(...)` [predicate](./predicates.md) in a `predicates` list, on the plugin itself, a `[[skills]]` group, an `[[mcp_servers]]` entry, or a `[[plugins]]` edge;
- a `depends-on(...)` predicate in a `[[hooks]]` entry's `predicates`;
- a [custom predicate](#predicate) used directly in the plugin-level `predicates`.

A `depends-on(...)` predicate counts wherever it sits in the list, including `depends-on(*)` and one nested inside `not`, `any`, or `all`.

These do not count: the crate named by a `[[plugins]]` edge's `source.cargo`, a `[subcommand.<name>]`'s `depends-on` or `predicates`, and the frontmatter of a `SKILL.md` inside a `[[skills]]` group. Crate and workspace plugins never go dormant: what loaded the crate (a chained reference or the user's enablement), or membership in the active workspace, is their gate.

## Plugin-level filtering

The top-level `depends-on` field controls when the entire plugin is active:

```toml
name = "my-plugin"
depends-on = ["serde", "tokio"]  # Only active in projects using serde OR tokio
```

Use the wildcard `depends-on = ["*"]` instead for a plugin that always applies.

Plugin-level filtering is combined with skill group filtering using AND logic — both must match for skills to be available.

## `[[skills]]` groups

Each `[[skills]]` entry declares a group of skills.

| Field | Type | Description |
|-------|------|-------------|
| `depends-on` | string or array | Which crates this group advises on. Accepts a single string (`"serde"`) or array (`["serde", "tokio>=1.0"]`). See [Dependency predicates](./depends-on.md) for syntax. |
| `predicates` | array of strings | Predicates (`depends-on`, `shell`, `path_exists`, `env`, `workspace-member`, `not`, `any`, `all`) that must all hold for the group to install. See [Predicates](./predicates.md). |
| `source.path` | string | Local directory containing skill subdirectories. Resolved relative to the manifest file. |
| `source.git` | string | GitHub URL pointing to a directory in a repository (e.g., `https://github.com/org/repo/tree/main/skills`). Symposium downloads the tarball, extracts the subdirectory, and caches it. |

A skill group must have exactly one of `source.path` or `source.git`. A crate is not a skill-group source; to load a crate's own skills, name it in a [chained plugin](#chained-plugins).

## Chained plugins

A `[[plugins]]` entry names *another* plugin that loads whenever this plugin is active (the "a package is a plugin" edge). Today the referenced plugin is a **crate**, which always loads as a first-class plugin built from its manifest sources (see [Crate-embedded manifest](#crate-embedded-manifest) below). A recommendations entry uses it to load a crate's own plugin without asking the user first; see [Supporting your crate](../crate-authors/supporting-your-crate.md).

| Field | Type | Description |
|-------|------|-------------|
| `source.cargo` | string or table | The crate carrying the plugin. A dependency-atom string (`"serde"`, `"serde>=1"`) or a `{ name = "...", version = "..." }` table. |
| `depends-on` | string or array | Gate for this edge — the referenced plugin loads only when these hold (in addition to the owning plugin's own gate). |
| `predicates` | array of strings | Additional gate for this edge. See [Predicates](./predicates.md). |

```toml
name = "serde-plugin"

# When serde is a dependency, load serde's plugin (its skills).
[[plugins]]
depends-on = ["serde"]
source.cargo = "serde"
```

The edge's `depends-on` decides *whether* to load the referenced crate; the crate name in `source.cargo` decides *which* crate. List several `[[plugins]]` entries to load several crates.

> Only `source.cargo` is supported today; `source.git` / `source.path` chained plugins are reserved and rejected with a clear error.

### Crate-embedded manifest

A referenced crate describes its plugin with the ordinary plugin-manifest schema, from **two interchangeable sources**: a `SYMPOSIUM.toml` at its source root, and/or a `[package.metadata.symposium]` table in its `Cargo.toml`. Both are honored the same as a registry manifest: `[[skills]]` groups, per-group predicates, `source.path` / `source.git` sources, and further `[[plugins]]` chained references. The crate's effective manifest is the two sources merged over the crate defaults (merge order **defaults -> `[package.metadata.symposium]` -> `SYMPOSIUM.toml`**): list entries from both are kept; where the two set the same scalar, the file wins. Each source is parsed leniently: a malformed layer is logged and dropped, and the crate still resolves through the remaining layers (at minimum the default `skills/` group). A merged manifest that parses but fails validation (for example a hook naming an unknown installation) disables the crate's whole plugin, default `skills/` group included.

Because the chained reference is already the gate, a crate manifest may omit `name` (defaults to the crate) and a top-level `depends-on`; the default `skills/` group is appended unless `[defaults] skills = false`. A crate with no manifest sources at all still resolves as a plugin whose only content is that default `skills/` group.

**The opt-out belongs to the referenced crate, not the referrer.** The edge decides only *whether* to load the crate (via its `depends-on` / `predicates`); it cannot toggle the crate's defaults. So for an active edge to crate `foo`:

- `foo` ships nothing → its `skills/` directory loads.
- `foo` declares `[[skills]] source.path = "guidance"` → both `guidance/` **and** `skills/` load (combined with defaults).
- `foo` declares `[defaults] skills = false` plus a custom group → only the custom group loads.
- `foo` declares `[defaults] skills = false` and nothing else → nothing loads.
- `foo` carries its own `[[plugins]] source.cargo = "bar"` → `bar` resolves the same way, recursively.

Everything a crate manifest declares loads like a registry plugin's: skills, hooks, MCP servers, subcommands, custom predicates, and further `[[plugins]]` references.

### Delegating to another crate

A crate can delegate to another crate with a `[[plugins]]` chained reference of its own:

```toml
# In the referenced crate's Cargo.toml (in its SYMPOSIUM.toml, write the table as [[plugins]])
[[package.metadata.symposium.plugins]]
source.cargo = "companion-crate"
```

Chained references are followed until no new crate is reached; there is no depth limit. A crate reached through several references loads only once (crates are identified by name, hyphen/underscore-insensitively), which also makes cycles harmless, and a `SKILL.md` reached two ways installs once, since a skill is identified by the path of its `SKILL.md`. See [Supporting your crate](../crate-authors/supporting-your-crate.md) for the full crate-author walkthrough.

## Installations

An **installation** describes how to obtain (and optionally pre-configure) something a hook, [subcommand](#subcommandname), or [custom predicate](#predicate) will run. They reference an installation as their `command`, either by name (`command = "mytool"`) or inline at the use site (`command = { script = "scripts/x.sh" }`).

A `[[installations]]` entry has a `name` plus any of:

| Field | Type | Description |
|-------|------|-------------|
| `source` | string | Optional. How to acquire bits onto disk: `cargo` or `github`. Each source takes its own fields, listed under [Installation sources](#installation-sources). When omitted, no acquisition step runs. |
| `install_commands` | array of strings | Optional. Shell commands run (in order) after the source step. Useful for post-install setup such as aliasing, or when *only* have manual commands. Each command must exit zero. |
| `requirements` | array | Optional. Other installations to acquire whenever this one is referenced. Strings name `[[installations]]` entries; tables are inline declarations. |
| `executable` | string | Optional. Path to a binary to run. For `cargo`, the binary name (looked up in the install's `bin/` dir). For `github`, a path inside the acquired tree. With no source, a path on disk, relative to the directory of the manifest that declares it. |
| `script` | string | Optional. Same resolution rules as `executable`, but invoked as `sh <path> <args>`. |
| `args` | array of strings | Optional. Default invocation arguments. |


`executable` and `script` are mutually exclusive — pick one. The hook layer applies the same rule, and **at most one of `executable` / `script` may be set across the hook AND the installation it references**. An installation may have neither (then it's pure setup — useful as a `requirements` entry). For a hook to run, the chosen layer pair must end up with exactly one runnable.

> Inline installations (used as `command` or as a requirement entry) accept the same fields, including `requirements`.

### Installation sources

#### `cargo`

```toml
[[installations]]
name = "rg"
source = "cargo"
crate = "ripgrep"
version = "13.0.0"     # optional; defaults to latest stable
executable = "rg"      # the binary to run; if omitted and the crate has a single binary, that one is used
args = ["--version"]   # optional default args
```

| Field | Required | Description |
|-------|----------|-------------|
| `crate` | yes | Name of the crate to install. |
| `version` | no | Exact version to install (`"13.0.0"`). Defaults to the latest stable release on crates.io. |
| `git` | no | Install from this git repository (`cargo install --git`) instead of crates.io. Requires `executable` on the installation. |
| `global` | no | `true` installs into the user's global cargo location instead of Symposium's cache (see below). Requires `executable` on the installation. |

Symposium attempts `cargo binstall` first, falls back to `cargo install`, and caches the result under `~/.symposium/cache/binaries/<crate>/<version>/bin/` (passing `--root` so the install doesn't pollute `~/.cargo/bin`). The chosen `executable` resolves to `<cache>/bin/<executable>`. Hooks that depend on this installation get `<cache>/bin/` prepended to `$PATH`, so scripts can invoke the binary by name.

To install from a git repo instead of crates.io, set `git`:

```toml
[[installations]]
name = "tool"
source = "cargo"
crate = "tool"
git = "https://github.com/example/tool"
executable = "tool"   # required for git sources (crates.io is not consulted)
```

To install into the user's global cargo location (`~/.cargo/bin`) instead of a symposium-managed cache, set `global = true`. No `--root` is passed; `$PATH` is not augmented (the binary is expected to already be on `$PATH`). This can be useful if you are using scripts which require globally-installed programs, or if you want to use tools separately in a CLI. A `global = true` installation must set `executable`.

```toml
[[installations]]
name = "rg"
source = "cargo"
crate = "ripgrep"
executable = "rg"
global = true
```

#### `github`

```toml
[[installations]]
name = "mytool-hooks"
source = "github"
url = "https://github.com/example/mytool-hooks"
script = "hooks/claude/mytool-rewrite.sh"   # optional; see below
args = ["--format"]
```

| Field | Required | Description |
|-------|----------|-------------|
| `url` | yes | `https://github.com/<owner>/<repo>`, or `https://github.com/<owner>/<repo>/tree/<ref>/<path>` for one directory of the repository. `git` is accepted as another name for this field. |

Acquires the repo (or a subtree, if `url` points at `…/tree/<ref>/<path>`) into a local cache. The chosen `executable` / `script` resolves to a file inside the cached tree.

`executable`/`script` may be set on the installation or on the hook (but not both, in any combination). Setting it on the installation pins this entry to a specific file; omitting it lets multiple hooks each pick a different file.

#### no source

Omit `source` entirely when you just need to point at a path on disk (or rely on `install_commands` to put one there):

```toml
[[installations]]
name = "tool"
executable = "/usr/local/bin/tool"
```

Or "shell-only" installations — useful as side-effect requirements:

```toml
[[installations]]
name = "setup"
install_commands = [
    "ln -sf $HOME/.cache/foo $HOME/.local/bin/foo",
]
```

## `[[hooks]]`

Each `[[hooks]]` entry declares a hook that responds to agent events. For the JSON schemas that symposium-format hooks receive and produce, see [Symposium hook events](./hook-events.md).

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Descriptive name for the hook (used in logs). |
| `event` | string | Event type to match: one of the [supported hook events](#supported-hook-events) (e.g., `PreToolUse`). |
| `matcher` | string (optional) | Regex over the tool name, as the agent names it (e.g., `Bash` in Claude Code, `bash` in Copilot; see [Matcher](./hook-events.md#matcher)). The regex is unanchored, so `Bash` also matches `BashOutput`; anchor it (`^Bash$`) to match one tool. Omit to match all. |
| `command` | string or table | What to run. A string names a `[[installations]]` entry; a table is an inline installation (promoted to a synthetic entry named after the hook). |
| `executable` | string (optional) | Path to a binary inside (or relative to) the installation. At most one of `executable`/`script` set across hook + installation. |
| `script` | string (optional) | Path to a shell script to run via `sh`. Same exclusivity rule as `executable`. |
| `args` | array (optional) | Invocation arguments. Forbidden when the installation also declares `args`. |
| `requirements` | array (optional) | Installations to acquire before running. Same shape as `command` (string name or inline declaration). |
| `agent` | string (optional) | Restrict the hook to a specific agent (`antigravity`, `claude`, `copilot`, `kiro`, ...). It does not change the input; `format` still decides that (canonical JSON by default). |
| `format` | string | Wire format the handler expects on stdin. `symposium` (default): symposium converts the agent's event to its canonical format before delivering. Any agent name (`antigravity`, `claude`, `codex`, `copilot`, `kiro`): the hook fires only when Symposium runs for that agent, and receives that agent's native input unchanged; on every other agent it does not fire, and nothing is converted. Pair it with a `format = "symposium"` hook to cover the other agents (see [Agent-specific hooks](#agent-specific-hooks)). Symposium always intermediates: it never registers plugin hooks directly into agent configs. See [Hooks](../crate-authors/authoring-a-plugin.md#hooks). |
| `predicates` | array (optional) | Predicates (`depends-on`, `shell`, `path_exists`, `env`, `workspace-member`, `not`, `any`, `all`) that must all hold for the hook to dispatch. Evaluated per-dispatch. A hook has no `depends-on` field: gate it on a dependency with `predicates = ["depends-on(serde)"]`. See [Predicates](./predicates.md). |

### Examples

Run a cargo-installed binary as the hook:

```toml
[[installations]]
name = "rg"
source = "cargo"
crate = "ripgrep"
executable = "rg"

[[hooks]]
name = "rg-version"
event = "PreToolUse"
command = "rg"
args = ["--version"]
```

Install a tool as a side requirement and run a hook script from a separate github source:

```toml
[[installations]]
name = "mytool"
source = "cargo"
crate = "mytool"

[[installations]]
name = "mytool-hooks"
source = "github"
url = "https://github.com/example/mytool-hooks"

[[hooks]]
name = "rewrite"
event = "PreToolUse"
requirements = ["mytool"]
command = "mytool-hooks"
script = "hooks/claude/mytool-rewrite.sh"
args = ["--format"]
```

Inline a one-off cargo install directly:

```toml
[[hooks]]
name = "rg-test"
event = "PreToolUse"
command = { source = "cargo", crate = "ripgrep", executable = "rg" }
args = ["--version"]
```

Run a script file on disk (no source):

```toml
[[hooks]]
name = "check"
event = "PreToolUse"
command = { script = "scripts/check.sh", args = ["--strict"] }
```

A cargo install with a post-install step (e.g. to symlink a wrapper script):

```toml
[[installations]]
name = "mytool"
source = "cargo"
crate = "mytool"
install_commands = [
    "ln -sf $HOME/.symposium/cache/binaries/mytool/*/bin/mytool $HOME/.local/bin/mytool",
]

[[hooks]]
name = "mytool-rewrite"
event = "PreToolUse"
command = "mytool"
args = ["rewrite"]
```

### Agent-specific hooks

An agent-specific hook expects a particular agent's native wire format on stdin. Use this when you need full access to an agent's event schema. Symposium still intermediates, but an agent-specific hook fires only when Symposium runs for that agent, and receives that agent's input unchanged. On every other agent it does not fire, and nothing is converted. To cover the other agents, pair it with a `format = "symposium"` hook.

A plugin with a Claude-specific hook and a symposium fallback:

```toml
[[installations]]
name = "my-hook-binary"
source = "cargo"
crate = "my-hook-binary"

[[hooks]]
name = "check-claude"
event = "PreToolUse"
format = "claude"
command = "my-hook-binary"

[[hooks]]
name = "check-portable"
event = "PreToolUse"
format = "symposium"
command = "my-hook-binary"
args = ["--symposium"]
```

On Claude, `check-claude` fires (receives Claude's native JSON). On other agents, `check-portable` fires (receives symposium canonical JSON).

### Requirements

`requirements` ensures other installations are acquired before the hook runs. Useful when the hook's command relies on something else being on disk (or eventually on `$PATH`).

```toml
[[installations]]
name = "mytool"
source = "cargo"
crate = "mytool"

[[hooks]]
name = "uses-mytool-via-script"
event = "PreToolUse"
requirements = ["mytool", { source = "cargo", crate = "ripgrep" }]
command = { script = "scripts/uses-mytool.sh" }
```

Requirements may also be declared on an `[[installations]]` entry. Whenever that installation is referenced — as a hook's `command` or in another `requirements` list — its declared requirements are appended (one level, prerequisites first):

```toml
[[installations]]
name = "mytool"
source = "cargo"
crate = "mytool"

[[installations]]
name = "mytool-hooks"
source = "github"
url = "https://github.com/example/mytool-hooks"
requirements = ["mytool"]   # mytool gets installed whenever mytool-hooks is used

[[hooks]]
name = "rewrite"
event = "PreToolUse"
command = "mytool-hooks"
script = "hooks/claude/mytool-rewrite.sh"
```

Requirement installation is best-effort: failures are logged and dispatch continues.

### Hook environment

Hooks are spawned with the following extras on top of the parent environment:

| Variable | When set | Value |
|----------|----------|-------|
| `$SYMPOSIUM_DIR_<name>` | Installation has a symposium-managed cache (scoped cargo, github) | Absolute path to the cache / clone directory. |
| `$SYMPOSIUM_<name>` | Installation resolves to a runnable with an absolute path | Absolute path to the resolved executable / script. |
| `$PATH` | One or more dependencies contribute a runnable with an absolute path | Each runnable's parent dir is prepended, with the hook's `command` first. |

`<name>` is the installation name with non-alphanumeric characters replaced by `_` (e.g. `mytool-hooks` -> `SYMPOSIUM_DIR_mytool_hooks`). Both the hook's `command` installation and every requirement (recursively, one level via installation-level requirements) contribute.

Global cargo installs (`global = true`) don't set `$SYMPOSIUM_DIR_<name>` or augment `$PATH` — the binary is expected to already be on the user's `$PATH` via `~/.cargo/bin`.

> **`install_commands` runs before env vars are set.** The `$SYMPOSIUM_*` vars and the augmented `$PATH` are only available to the hook's spawned process. `install_commands` runs earlier, inside the symposium dispatch process, so it cannot reference its own (or any other) installation's env vars. Use absolute paths in `install_commands` instead.

### Supported hook events

| Hook event | Description | CLI usage |
|------------|-------------|-----------|
| `PreToolUse` | Before a tool (e.g., `Bash`) is invoked by the agent. | `pre-tool-use` |
| `PostToolUse` | After a tool completes. | `post-tool-use` |
| `UserPromptSubmit` | When the user submits a prompt. | `user-prompt-submit` |
| `SessionStart` | When an agent session starts. | `session-start` |
| `Stop` | When the agent's turn ends. | `stop` |

Not every agent delivers every event, and agents name them differently. See [Agent support](./hook-events.md#agent-support) for which events each agent delivers.

### Hook semantics

- **Exit codes**:
	- `0` — success: the hook's stdout is parsed as JSON and merged into the overall hook result.
	- `2` (or no reported exit code): block. The action is blocked, dispatch stops, and the hook's stderr is returned to the agent as the reason.
	- any other non-zero code — treated as success for dispatching purposes; stdout is still parsed and merged when possible.

- **Stdout handling**: Hooks should write a JSON object to stdout to contribute structured data back to the caller. Valid JSON objects are merged together across successful hooks; keys from later hooks overwrite earlier keys. The exception is `additionalContext` on `SessionStart`, `UserPromptSubmit` and `PostToolUse`: symposium's own context and every hook's are joined, in order, so none replaces another.

- **Stderr handling**: If a hook exits with code `2` (or no exit code), its stderr is returned to the agent as the reason the action was blocked. Otherwise stderr is captured but not returned.

### Testing hooks

To exercise a hook end to end, pipe a real agent payload into [`cargo agents hook <agent> <event>`](./cargo-agents-hook.md), using the event's CLI name from the table above. For Claude Code:

```bash
echo '{"session_id":"s","cwd":"/path/to/project","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"cargo test"}}' \
  | cargo agents hook claude pre-tool-use
```

This runs the full dispatch (including the automatic sync, when `auto-sync` is on) for the workspace at `cwd`, so the plugin must be installed and active there; the output is in the agent's own format. The payload must be the named agent's native JSON: you can also use `antigravity`, `copilot`, `codex`, or `kiro` as the agent name, each with that agent's payload. To test a symposium-format handler on its own, pipe canonical JSON straight into it (see [Testing](./hook-events.md#testing)).

## `[[predicate]]`

Each `[[predicate]]` entry defines a custom predicate function that can be used in `predicates` expressions anywhere a predicate is accepted. Custom predicates extend the built-in predicate language with plugin-specific checks.

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | The predicate name. Must be a valid identifier (`[a-zA-Z][a-zA-Z0-9_]*`) and must not collide with builtins (`depends-on`, `crate`, `shell`, `path_exists`, `env`, `workspace-member`, `not`, `any`, `all`). |
| `command` | string or table | The installation to run. Same shape as hook `command` (a string naming a `[[installations]]` entry or an inline table). |
| `args` | array of strings | Optional. Static arguments passed to the command before the dynamic argument. |

### How custom predicates work

Custom predicates are registered globally — a predicate defined in one plugin can be used by any other plugin's `predicates` expressions. Registration is unconditional: even if the defining plugin's own crate predicates don't match the current workspace, its `[[predicate]]` entries are still available.

When a predicate expression uses a function name that isn't a builtin, Symposium looks it up in the custom predicate registry. If found, it spawns the declared command with the static `args` followed by the raw argument text from the expression.

```toml
[[installations]]
name = "cargo-bp-install"
source = "cargo"
crate = "cargo-bp"
executable = "cargo-bp"

[[predicate]]
name = "battery_pack"
command = "cargo-bp-install"
args = ["bp", "status", "--check"]
```

Usage in a `predicates` expression:

```toml
predicates = ["battery_pack(cli>=0.3)"]
```

This evaluates as:

```
cargo-bp bp status --check cli>=0.3
```

Exit 0 means the predicate passes; non-zero means it fails.

The argument is trimmed of leading/trailing whitespace before being passed. An empty argument — `battery_pack()` or `battery_pack( )` — does not append anything to the command (only the static `args` are passed).

A custom predicate is a **boolean gate only**: it passes iff the command exits 0. Its stdout does not affect the result; it may carry watch events that control [caching](#caching).

### Collisions

If two plugins define the same predicate name, both definitions are skipped and a warning is emitted. Skills referencing the collided name evaluate as false.

### Caching

Symposium caches a custom predicate's result on disk, per workspace (under `~/.symposium/cache/predicates/`), keyed by the call as written (`battery_pack(cli>=0.3)`). Later syncs reuse the cached result without spawning the command until one of the inputs the predicate reported changes. Within one run, the same call is spawned at most once.

The predicate reports its inputs by printing watch events on stdout as JSON Lines, one object per line:

```text
{"watchFile":"/path/to/project/Config"}
{"watchEnv":"LAMBDA_ENV"}
{"watchTime":60000}
```

| Event | The cached result is discarded when |
|-------|-------------------------------------|
| `{"watchFile":"<path>"}` | The file's size or modification time changes, or the file appears or disappears. Prefer an absolute path: a relative one resolves against the directory Symposium runs in. |
| `{"watchEnv":"<NAME>"}` | The variable's value changes, or it becomes set or unset. |
| `{"watchTime":<milliseconds>}` | That much time has passed. The shortest `watchTime` wins, and `{"watchTime":0}` turns caching off for the call. |

The events from one run are combined, and a line that is not a known event is skipped with a warning. A predicate that exits non-zero is cached the same way as one that exits 0. A predicate that prints no events is cached forever, so report every input the result depends on, or print `{"watchTime":0}`. The cache is also discarded when a Symposium upgrade changes its format; delete `~/.symposium/cache/predicates/` to force every predicate to run again.

In Rust, the [`symposium-sdk`](https://crates.io/crates/symposium-sdk) crate prints these events for you: `symposium_sdk::env::var` and `symposium_sdk::fs::read_to_string` read a value and report it as an input, and `symposium_sdk::predicate::PredicateEmitter` writes events directly (`watch_file`, `watch_env`, `watch_time`).

## `[[mcp_servers]]`

Each `[[mcp_servers]]` entry declares an MCP server that Symposium registers into the agent's configuration during `cargo agents sync` (and the automatic sync that hooks run).

Symposium does not install MCP servers: unlike hooks and subcommands, they do not use `[[installations]]`. A stdio server's `command` is written into the agent's MCP configuration verbatim, so it must be an absolute path, or a command name the agent can find on the user's `PATH`.

There are multiple MCP transports:

### Stdio

```toml
[[mcp_servers]]
name = "my-server"
command = "/usr/local/bin/my-server"
args = ["--stdio"]
env = [{ name = "RUST_LOG", value = "info" }]
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Server name as it appears in the agent's MCP config. |
| `command` | string | yes | Absolute path to the server binary, or a command name on the user's `PATH`. Written into the agent's MCP config verbatim; Symposium does not install it. |
| `args` | array of strings | yes | Arguments passed to the binary. Use `[]` when there are none. |
| `env` | array of tables | yes | Environment variables to set when launching the server, each `{ name = "...", value = "..." }`. Use `[]` when there are none. |
| `depends-on` | string or array | no | Which crates this server applies to. ANDed with the plugin's gate. |
| `predicates` | array of strings | no | Predicates (`depends-on`, `shell`, `path_exists`, `env`, `workspace-member`, `not`, `any`, `all`) that must all hold for the server to register. See [Predicates](./predicates.md). |

Stdio entries do not need a `type` field.

### HTTP

```toml
[[mcp_servers]]
type = "http"
name = "my-server"
url = "http://localhost:8080/mcp"
headers = []
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | yes | Must be `"http"`. |
| `name` | string | yes | Server name as it appears in the agent's MCP config. |
| `url` | string | yes | HTTP endpoint URL. |
| `headers` | array of tables | yes | HTTP headers to set when making requests, each `{ name = "...", value = "..." }`. Use `[]` when there are none. |
| `depends-on` | string or array | no | Which crates this server applies to. ANDed with the plugin's gate. |
| `predicates` | array of strings | no | Predicates that must all hold for the server to register. See [Predicates](./predicates.md). |

### SSE

```toml
[[mcp_servers]]
type = "sse"
name = "my-server"
url = "http://localhost:8080/sse"
headers = []
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | yes | Must be `"sse"`. |
| `name` | string | yes | Server name as it appears in the agent's MCP config. |
| `url` | string | yes | SSE endpoint URL. |
| `headers` | array of tables | yes | HTTP headers to set when making requests, each `{ name = "...", value = "..." }`. Use `[]` when there are none. |
| `depends-on` | string or array | no | Which crates this server applies to. ANDed with the plugin's gate. |
| `predicates` | array of strings | no | Predicates that must all hold for the server to register. See [Predicates](./predicates.md). |

### How registration works

During `cargo agents sync`, each MCP server entry is written into each configured agent's config file in the format that agent expects. Registration is idempotent — existing entries with correct values are left untouched, stale entries are updated in place.

When a user runs `cargo agents sync` (or the hook triggers it automatically), Symposium:

1. Collects `[[mcp_servers]]` entries from all enabled plugins.
2. Writes each server into the agent's MCP configuration file.

All supported agents have MCP server configuration. Symposium handles the format differences — you declare the server once and it works across agents.

| Agent | Config location (project scope) | Key |
|-------|----------------|-----|
| Antigravity | `.agents/mcp_config.json` | `mcpServers.<name>` |
| Claude Code | `.mcp.json` | `mcpServers.<name>` |
| GitHub Copilot | `~/.copilot/mcp-config.json` (no project file) | `mcpServers.<name>` |
| Codex CLI | `~/.codex/config.toml` (no project file) | `[mcp_servers.<name>]` |
| Kiro | `.kiro/settings/mcp.json` | `mcpServers.<name>` |
| OpenCode | `opencode.json` | `mcp.<name>` |
| Goose | `~/.config/goose/config.yaml` (no project file) | `extensions.<name>` |

Servers are registered at the scope chosen by the `hook-scope` setting (see [Configuration](./configuration.md)): with `"project"`, in the files above; with `"global"` (the default), in each agent's user-level configuration. Agents without a project-level file always get the user-level one. See each agent's page under [Supported agents](./supported-agents.md) for the global locations.

## `[subcommand.<name>]`

A `[subcommand.<name>]` table adds a command that users and agents run as `cargo agents <name>`. Symposium resolves `command` through the same [installation](#installations) pipeline hooks use, runs it with the installation's `args` followed by the arguments typed after `<name>`, passes its output through, and exits with its exit code. The command owns its own arguments, validation, and `--help`.

```toml
# A recommendations entry; in a crate's own SYMPOSIUM.toml, leave out name and depends-on.
name = "widgetlib"
depends-on = ["widgetlib"]

[[installations]]
name = "widget-tool"
source = "cargo"
crate = "widget-tool"
executable = "widget-tool"
args = ["check"]

# `cargo agents widget-check --strict` runs `widget-tool check --strict`
[subcommand.widget-check]
description = "Check the widget definitions in this workspace"
command = "widget-tool"
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `description` | string | yes | Shown next to the command in `cargo agents --help`. At most 1024 bytes. |
| `audience` | string | no | `"agents"` (default) or `"humans"`. Only chooses the `cargo agents --help` section the command is listed under ("Commands for agents" or "Commands for humans"); it does not restrict who can run it. |
| `command` | string or table | yes | What to run. A string names an `[[installations]]` entry; a table is an inline installation (promoted to a synthetic entry named after the subcommand). A subcommand has no `executable`, `script`, or `args` fields of its own: set them on the installation. |
| `depends-on` | string or array | no | Which crates this subcommand applies to. ANDed with the plugin's gate. |
| `predicates` | array of strings | no | Predicates that must all hold for the subcommand to be available. See [Predicates](./predicates.md). |

`<name>` is the word typed after `cargo agents`. It may contain only ASCII letters, digits, `-`, and `_`, and it must not be the name of a built-in command: `init`, `sync`, `search`, `use`, `status`, `hook`, `plugin`, `self-update`, `crate-info`, `telemetry`, or `help`.

A subcommand is listed in `cargo agents --help`, and can be run, only while its plugin is active and its own `depends-on` and `predicates` hold. If two active plugins define the same name, running it fails with an error naming both plugins. A subcommand's gate does not keep its plugin out of [dormancy](#top-level-fields).

## Example: full manifest

```toml
# A recommendations entry; in a crate's own SYMPOSIUM.toml, leave out name and depends-on.
name = "widgetlib"
depends-on = ["widgetlib"]

# Skills shipped inside the widgetlib crate source (in skills/)
[[plugins]]
source.cargo = "widgetlib"

# Additional skills hosted in a git repo (the recommendations repository does not accept git sources)
[[skills]]
depends-on = ["widgetlib^1.0"]
source.git = "https://github.com/org/widgetlib/tree/main/symposium/skills"

[[hooks]]
name = "check-widget-usage"
event = "PreToolUse"
matcher = "^(Bash|bash|run_command|execute_bash)$"
command = { script = "scripts/check-widget.sh" }

[[mcp_servers]]
name = "widgetlib-mcp"
command = "/usr/local/bin/widgetlib-mcp"
args = ["--stdio"]
env = []
```

## Validation

```bash
cargo agents plugin validate path/to/SYMPOSIUM.toml
```

This parses the manifest and reports any errors. The file must be named `SYMPOSIUM.toml` (in capitals) for Symposium to discover it; to validate every entry in a registry directory, pass the directory instead (see [Plugin sources](./plugin-source.md#validation)). Crate name checking against crates.io is on by default; use `--no-check-crates` to skip it.
