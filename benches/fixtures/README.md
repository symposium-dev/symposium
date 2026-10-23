# Benchmark fixtures

These checked-in fixtures are deterministic workloads composed by the benchmark targets. Support code copies them into an isolated sandbox and validates their invariants before starting a timed operation.

## `config`

`config` contains production-valid Symposium configurations selected by the
hook-dispatch scenarios. Both configurations disable the builtin registries.
The local-registry configuration adds the staged `local-registry` fixture by a
path relative to the sandbox's `symposium-home` configuration directory, where
the harness writes the selected file.

## `reference-project`

`reference-project` is a virtual Cargo workspace used by the workspace-dependency and hook-dispatch benchmarks. It has these invariants:

- its workspace members are exactly `cli` and `server`;
- its local path dependencies are exactly `domain`, `terminal`, and `storage`;
- `domain` is a direct dependency of both members, `terminal` belongs only to `cli`, and `storage` belongs only to `server`;
- `Cargo.lock` is committed and `.cargo/config.toml` enables offline mode;
- neither the workspace root nor a member defines a workspace plugin through `SYMPOSIUM.toml`, `skills/`, or `.agents/skills/`.

The three dependency packages contain empty `[workspace]` tables so Cargo does not associate them with Symposium's outer workspace.

## `local-registry`

`local-registry` is a path registry used by the representative hook-dispatch benchmark. It has exactly three manifest-backed entries:

- `always-active` uses `depends-on = ["*"]`;
- `predicate-gated` is active but its `PreToolUse` hook is disabled by `path_exists(./.symposium-benchmark-never-present)`;
- `dormant` has no activation gate.

Every entry contains `SYMPOSIUM.toml`. The predicate-gated entry's
`unexpected-hook.sh` is required fixture data, so the disabled hook always has
a valid command behind it. The registry contains no bare `SKILL.md` entry. This
keeps `src/skills.rs` outside the measured hook path.

Before measuring, the harness must verify the project graph, assert that `.symposium-benchmark-never-present` is absent from the benchmark process's current working directory, and require the loaded plugin names to be exactly `always-active`, `predicate-gated`, and `dormant`. Cargo sets that working directory to the `benches/benchsuite` package root. A missing or malformed entry is a setup failure, never a faster sample.
