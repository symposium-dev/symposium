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

## Subcommand dispatch

When the user runs `cargo agents <name>` for a name not built into the binary, clap's `allow_external_subcommands` routes it to `Commands::External(argv)`.

1. The binary (or library `cli::run`) calls `subcommand_dispatch::dispatch_external(sym, cwd, argv)`.
2. `find_subcommand` walks the plugin registry. For each plugin it applies the plugin-level `crates` predicate against the workspace, then looks up `argv[0]` in `plugin.subcommands`. If the entry has its own `crates` predicate, that must also match. Two or more matches → error.
3. The matched subcommand's `command` field names an `Installation` on the same plugin. `installation::resolve_runnable` acquires the source if any, runs `install_commands`, and picks the `Runnable` (`Exec` for binaries, `Script` for shell scripts).
4. The child is spawned with stdio inherited. Its exit code is collapsed to a `u8` — the binary wraps it in `ExitCode::from`; the library treats non-zero as an error so the test harness can assert on success/failure.

The key code paths are in `subcommand_dispatch.rs`, `cli.rs` (the `External` arm), and `bin/cargo-agents.rs` (binary-side wrapping that surfaces the numeric exit code to the OS).
