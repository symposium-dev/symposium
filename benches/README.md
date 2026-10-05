# Symposium benchmarks

This directory contains Symposium's focused performance benchmarks, checked-in workloads, and shared benchmark support. The suite is developed incrementally: every target should be independently runnable and have a clearly documented interpretation.

See the [benchmarking design](../md/design/benchmarking.md) for the suite architecture, measurement policy, CI strategy, and lifecycle criteria.

## Layout

- `benchsuite/` is the non-publishable workspace package containing benchmark targets and shared support code.
- `fixtures/` contains the composable deterministic workloads described in its own [README](fixtures/README.md).

Shared support code handles fixtures and sandbox mechanics. Each benchmark target is responsible for defining its own scenarios and timed operations.

## Current targets

| Target | Cases | Lifecycle |
| --- | --- | --- |
| [`workspace_deps`](benchsuite/benches/workspace_deps.rs) | `symposium_cache_miss`, `new_resolver_disk_cache_hit` | Experimental |
| [`hook_dispatch`](benchsuite/benches/hook_dispatch.rs) | `pre_tool_use_minimal_config`, `pre_tool_use_local_registry` | Experimental |

`workspace_deps` compares dependency resolution with an empty Symposium
workspace cache against a new resolver reusing a valid disk cache. The miss is
not a fully cold machine load: Cargo and operating-system caches may already be
warm. The target's source contains the complete measurement contracts.

`hook_dispatch` measures the in-process `PreToolUse` path in an unchanged
workspace. Its minimal case disables all plugin registries to establish the
fixed pipeline and Cargo workspace-lookup floor. Its local-registry case adds
the checked-in three-plugin registry as a representative end-to-end workload.
The two Cargo workspace lookups currently dominate both cases, so the latter
is not an isolated measurement of registry or predicate processing.

## Commands

Run commands from the repository root:

```text
cargo check -p symposium-benchsuite --all-targets
cargo test -p symposium-benchsuite --lib
cargo test -p symposium-benchsuite --benches
cargo bench -p symposium-benchsuite --bench workspace_deps
cargo bench -p symposium-benchsuite --bench hook_dispatch
```

Pass a case name after `--` to run only that case.

## Benchmark contracts

Every benchmark target documents its measurement contract in the crate-level doc comment next to the implementation. This README acts as an index and does not duplicate those contracts.

## Lifecycle

New benchmarks begin as `experimental`. Measurements remain informational until sufficient history demonstrates that a benchmark is stable enough to become `observed` or `gated`.
