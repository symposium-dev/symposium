# Important flows

This section describes the logic of each `cargo agents` command.

## Crate-sourced skill resolution

A plugin loads a crate as a plugin by naming that crate in a `[[plugins]]` chained reference (`source.cargo = "..."`); the user can also load one directly by enabling the dependency it lives in (see [enablement](#dependency-enablement) below). When the owning plugin is active and the edge's predicates hold, the crate is resolved into the **active plugin set** — the shared list every facet (skills, MCP servers, hooks, subcommands) resolves over, so a crate-sourced plugin's extensions dispatch exactly like a registry plugin's. A single path handles every crate — a crate is always a first-class plugin, whether it describes itself with a `SYMPOSIUM.toml`, with `[package.metadata.symposium]`, with both, or with neither:

1. `skills::active_plugins` seeds a worklist from the trust-root plugins the registry loaded: each active plugin's `plugin.chained` edges whose predicates hold (evaluated against the *owning* plugin's provenance) contribute a `source.cargo` crate id.
2. For each id the fixed-point calls `pms.load_plugin(id)` on the **package-manager set** `active_plugins` was handed (built once by `package_managers(deps)`). The id's `pm` routes it to the cargo transport, which:
   - `CargoPm::fetch` resolves the source via `RustCrateFetch` (path overrides for local path deps, then the cargo registry cache, then crates.io) with `UpdateLevel::None` — cache-only, so this is safe on the per-event hook path. The fetched id carries the exact resolved version.
   - `plugins::load_crate_manifest` builds the plugin definition by layering three sources (merge order: crate defaults → `[package.metadata.symposium]` from `Cargo.toml` → `SYMPOSIUM.toml` file). Both manifest sources use the ordinary plugin-manifest schema and are parsed **leniently** (a malformed layer is logged and dropped). Validation runs under `ManifestOrigin::Crate` (name defaults to the crate, `depends-on` is waived, `[defaults]` accepted, default `skills/` group appended unless `[defaults] skills = false`). The result is a `ParsedPlugin` whose `canonical` id is the resolved crate. A crate with no manifest sources still yields one whose only content is that default `skills/` group.
3. `record_active` honors the crate plugin's own plugin-level predicates (`applies`, which stamps its provenance — never a workspace member), appends it to the active set, and **enqueues its own `[[plugins]]` edges**. This is how a `[package.metadata.symposium]` redirect (now a `[[plugins]] source.cargo` chained reference to the target crate) is followed. A `visited` set keyed on `(pm, normalized name)` — global across the whole `active_plugins` call — collapses diamonds (a crate reached through two plugins loads once, so its hooks don't double-fire and its subcommands don't read as a false conflict) and breaks cycles; the finite crate universe bounds termination.
4. Facet extraction then walks the active set. `collect_skills` runs each plugin's skill groups through the ordinary `load_skills_for_group` pipeline — honoring named groups, group predicates, and `source.path`/`source.git`, with each discovered skill's origin hashed from its on-disk `SKILL.md` path (this is where git skill sources are fetched, hence the `update` level). MCP-server filtering (`sync`), hook dispatch (`hook::dispatch_plugin_hooks`), and subcommand lookup (`subcommand_dispatch`) each iterate the same set. A crate plugin's **custom predicate definitions** are the one facet still not wired in — they resolve only from configured registries, and `warn_undispatched_crate_features` notes when a crate declares one.

A skill's install identity is the hash of its on-disk `SKILL.md` path, so a crate reached two ways dedupes to one install. The edge's version requirement is recorded but not yet enforced — the crate resolves against the workspace (pin / path override).

The key code paths are in `pm/cargo/mod.rs` (`CargoPm::load_plugin`, `build_from_fetched`), `plugins.rs` (`load_crate_manifest`, `RawPluginManifest::merge`, `ManifestOrigin::Crate`, `ParsedPlugin::canonical`), `skills.rs` (`active_plugins`, `record_active`, `plugin_key`, `collect_skills`, `hash_origin_key`), `crate_metadata.rs` (`symposium_metadata`), `pm/cargo/workspace.rs` (`WorkspaceDeps`, `WorkspaceCrate`), and `crate_sources/mod.rs` (`RustCrateFetch`).

## Dependency enablement

A dependency's own plugin content — a `SYMPOSIUM.toml`, `[package.metadata.symposium]`, or a `skills/` directory — is reachable without any manifest pointing at it, but only with the user's consent: dependencies are not a trust root.

1. `discovery::discover` asks the **untrusted** cargo transport for its `active_plugins(dep_ids)`: the plugins embedded in the workspace's dependencies. `CargoPm::active_plugins` fetches each dependency cache-only and inspects it — a workspace dep resolves into the source `cargo metadata` already extracted (`WorkspaceCrate::source_dir`), no probe/network — so registry-dep embedded plugins are discoverable too. The trusted registries (including the recommendations repo) are skipped, because their plugins are trust roots and never need consent. Each candidate is classified against `[plugins]` on its crate name — enabled by `use`, auto-enabled, declined, or an undecided candidate. Nothing is prompted or written.
2. At sync time, `skills::active_plugins` asks `discovery::enabled_dependencies` which crate names `[plugins] auto-enable` or an applicable `use` entry covers — workspace deps, plus `use`d crates that aren't deps at all — and seeds each as a cargo id on the same worklist a chained reference feeds, so `pms.load_plugin` honors the crate's manifest sources, skill groups, and its own `[[plugins]]` edges. This reads config rather than the offer list, so `cargo agents use <crate>` loads a crate from crates.io whether or not the workspace depends on it, and even before its source has been fetched. (`CargoPm::search` is what lets `use` name such a crate; a name a configured registry already provides is skipped here so it isn't double-loaded.)
3. Independently, a registry plugin with no dependency gate anywhere loads *dormant* (`Plugin::requires_use`) and activates only when a `use` entry names it. The gate rides the `PredicateContext` (`with_used_names` / `is_used`), so skill resolution, hook dispatch, subcommand lookup, help, and MCP filtering all agree.

The consent prompt and the `use` / `search` / `status` commands that record decisions are not implemented yet — today the `[plugins]` config is edited by hand.

The key code paths are in `discovery.rs`, `config.rs` (`PluginsConfig`, `UseEntry`), `pm/cargo/mod.rs` (`active_plugins`, `load_plugin`), `plugins.rs` (`Plugin::requires_use`), `predicate.rs` (`PredicateContext::is_used`), and `skills.rs` (`active_plugins`, `record_active`).

## Compilation and delivery of agent plugin directories

Every `cargo agents sync` compiles the plugins that apply into the unit agents consume, then hands each directory to the agents that can take it. It runs after skills are resolved, so it never re-evaluates a gate. Outside a Rust workspace only the global half happens.

1. `agent_plugin::compile` groups applicable skills by their plugin's `canonical` id and builds one `CompiledPlugin` each: a slugged name, a version, the description, and one entry per distinct skill origin. Directory names and skill names are disambiguated with the same origin-hash suffix rule that governs skill installs.
2. `Scope::of` sends each to `<project root>/.symposium/plugins/` or `<config dir>/installed/` — see [key modules](./module-structure.md#agent_plugin--compiling-an-agent-plugin-directory) for what global requires and why. A scope no configured agent can take is not compiled.
3. `agent_plugin::write` stages into a tempdir and syncs it in through `sync::sync_managed_dir`, so an unchanged plugin is not recopied. `write_marketplace` writes `.claude-plugin/marketplace.json` at each staging root, and removes it when the root empties.
4. For each configured agent and each scope it accepts, `Agent::install_plugins` writes that agent's configuration and, where the agent loads only from its own tree, copies the directory there. Plugins an agent received are recorded, so their skills are skipped in the per-skill loop and nothing arrives twice.
5. `agent_plugin::reap_to_depth` removes marked directories this sync did not write, under both staging roots and every known agent's plugin tree. Reaping the global root from a project sync is sound only because step 2 keeps the global set a function of user config alone. Steps 4 and 5 are skipped entirely when a trusted source was unreadable, so a transient registry failure cannot be read as an uninstall.

The key code paths are in `agent_plugin/mod.rs`, `agent_plugin/manifest.rs`, `agents/plugin_install.rs`, `predicate.rs` (`is_workspace_independent`), and `sync.rs`.

## Reading an externally authored package

A directory holding a `plugin.json` loads as an ordinary symposium plugin, so compilation, delivery and `status` treat it like any other.

1. `pm::layout::classify` returns `EntryKind::AgentPlugin`. Precedence runs `SYMPOSIUM.toml`, `plugin.json`, `SKILL.md`. A claimed directory is not descended into, so a package cannot nest another; a source root that is itself a package is an error.
2. `agent_plugin::read::load` parses the manifest, reports unknown fields and an unsupported `mcp.json`, reads the gate from `extensions["dev.symposium"]`, and returns a `Plugin` with one `skills/` group limited to immediate children.
3. The three positions call it: `plugins::load_entry` (registry, dormancy applies), `workspace_plugin_for_dir` (member), and `CargoPm::build_from_fetched` (dependency) — the latter two gated by position. `embedded_plugin_kind` counts a `plugin.json`, so a dependency carrying one is offered for consent.
4. Containment is per unit: a bad manifest rejects that package alone, an unknown field is ignored, a broken skill is skipped, and a skill resolving outside the package is refused.

The key code paths are in `agent_plugin/read.rs`, `pm/layout.rs`, `plugins.rs` (`load_entry`, `workspace_plugin_for_dir`, `apply_sibling_identity`, `dormant_without_gate`), and `skills.rs` (`discover_skills`, `SkillDepth`).

## Help rendering

`cargo agents --help` (and `-h`, the bare `help` keyword, or no subcommand) is rendered by `help_render`, not by clap's default help.

1. The binary and the test harness parse argv with `Cli::try_parse_from`, then call `help_render::help_text(parse, args, sym, cwd)`. Because the decision happens after parsing, argument order (`--help --quiet`) does not matter and there is no second argv parser to keep in sync.
2. For no subcommand, `--help`/`-h`, or the bare `help` keyword, `help_text` returns the top-level grouped help: `render` slices clap's own rendered help (header + options block) and hand-renders "Commands for humans" / "Commands for agents" between them, mixing built-ins (`cli::builtin_audience`) with workspace-filtered plugin subcommands (`subcommand_dispatch::applicable_subcommands`).
3. For `<built-in> --help`, `help_text` re-renders clap's per-command help by walking clap's command tree to the named subcommand — so required-arg commands (`crate-info`), required-subcommand groups (`plugin`), and nested commands (`plugin list`) all work even though clap's auto help flag is disabled.
4. A plugin-vended `<name> --help` is left alone: `help_text` returns `None`, and dispatch forwards `--help` to the child binary, which owns its own help.

clap's auto help flag and help subcommand are disabled in `cli::Cli`; `--help`/`-h` is a manual `global` bool. The key code paths are in `help_render.rs` (`help_text`, `render`, `subcommand_help`), `cli.rs` (`builtin_audience`, the `Cli` flags), and `bin/cargo-agents.rs` plus `symposium-testlib` (the parse-then-`help_text` wiring).

## Subcommand dispatch

When the user runs `cargo agents <name>` for a name not built into the binary, clap's `allow_external_subcommands` routes it to `Commands::External(argv)`.

1. The binary (or library `cli::run`) calls `subcommand_dispatch::dispatch_external(sym, cwd, argv)`, which first resolves the **active plugin set** (`skills::active_plugins` — registry plugins plus crate-sourced ones) so a crate's subcommands are dispatchable too.
2. `find_subcommand` walks that set. For each plugin it applies the plugin-level `depends-on` predicate against the workspace, then looks up `argv[0]` in `plugin.subcommands`. If the entry has its own `depends-on` predicate, that must also match. Two or more matches → error.
3. The matched subcommand's `command` field names an `Installation` on the same plugin. `installation::resolve_runnable` acquires the source if any, runs `install_commands`, and picks the `Runnable` (`Exec` for binaries, `Script` for shell scripts).
4. The child is spawned with stdio inherited. Its exit code is collapsed to a `u8` — the binary wraps it in `ExitCode::from`; the library treats non-zero as an error so the test harness can assert on success/failure.

The key code paths are in `subcommand_dispatch.rs`, `cli.rs` (the `External` arm), and `bin/cargo-agents.rs` (binary-side wrapping that surfaces the numeric exit code to the OS).
