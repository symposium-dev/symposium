# Important flows

This section describes the logic of each `cargo agents` command.

## Crate-sourced skill resolution

When a skill group uses `source = "crate"`, the sync flow takes an additional path:

1. `predicate::union_matched_crates()` resolves plugin-level and group-level predicates against the workspace to produce a set of concrete crate name/version pairs.
2. For each crate in the set, `RustCrateFetch` fetches the source — checking path overrides (for local path deps), then the cargo registry cache, then crates.io.
3. `crate_metadata::parse_crate_metadata()` reads `[package.metadata.symposium]` from the crate's `Cargo.toml`:
   - **No metadata** — fall back to the default `skills/` subdirectory.
   - **`skills = []`** — no skills from this crate.
   - **`path = "..."` entries** — scan that subdirectory for skills.
   - **`crate = { name, version? }` entries** — redirect: fetch the target crate and follow its metadata recursively (with cycle detection and a depth limit of 10).
4. `discover_skills()` scans each resolved directory for `SKILL.md` files.

The key code paths are in `skills.rs` (`load_crate_skills`, `fetch_and_resolve_skills`), `crate_metadata.rs` (`parse_crate_metadata`), `predicate.rs` (`matched_crates`, `union_matched_crates`), and `crate_sources/mod.rs` (`RustCrateFetch`, `WorkspaceCrate`).

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
2. `find_subcommand` walks the plugin registry. For each plugin it applies the plugin-level `crates` predicate against the workspace, then looks up `argv[0]` in `plugin.subcommands`. If the entry has its own `crates` predicate, that must also match. Two or more matches → error.
3. The matched subcommand's `command` field names an `Installation` on the same plugin. `installation::resolve_runnable` acquires the source if any, runs `install_commands`, and picks the `Runnable` (`Exec` for binaries, `Script` for shell scripts).
4. The child is spawned with stdio inherited. Its exit code is collapsed to a `u8` — the binary wraps it in `ExitCode::from`; the library treats non-zero as an error so the test harness can assert on success/failure.

The key code paths are in `subcommand_dispatch.rs`, `cli.rs` (the `External` arm), and `bin/cargo-agents.rs` (binary-side wrapping that surfaces the numeric exit code to the OS).
