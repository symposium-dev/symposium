# Predicate caching

## TL;DR

Predicates, especially custom predicates, spawn processes on every sync. This RFD lets custom predicates emit `Watch` JSONL events for files and environment variables. Symposium caches their results and skips reevaluation while the union of watched inputs is unchanged. No watch hints means cached indefinitely; `Volatile` means never cached.

## Problem

Auto-sync means predicates re-evaluate on every agent session start. A workspace with 10 plugins, each with a custom predicate, forks 10+ processes every time.

## Design

Custom predicates already emit JSONL events to stdout. We add two variants:

```rust
#[derive(Serialize, Deserialize)]
enum CustomPredicateEvent {
    // ... existing variants ...
    Watch {
        files: Vec<PathBuf>,
        env: HashMap<String, String>,
    },
    Volatile {},
}
```

A predicate can emit multiple watch hints:

```jsonl
{"watch": {"files": ["CargoBrazil.toml"]}}
{"watch": {"files": ["Config"]}}
{"watch": {"env": {"LAMBDA_ENV": "prod"}}}
```

Symposium unions these into one watch set. A change to any watched input causes one reevaluation. Files are relative to the workspace root.

The process exit status determines the predicate result. `Watch` and `Volatile` events only control caching. No `Watch` events means the result is cached indefinitely and predicates must report every changing input or Symposium may reuse a stale result. `Volatile` disables caching and takes precedence over watch hints.

```jsonl
{"volatile": {}}
```

## SDK helper

The `symposium-sdk` crate provides a helper that reads an environment variable and emits its watch event:

```rust
let val = symposium_sdk::env::var("LAMBDA_ENV")?;
// Emits {"watch": {"env": {"LAMBDA_ENV": "prod"}}}.
```

Multiple helper calls emit multiple events, which Symposium unions.

## How it works

File fingerprints use `mtime + size`; missing is a valid state. Environment fingerprints use the current value or absent state.

Cache lives at `~/.symposium/cache/predicates.json`.

1. Look up the predicate in the cache.
2. If all watched inputs match, use the cached result. An empty watch set always matches.
3. Otherwise, evaluate the predicate and obtain its result from the exit status.
4. If it emitted `Volatile`, do not cache the result. Otherwise, store the result with the union of its watch hints.

Cache is discarded on Symposium version upgrade.

## Built-in predicates

- `workspace-member()` requires no cache because it is already cheap and evaluated in memory.
- `path_exists(path)` watches the path itself.
- `env(FOO=BAR)` watches the value of `FOO`.
- `shell(cmd)` is volatile because its inputs are unknown.
- Caching `depends-on(name)` is deferred to the PM interface work.

## PM integration

Changes to `list_deps` and PM-derived caching are deferred to the PM interface work.

## Implementation steps

1. Add and parse `Watch` and `Volatile` events while keeping the exit status as the predicate result.
2. Union watch hints, cache an empty watch set indefinitely, and give `Volatile` precedence.
3. Add cache storage and fingerprint comparison.
4. Wire `path_exists`, `env`, and volatile `shell` behavior.
5. Add `symposium_sdk::env::var()` to emit environment watch events.
