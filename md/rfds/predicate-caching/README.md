# Predicate caching

## TL;DR

Predicates (especially custom predicates) spawn processes on every sync. This RFD lets them emit watch declarations via JSONL events. Symposium caches results and skips re-evaluation while watched files/env vars are unchanged. No watch declaration = never cached.

## Problem

Auto-sync means predicates re-evaluate on every agent session start. A workspace with 10 plugins, each with a custom predicate, forks 10+ processes every time.

## Design

Custom predicates already emit JSONL events to stdout. We add a `Watch` event variant:

```rust
#[derive(Serialize, Deserialize)]
enum CustomPredicateEvent {
    // ... existing variants (e.g. SelectedCrates) ...
    Watch {
        files: Vec<PathBuf>,
        env: HashMap<String, String>,
    }
}
```

Emitted as a JSONL line:

```jsonl
{"watch": {"files": ["CargoBrazil.toml"], "env": {"LAMBDA_ENV": "prod"}}}
```

The predicate result still comes from exit status — the watch event is just a cache hint. Predicates that don't emit a `Watch` event are never cached (current behavior preserved).

Both `files` and `env` are optional (either alone is sufficient to enable caching). Files are relative to workspace root.

## SDK helper

The `symposium-sdk` crate provides a helper that reads env vars and automatically emits the cache line:

```rust
// In a custom predicate binary:
let val = symposium_sdk::env::var("LAMBDA_ENV")?;
// ^ reads the var AND emits {"watch": {"env": {"LAMBDA_ENV": "prod"}}}
```

This way predicate authors get caching for free when they use the SDK.

## How it works

Fingerprint for files is `mtime + size` (same model as Cargo). A missing file is a valid fingerprint state — invalidates when the file appears. Env fingerprint is the literal string value (or absent).

Cache lives at `~/.symposium/cache/predicates.json`, keyed by the normalized predicate string. No workspace-root keying needed — watch paths are resolved relative to the current workspace root at stat time, so different workspaces naturally produce different fingerprints.

Algorithm:

1. Look up predicate in cache
2. If found and all watched files + env vars match fingerprints → use cached result
3. Otherwise → evaluate, store result + watch if declared
4. On write, prune entries for predicates no longer in any manifest

Cache is discarded on Symposium version upgrade.

## Built-in predicates

- `workspace-member()` → no cache needed (already cheap, in-memory)
- `path_exists(path)` → cached, watches the path itself
- `depends-on(name)` → cached, watches PM manifest (e.g. `Cargo.lock`)
- `env(FOO=BAR)` → cached, watches env value of `FOO`
- `shell(cmd)` / custom predicates → cached only when they emit a `Watch` event

## PM integration

`list_deps` return type expands to include optional watch paths:

```rust
struct DepsResult {
    ids: Vec<PackageId>,
    watch: Option<Vec<PathBuf>>,
}
```

`CargoPm` returns `[Cargo.lock]`. This is a return-type change, not a new method.

## Implementation steps

1. Add `Watch` variant to `CustomPredicateEvent`. Parse it from predicate stdout. No caching yet — just capture the data.
2. Cache storage in `~/.symposium/cache/predicates.json`: read/write, fingerprint comparison, dead-entry pruning.
3. Wire built-in predicates: `path_exists` emits file watch, `env` emits env watch, `depends-on` uses PM watch paths.
4. Add `symposium_sdk::env::var()` helper that auto-emits the cache event.
5. PM `list_deps` watch support (coordinate with pm-split).
