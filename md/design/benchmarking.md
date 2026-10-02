# Benchmarking

The performance question for the initial benchmark suite is:

> How much wall-clock time does Symposium's in-process `PreToolUse` pipeline take in an unchanged workspace, with minimal and representative local configurations?

The first performance story answers that question with two in-process hook cases and two `WorkspaceDeps` component measurements. The component numbers make the hook results interpretable: they show the difference between resolving metadata and reusing Symposium's disk cache.

The suite begins with this bounded story rather than attempting to benchmark every performance-sensitive path in the same change.

## Goals

The benchmark suite is organized so that:

- each benchmark addition can be built, run, and understood independently;
- the primary measurements represent the in-process portion of a user-visible operation;
- component measurements explain important contributors to that operation;
- fixtures and environment setup can be shared without hiding the operation being measured;
- workloads are deterministic and cannot access the network;
- benchmark names state the cache or workload conditions they control;
- normal pull requests compile the suite, while measurements run separately;
- performance gating is introduced only after a benchmark has a stable and useful history.

The first story does not measure workspace-size scaling, `SessionStart`, real plugin hook subprocesses, remote registry refresh, networking, or every performance-sensitive module. Those require later, separately justified benchmark additions.

## Organization

The suite uses this layout:

```text
benches/
|-- README.md
|-- benchsuite/
|   |-- Cargo.toml
|   |-- src/
|   |   |-- cargo.rs
|   |   |-- fixture.rs
|   |   |-- lib.rs
|   |   `-- sandbox.rs
|   `-- benches/
|       |-- hook_dispatch.rs
|       `-- workspace_deps.rs
`-- fixtures/
    |-- README.md
    |-- reference-project/
    |   |-- .cargo/
    |   |   `-- config.toml
    |   |-- Cargo.toml
    |   |-- Cargo.lock
    |   |-- cli/
    |   |-- server/
    |   |-- domain/
    |   |-- terminal/
    |   `-- storage/
    `-- local-registry/
        |-- always-active/
        |   `-- SYMPOSIUM.toml
        |-- predicate-gated/
        |   |-- SYMPOSIUM.toml
        |   `-- unexpected-hook.sh
        `-- dormant/
            `-- SYMPOSIUM.toml
