# Important flows

This section describes the logic of each `cargo agents` command.

## Plugin source resolution

When Symposium loads plugin sources during sync, it follows this process:

1. **Load installed sources** — read `[installed.crates]`, `installed.paths`, and `installed.git` entries from user config. Crate-registry entries resolve through Cargo dependency syntax; direct path and git entries use their own registries.
2. **Resolve discovery policy** — collect `[discovery.allow]` / `[discovery.deny]` rules from config and installed plugins. Check workspace deps and other candidates against the combined policy; resolve matching sources.
3. **Resolve transitive plugin sources** — follow `[[plugins]] source.*` entries recursively, evaluating `where.*` filters. Path declarations participate in source-root discovery; git and crate declarations are resolved through the source graph.
4. **Discover plugins within each source root** — load `$ROOT/SYMPOSIUM.toml`, or synthesize an empty root manifest when it is absent. Every manifest defaults to `where.crates = ["*"]`, implicit `skills/` and workspace-gated `.agents/skills/` skill groups, and implicit nested-manifest search through `[[plugins]] source.path = "."`. Nested manifests are independent plugins and dedupe by canonical manifest path.
5. **Resolve implicit installations** — read crate `Cargo.toml` binary targets and register them as available installations for `SYMPOSIUM.toml` to reference.

The key code paths are in `crate_sources/mod.rs` (`RustCrateFetch`), `plugins.rs` (discovery walk), and `skills.rs` (skill resolution).

## Help rendering

`cargo agents --help` (and `-h`, the bare `help` keyword, or no subcommand) is rendered by `help_render`, not by clap's default help.

1. The binary and the test harness parse argv with `Cli::try_parse_from`, then call `help_render::help_text(parse, args, sym, cwd)`. Because the decision happens after parsing, argument order (`--help --quiet`) does not matter and there is no second argv parser to keep in sync.
2. For no subcommand, `--help`/`-h`, or the bare `help` keyword, `help_text` returns the top-level grouped help: `render` slices clap's own rendered help (header + options block) and hand-renders "Commands for humans" / "Commands for agents" between them, mixing built-ins (`cli::builtin_audience`) with workspace-filtered plugin subcommands (`subcommand_dispatch::applicable_subcommands`).
3. For `<built-in> --help`, `help_text` re-renders clap's per-command help by walking clap's command tree to the named subcommand — so required-arg commands (`crate-info`), required-subcommand groups (`plugin`), and nested commands (`plugin validate`) all work even though clap's auto help flag is disabled.
4. A plugin-vended `<name> --help` is left alone: `help_text` returns `None`, and dispatch forwards `--help` to the child binary, which owns its own help.

clap's auto help flag and help subcommand are disabled in `cli::Cli`; `--help`/`-h` is a manual `global` bool. The key code paths are in `help_render.rs` (`help_text`, `render`, `subcommand_help`), `cli.rs` (`builtin_audience`, the `Cli` flags), and `bin/cargo-agents.rs` plus `symposium-testlib` (the parse-then-`help_text` wiring).

## Subcommand dispatch

When the user runs `cargo agents <name>` for a name not built into the binary, clap's `allow_external_subcommands` routes it to `Commands::External(argv)`.

1. The binary (or library `cli::run`) calls `subcommand_dispatch::dispatch_external(sym, cwd, argv)`.
2. `find_subcommand` walks the plugin registry. For each plugin it applies the plugin-level `crates` predicate against the workspace, then looks up `argv[0]` in `plugin.subcommands`. If the entry has its own `crates` predicate, that must also match. Two or more matches → error.
3. The matched subcommand's `command` field names an `Installation` on the same plugin (or an implicit binary target from the crate). `installation::resolve_runnable` acquires the source if any, runs `install_commands`, and picks the `Runnable` (`Exec` for binaries, `Script` for shell scripts).
4. The child is spawned with stdio inherited. Its exit code is collapsed to a `u8` — the binary wraps it in `ExitCode::from`; the library treats non-zero as an error so the test harness can assert on success/failure.

The key code paths are in `subcommand_dispatch.rs`, `cli.rs` (the `External` arm), and `bin/cargo-agents.rs` (binary-side wrapping that surfaces the numeric exit code to the OS).
