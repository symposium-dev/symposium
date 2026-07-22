# Predicate caching

## TL;DR

Predicates, especially custom predicates, spawn processes on every sync. This RFD lets custom predicates emit granular JSONL watch events for files, environment variables, and time. Symposium caches their results and skips reevaluation while every watched input is unchanged and no watch expires. No watch events means cached indefinitely; `WatchTime(0)` means never cached.

## Problem

Auto-sync means predicates re-evaluate on every agent session start. A workspace with 10 plugins, each with a custom predicate, forks 10+ processes every time.

## Design

Custom predicates already emit JSONL events to stdout. We add one event per watched resource:

```rust
#[non_exhaustive]
enum CustomPredicateEvent {
    // ... existing variants ...
    /// Result depends on the contents of the given file.
    WatchFile(PathBuf),
    /// Result depends on the value of the given environment variable.
    WatchEnv(String),
    /// Result becomes stale after this many milliseconds.
    WatchTime(usize),
}
```

A predicate can emit any number of these events:

```jsonl
{"watchFile": "CargoBrazil.toml"}
{"watchFile": "Config"}
{"watchEnv": "LAMBDA_ENV"}
{"watchTime": 60000}
```

Symposium unions the file and environment events. A change to any watched input or expiry of the shortest `WatchTime` causes one reevaluation. Files are relative to the workspace root.

The process exit status determines the predicate result. Watch events only control caching. No watch events means the result is cached indefinitely; predicates must report every changing input or Symposium may reuse a stale result. `WatchTime(0)` means the result is stale immediately, which effectively disables caching. The `#[non_exhaustive]` attribute leaves room for new watch kinds.

## SDK helper

The `symposium-sdk` crate provides a helper that reads an environment variable and emits its watch event:

```rust
let val = symposium_sdk::env::var("LAMBDA_ENV")?;
// Emits {"watchEnv": "LAMBDA_ENV"}.
```

Multiple helper calls emit multiple events, which Symposium unions.

## How it works

File fingerprints use `mtime + size`; missing is a valid state. Environment fingerprints use the current value or absent state. Time fingerprints use the wall-clock time at which the entry becomes stale.

Cache lives at `~/.symposium/cache/predicates.json`.

1. Look up the predicate in the cache.
2. If all watched inputs match and no `WatchTime` has elapsed, use the cached result. An empty watch set always matches.
3. Otherwise, evaluate the predicate and obtain its result from the exit status.
4. Store the result with its emitted watch events. `WatchTime(0)` yields an immediately stale entry.

Cache is discarded on Symposium version upgrade.

## Built-in predicates

- `workspace-member()` requires no cache because it is already cheap and evaluated in memory.
- `path_exists(path)` emits `WatchFile(path)`.
- `env(FOO=BAR)` emits `WatchEnv("FOO")`.
- `shell(cmd)` emits `WatchTime(0)` because its inputs are unknown.
- Caching `depends-on(name)` is deferred to the PM interface work.

## PM integration

Changes to `list_deps` and PM-derived caching are deferred to the PM interface work.

## Implementation steps

1. Add and parse `WatchFile`, `WatchEnv`, and `WatchTime` events while keeping the exit status as the predicate result.
2. Union watch events, cache an empty watch set indefinitely, and treat `WatchTime(0)` as immediate staleness.
3. Add cache storage and fingerprint comparison for files, environment variables, and expiry times.
4. Wire `path_exists`, `env`, and `shell` to emit their watch events.
5. Add `symposium_sdk::env::var()` to emit environment watch events.