```

`benchsuite` is a non-publishable package (`publish = false`) listed explicitly in the root workspace's `members`. Its library owns reusable mechanics: locating and copying checked-in fixtures, creating isolated configuration and cache directories, validating prepared workloads, and constructing the metadata-rejecting Cargo guard used by untimed cache-hit preflights. Fixture metadata is centralized in private typed specifications so its directory, required files, and expected workspace shape cannot drift across separate declarations. The library exports only the fixture, staged-fixture, sandbox, and Cargo-guard capabilities needed by benchmark targets. Individual targets retain semantic ownership of their scenarios and timed operations.

Each Criterion target is declared explicitly in the benchsuite manifest with `harness = false`. Shared support code does not wrap Criterion or define a universal benchmark framework. A target exposes Criterion's concepts directly so its measurement choices remain visible.

`benches/fixtures` is separate from the runner package so future benchmark targets and performance tools can reuse its workloads.

The root package sets `autobenches = false`, preventing Cargo from interpreting future paths under the top-level `benches` directory as benchmark targets of the `symposium` package. It also excludes `/benches` from the published package.
`cargo package --list` verifies that benchmark-only files are absent from the crate archive.

The fixture manifest is a virtual workspace containing `cli` and `server`. The `domain`, `terminal`, and `storage` path dependencies are excluded from that workspace so Cargo metadata sees them as dependencies rather than members. Each dependency manifest contains an empty `[workspace]` marker, preventing Cargo from associating it with Symposium's outer workspace. The fixture therefore does not require entries in the root workspace's `exclude` list.

## Benchmark contract

Every benchmark target begins with a doc comment containing these fields:


| Field           | Meaning                                                          |
| --------------- | ---------------------------------------------------------------- |
| Claim           | The performance property the benchmark is intended to represent. |
| Workload        | The fixture and inputs supplied to the code.                     |
| Timed operation | The exact operation included in the measurement.                 |
| Excluded setup  | Preparation deliberately kept outside the timer.                 |
| Invariants      | Conditions checked to ensure the intended path is exercised.     |
| Metric          | The quantity reported and its unit.                              |
| Noise           | Known uncontrolled effects and interpretation limits.            |
| Lifecycle       | `experimental`, `observed`, or `gated`.                          |


The target's doc comment is the single source of truth because it is next to the code that can invalidate the contract. `benches/README.md` is an index of targets, commands, lifecycle states, and links to those contracts; it does not duplicate all eight fields.

## Framework

The initial suite uses [Criterion.rs](https://criterion-rs.github.io/book/). The performance story includes filesystem access and Cargo subprocesses, so wall-clock measurement and statistical sampling are appropriate. A callgrind-based instruction counter would not represent the latency of those external operations. It may still be useful for a later CPU-bound benchmark.

The implementation uses `std::hint::black_box` for both values passed from Criterion setup into a timed closure and results returned by the timed operation. Fixture preparation, cache-state construction, runtime construction, and correctness assertions remain outside the timer.

The initial `WorkspaceDeps` cases use Criterion's minimum of ten samples and a 15-second measurement target. These subprocess-bound cases can take more than a second per iteration and vary substantially across machines, so the suite bounds individual runs instead of continually increasing the target time. Lifecycle stability is evaluated from repeated runs in the pinned benchmark environment rather than from a larger local sample count.

## Hook cases: unchanged workspace

The hook target contains two cases:


| Case                                        | Configuration                                                            | Interpretation                                                                                                                      |
| ------------------------------------------- | ------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------- |
| `hook_dispatch/pre_tool_use_minimal_config` | Both builtin registries disabled and no configured plugins               | The fixed in-process pipeline and Cargo-subprocess floor.                                                                           |
| `hook_dispatch/pre_tool_use_local_registry` | Builtin registries disabled and one small local path registry configured | The headline end-to-end case for a representative local registry. It includes registry loading, activation gating, hook selection, and predicate evaluation, but does not isolate their cost from the Cargo-subprocess floor. |


Their shared contract is:


| Field           | Definition                                                                                                                                                                                                                                              |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Claim           | Wall-clock latency of Symposium's in-process `PreToolUse` pipeline in an unchanged Cargo workspace, at its minimal floor and with a representative local registry.                                                                                      |
| Workload        | A simulated agent event using the checked-in fixtures, with default auto-sync enabled, fresh workspace state, and a valid `WorkspaceDeps` disk cache. The local-registry case loads the registry fixture's three plugins.                              |
| Timed operation | Input parsing, auto-sync freshness decision, built-in dispatch, registry and workspace-plugin loading, plugin activation, hook selection, predicate evaluation, and output serialization.                                                               |
| Excluded setup  | Fixture copy, `Symposium` construction, Tokio runtime construction, initial cache population, workspace-state preparation, and invariant checks. CLI startup, configuration parsing, registry refresh, stdin/stdout, and terminal I/O are not measured. |
| Invariants      | The workspace state and dependency cache are valid; metadata and network access are not attempted; the loaded plugin names are exactly the three fixture entries; no external plugin process runs; the expected successful hook output is produced.           |
| Metric          | Wall-clock time per in-process hook dispatch.                                                                                                                                                                                                           |
| Noise           | The two Cargo workspace-lookup subprocesses currently dominate the result and can mask changes in the in-process registry and predicate work. Filesystem and operating-system caches, process scheduling, shared-runner hardware, and developer-level Cargo configuration also vary. |
| Lifecycle       | `experimental`.                                                                                                                                                                                                                                         |


The local registry has three fixed entries:

- `always-active` uses the explicit `depends-on = ["*"]` gate and exercises the active-plugin path;
- `predicate-gated` has a `PreToolUse` hook gated by `path_exists(./.symposium-benchmark-never-present)`; setup asserts that path is absent from the benchmark process's current working directory, so hook selection and predicate evaluation run without spawning its otherwise-valid command;
- `dormant` has no inferred or explicit activation gate and therefore exercises the dormant-plugin path.

The benchmark package calls the public `symposium::hook::execute_hook` API directly rather than depending on `symposium-testlib`. This follows the same simulation seam as the test harness while keeping the benchmark package's support code focused.

The predicate's relative path resolves from the benchmark process's current working directory, not from the copied project fixture. Cargo sets that directory to the `benches/benchsuite` package root. Setup reads the actual current directory and verifies the sentinel path is absent there before measurement.

In the current implementation, the unchanged-workspace path executes `cargo locate-project` once during the auto-sync freshness check and again when the new `WorkspaceDeps` resolves its disk cache. These are identical subprocesses with identical arguments and working directory. The cases make that floor visible and will register a change if the flow later reuses the workspace root or otherwise removes one lookup. That optimization follows the benchmark addition rather than being bundled into it.

The local-registry case therefore reports representative end-to-end dispatch
latency, not an isolated registry-processing measurement. The subprocess floor
can hide changes in the smaller in-process portion; a focused component case is
needed later if those operations require their own regression sentinel.

## Component cases: `WorkspaceDeps`

The initial component target has two cases:


| Case                                         | Prepared state                                     | Timed operation                                                                                       | Interpretation                                                                                                                                      |
| -------------------------------------------- | -------------------------------------------------- | ----------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `workspace_deps/symposium_cache_miss`        | Empty Symposium workspace cache and a new resolver | `WorkspaceDeps::load()`, including workspace lookup, Cargo metadata, and cache write-through          | Quantifies the work avoided by a valid Symposium cache. Most time belongs to Cargo rather than Symposium.                                           |
| `workspace_deps/new_resolver_disk_cache_hit` | Valid disk cache and a new resolver                | `WorkspaceDeps::load()`, including workspace lookup, cache validation, file read, and deserialization | Quantifies the component cost paid by each new resolver when the cache is valid. It is normally dominated by the `cargo locate-project` subprocess. |


The first case is called a *Symposium cache miss*, not a *cold load*. Criterion repeats work on one machine, so the operating system's filesystem cache and the Cargo executable may already be warm. The harness controls Symposium's cache state, not the complete machine state.

The cases may use different Criterion measurement settings. The subprocess case needs fewer, longer samples than an in-process CPU benchmark.

There is no `memory_cache_hit` performance case. Once initialized, `WorkspaceDeps::load()` primarily measures `OnceLock` and `Option::as_ref`, not a meaningful user-visible Symposium operation. The memoization invariant is protected by a correctness test instead.

There is also no direct `try_disk_cache` benchmark in the initial story. That function is private, and exposing an implementation detail solely to measure a microsecond-scale parse is not justified while workspace lookup dominates the user-visible disk-hit path. A direct parsing benchmark can be added if cache size or profiling later shows serialization to be material.

### Fixture

The checked-in `reference-project` fixture contains a virtual Cargo workspace with two members, `cli` and `server`, and three local path dependencies: `domain`, `terminal`, and `storage`. `domain` is shared by both members. The dependency packages are excluded from fixture workspace membership, and a `Cargo.lock` is committed. This produces a small but nontrivial direct dependency graph.

The separate `local-registry` fixture contains the three manifest-backed entries used by the representative hook case. Keeping the registry outside the Cargo project models a separately configured registry and prevents workspace-plugin discovery from observing registry content. The fixtures README records the graph and registry invariants that support code validates before measurement.

The `reference-project` fixture contains `.cargo/config.toml` with Cargo offline mode enabled. Path dependencies and a committed lockfile avoid registry resolution; the Cargo configuration enforces the no-network invariant rather than relying on that layout by convention.

The local-registry configuration points at the copied `local-registry` fixture with a sandbox-relative path. The minimal configuration leaves that directory unconfigured. Both disable the builtin recommendations and user-plugin registries, so neither case depends on mutable user or remote content.

The fixtures represent one small project and one registry, not a workspace-size or plugin-count scaling curve. Larger or generated fixtures require a separate benchmark claim.

Each benchmark run copies the required fixtures into an isolated sandbox. The sandbox also contains dedicated Symposium configuration and cache directories, so a run cannot read or modify the developer's normal Symposium state.

The initial harness does not change the benchmark process's `CARGO_HOME`. Process-wide environment mutation is unsafe once other threads may exist, and the production resolver has no per-command Cargo-environment seam. CI provides an ephemeral Cargo home; local runs may still be influenced by user-level Cargo configuration. The fixture-local offline setting enforces the important no-network property, and the remaining local configuration is recorded as noise rather than expanding production APIs solely for the benchmark.

### Setup and data flow

For `symposium_cache_miss`, per-iteration setup removes only the sandbox's workspace cache and constructs a new resolver. Criterion's per-iteration setup runs outside the timer. The timed load recreates the cache.

For `new_resolver_disk_cache_hit`, setup loads the workspace once and verifies that the cache file exists. Each measured iteration receives a new resolver pointed at that cache, so its in-memory `OnceLock` is empty while the disk cache is valid.

Before measurement, an untimed preflight uses a mock Cargo executable that forwards `locate-project` but rejects `metadata`. A successful load therefore proves that the disk cache was used. Timed samples switch back to the real Cargo executable so the wrapper does not add another shell process to the result.

The harness also validates the workspace root, the expected two members, the expected three dependencies, and the required cache state.

`WorkspaceDeps` records `Cargo.lock` modification times with whole-second granularity. The fixture lockfile is immutable during a benchmark run. Future benchmarks that modify it must account for that granularity rather than assume an immediate timestamp change will invalidate the cache.

## Failures and correctness checks

Shared fixture helpers return errors with the fixture, workspace, or cache path needed to diagnose the failure. The benchmark executable reports the error and stops. A setup failure or `WorkspaceDeps::load()` returning `None` must never be converted into a timing sample.

The benchsuite library has unit tests for fixture discovery, copying, sandbox preparation, and its benchmark-specific mock-Cargo preflight helper. Cache behavior belongs to the main crate and is tested in `tests/workspace_cache.rs` with `symposium-testlib`'s existing cross-platform mock-Cargo support:

1. The first cache-miss load invokes `locate-project` and `metadata` once each; repeated loads through that resolver invoke neither again.
2. A new resolver with a valid disk cache can invoke `locate-project` but must not invoke `metadata`.
3. Advancing `Cargo.lock`'s modification time by one full second invalidates the disk cache and makes a new resolver invoke `metadata` again. The test sets the timestamp explicitly rather than sleeping or relying on filesystem timing.

The wrapper is never part of a timed sample.

Criterion targets support a fast smoke run through:

```text
cargo test -p symposium-benchsuite --benches
```

Smoke runs execute workloads without collecting full measurements. Normal pull request CI compiles benchmark targets and runs the small support-library and cache-invariant tests. Full smoke and measurement runs belong to the benchmark workflow.

## Commands

The operator guide records the authoritative commands. The initial interface is:

```text
cargo check -p symposium-benchsuite --all-targets
cargo test -p symposium-benchsuite --lib
cargo test -p symposium-benchsuite --benches
cargo bench -p symposium-benchsuite --bench workspace_deps
cargo bench -p symposium-benchsuite --bench hook_dispatch
```

Criterion filters allow an individual group or case to run without executing unrelated benchmark additions.

## CI and result lifecycle

Normal pull request CI runs `cargo check -p symposium-benchsuite --all-targets` on native Linux, macOS, and Windows jobs. The musl cross-compilation job is not part of the initial benchmark check. Support-library and cache-invariant tests run as ordinary correctness tests.

A separate measurement workflow runs:

- on manual dispatch for a chosen ref;
- on pull requests that change benchmark code or a path participating in the measured flows, including the benchmark workflow file itself.

The pull-request path filter follows package and configuration ownership boundaries rather than listing individual source modules. It covers `.cargo/**`, `benches/**`, `src/**`, `symposium-install/**`, `symposium-sdk/**`, the root Cargo manifests, and the benchmark workflow itself. This deliberately accepts some extra runs: a module-by-module list can miss a transitive dependency or become incomplete when code moves, silently leaving measured behavior uncovered.

There is no weekly schedule initially. A scheduled job is added only when its results have a durable consumer or a named maintainer responsible for reviewing them. Until then, a recurring artifact would be write-only storage.

The measurement workflow uses `ubuntu-24.04` and names an exact Rust toolchain version rather than the moving `stable` alias. Changing either is an explicit benchmark-environment change and resets historical comparability. It uses Node 24-compatible releases of the official GitHub checkout, cache, and artifact actions so the measurement job does not depend on a deprecated runner runtime. Each run records the commit SHA, Rust and Cargo versions, operating-system details, and available CPU information.

Before measuring, the workflow runs every Criterion target once in test mode. This separates workload correctness from statistical measurement and fails early when fixture preparation, invariants, or timed operations are broken.

Headline estimates are written to the Actions job summary so the person who triggered a run can read them without downloading an archive. The summary labels the measurements as experimental and informational. For the `WorkspaceDeps` pair, it includes both medians and the derived cache-miss-to-hit speedup ratio. Criterion's full result directory is uploaded as an expiring, run-attempt-specific artifact only for post-hoc inspection. The attempt identifier prevents an immutable artifact from colliding with one produced by an earlier rerun. General build caches must not implicitly supply an unnamed Criterion baseline; otherwise the displayed comparison can refer to an unrelated run.

The initial workflow does not fail because of a measured slowdown and is not a required merge gate. Compilation failures, setup failures, and benchmark crashes remain visible failures rather than being hidden with `continue-on-error`.

Benchmarks move through three lifecycle states:

```text
experimental -> observed -> gated
```

- **Experimental to observed:** the workload contract is unchanged across six consecutive successful runs using the same runner image and exact toolchain; a maintainer reviews the results; and `(maximum median - minimum median) / median of the six medians` is at most 10%.
- **Observed to gated:** a named owner agrees to triage failures; at least 20 paired no-change base/head comparisons run in the intended comparison environment; the 95th percentile absolute paired difference is at most 3%; and the regression threshold is no smaller than twice that measured noise.

A benchmark that does not meet these conditions remains in its current state. The numbers are initial operating criteria and can be revised explicitly when collected data demonstrates that a different definition is more useful.

All initial benchmarks start as experimental. Shared GitHub-hosted hardware may never be stable enough for gating even with a fixed image and toolchain; gating can therefore require paired execution on a controlled runner or dedicated hardware.

## Incremental delivery

The first pull request is built as independently working additions:

1. root-package safeguards, the benchsuite workspace package, and the operator README;
2. shared fixture and sandbox support with unit tests;
3. cache-behavior correctness tests;
4. the Symposium-cache-miss component case;
5. the new-resolver disk-cache-hit component case;
6. the minimal and local-registry unchanged-workspace `PreToolUse` cases;
7. CI workflows and final documentation updates.

Each addition compiles and has one stated purpose before the next is introduced. Implementation findings can revise this design when the code exposes a misleading workload or unnecessary abstraction.
