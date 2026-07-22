# Important flows

This section describes the logic of each `cargo agents` command.

## Crate-sourced skill resolution

A plugin loads a crate as a plugin by naming that crate in a `[[plugins]]` chained reference (`source.cargo = "..."`). When the owning plugin is active and the edge's predicates hold, sync resolves the crate. A single path handles every crate — a crate is always a first-class plugin, whether it describes itself with a `SYMPOSIUM.toml`, with `[package.metadata.symposium]`, with both, or with neither:

1. `skills_applicable_to` runs `expand_chained_plugins` over the active plugin's `plugin.chained` edges; each edge whose predicates hold (evaluated against the *owning* plugin's provenance) names a crate directly.
2. For that crate, `expand_chained_plugins` calls `CargoPm::load_plugin(name, workspace)`:
   - `CargoPm::fetch` resolves the source via `RustCrateFetch` (path overrides for local path deps, then the cargo registry cache, then crates.io). The fetched id carries the exact resolved version.
   - `plugins::load_crate_manifest` builds the plugin definition by layering three sources (merge order: crate defaults → `[package.metadata.symposium]` from `Cargo.toml` → `SYMPOSIUM.toml` file). Both manifest sources use the ordinary plugin-manifest schema and are parsed **leniently** (a malformed layer is logged and dropped). Validation runs under `ManifestOrigin::Crate` (name defaults to the crate, `depends-on` is waived, `[defaults]` accepted, default `skills/` group appended unless `[defaults] skills = false`). The result is a `ParsedPlugin` whose `canonical` id is the resolved crate. A crate with no manifest sources still yields one whose only content is that default `skills/` group.
3. Back in `expand_chained_plugins`, the crate plugin's own plugin-level predicates are honored (`applies`, which stamps its provenance — never a workspace member), its skill groups run through the ordinary `load_skills_for_group` pipeline — honoring named groups, group predicates, and `source.path`/`source.git`, with each discovered skill's origin hashed from its on-disk `SKILL.md` path — and **its own `[[plugins]]` edges are expanded in turn**. This is how a `[package.metadata.symposium]` redirect (now a `[[plugins]] source.cargo` chained reference to the target crate) is followed. A per-top-level-plugin `visited` set keyed on the normalized crate name collapses diamonds (a crate reached two ways loads once) and breaks cycles; `MAX_CHAIN_DEPTH` (10) is a backstop. The crate plugin's hooks/MCP/subcommands are parsed but not yet dispatched (a `warn_undispatched_crate_features` notice fires when present).

A skill's install identity is the hash of its on-disk `SKILL.md` path, so a crate reached two ways dedupes to one install. The edge's version requirement is recorded but not yet enforced — the crate resolves against the workspace (pin / path override).

The key code paths are in `pm/cargo.rs` (`CargoPm::load_plugin`), `plugins.rs` (`load_crate_manifest`, `RawPluginManifest::merge`, `ManifestOrigin::Crate`, `ParsedPlugin::canonical`), `skills.rs` (`expand_chained_plugins`, `hash_origin_key`), `crate_metadata.rs` (`symposium_metadata`), and `crate_sources/mod.rs` (`RustCrateFetch`, `WorkspaceCrate`).

## Help rendering

`cargo agents --help` (and `-h`, the bare `help` keyword, or no subcommand) is rendered by `help_render`, not by clap's default help.

1. The binary and the test harness parse argv with `Cli::try_parse_from`, then call `help_render::help_text(parse, args, sym, cwd)`. Because the decision happens after parsing, argument order (`--help --quiet`) does not matter and there is no second argv parser to keep in sync.
2. For no subcommand, `--help`/`-h`, or the bare `help` keyword, `help_text` returns the top-level grouped help: `render` slices clap's own rendered help (header + options block) and hand-renders "Commands for humans" / "Commands for agents" between them, mixing built-ins (`cli::builtin_audience`) with workspace-filtered plugin subcommands (`subcommand_dispatch::applicable_subcommands`).
3. For `<built-in> --help`, `help_text` re-renders clap's per-command help by walking clap's command tree to the named subcommand — so required-arg commands (`crate-info`), required-subcommand groups (`plugin`), and nested commands (`plugin list`) all work even though clap's auto help flag is disabled.
4. A plugin-vended `<name> --help` is left alone: `help_text` returns `None`, and dispatch forwards `--help` to the child binary, which owns its own help.

clap's auto help flag and help subcommand are disabled in `cli::Cli`; `--help`/`-h` is a manual `global` bool. The key code paths are in `help_render.rs` (`help_text`, `render`, `subcommand_help`), `cli.rs` (`builtin_audience`, the `Cli` flags), and `bin/cargo-agents.rs` plus `symposium-testlib` (the parse-then-`help_text` wiring).

## Subcommand dispatch

When the user runs `cargo agents <name>` for a name not built into the binary, clap's `allow_external_subcommands` routes it to `Commands::External(argv)`.

1. The binary (or library `cli::run`) calls `subcommand_dispatch::dispatch_external(sym, cwd, argv)`.
2. `find_subcommand` walks the plugin registry. For each plugin it applies the plugin-level `depends-on` predicate against the workspace, then looks up `argv[0]` in `plugin.subcommands`. If the entry has its own `depends-on` predicate, that must also match. Two or more matches → error.
3. The matched subcommand's `command` field names an `Installation` on the same plugin. `installation::resolve_runnable` acquires the source if any, runs `install_commands`, and picks the `Runnable` (`Exec` for binaries, `Script` for shell scripts).
4. The child is spawned with stdio inherited. Its exit code is collapsed to a `u8` — the binary wraps it in `ExitCode::from`; the library treats non-zero as an error so the test harness can assert on success/failure.

The key code paths are in `subcommand_dispatch.rs`, `cli.rs` (the `External` arm), and `bin/cargo-agents.rs` (binary-side wrapping that surfaces the numeric exit code to the OS).
