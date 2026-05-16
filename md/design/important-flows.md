# Important flows

This section describes the logic of each `cargo agents` command.

## Crate-sourced skill resolution

When a skill group uses `source = "crate"` or `source.crate_path`, the sync flow takes an additional path:

1. `predicate::union_matched_crates()` resolves plugin-level and group-level predicates against the workspace to produce a set of concrete crate name/version pairs.
2. For each crate in the set, `RustCrateFetch` fetches the source — checking path overrides (for local path deps), then the cargo registry cache, then crates.io.
3. `discover_skills()` scans the specified subdirectory within each fetched crate source.

The key code paths are in `skills.rs` (`load_skills_for_group`), `predicate.rs` (`matched_crates`, `union_matched_crates`), and `crate_sources/mod.rs` (`RustCrateFetch`, `WorkspaceCrate`).

## Subcommand dispatch

When the user runs `cargo agents <name>` for a name not built into the binary, clap's `allow_external_subcommands` routes it to `Commands::External(argv)`.

1. The binary (or library `cli::run`) calls `subcommand_dispatch::dispatch_external(sym, cwd, argv)`.
2. `find_subcommand` walks the plugin registry. For each plugin it applies the plugin-level `crates` predicate against the workspace, then looks up `argv[0]` in `plugin.subcommands`. If the entry has its own `crates` predicate, that must also match. Two or more matches → error.
3. The matched subcommand's `command` field names an `Installation` on the same plugin. `installation::resolve_runnable` acquires the source if any, runs `install_commands`, and picks the `Runnable` (`Exec` for binaries, `Script` for shell scripts).
4. The child is spawned with stdio inherited. Its exit code is collapsed to a `u8` — the binary wraps it in `ExitCode::from`; the library treats non-zero as an error so the test harness can assert on success/failure.

The key code paths are in `subcommand_dispatch.rs`, `cli.rs` (the `External` arm), and `bin/cargo-agents.rs` (binary-side wrapping that surfaces the numeric exit code to the OS).
