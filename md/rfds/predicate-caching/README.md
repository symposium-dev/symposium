# Predicate caching

Predicates (especially `shell(...)`) spawn processes on every sync. This RFD helps them declare watch paths. Symposium caches results and skips re-evaluation while watched files are unchanged. No watch declaration = never cached.

## Problem

Auto-sync means predicates re-evaluate on every agent session start. A workspace with 10 plugins, each with a `shell(...)` gate, forks 10+ processes every time.

## Design

Two ways to declare watch paths:

### 1. Syntactic

For predicates whose inputs are known at authorship time:

```toml
[[skills]]
predicates = ["shell(grep -q lambda CargoBrazil.toml)"]
watch = ["CargoBrazil.toml"]
source.path = "skills/lambda"
```

`watch` is a sibling field, not embedded in the predicate string.

### 2. Runtime

The shell command emits JSON to stdout:

```json
{"result": true, "watch": ["CargoBrazil.toml"]}
```

Commands that print nothing or non-JSON fall back to exit-code semantics with no caching. Backward compatible.

Runtime takes priority when both are present. Either is sufficient to enable caching.

## How it works

Fingerprint is `mtime + size` (same model as Cargo). A missing file is a valid fingerprint state — invalidates when the file appears.

Cache lives at `predicate-cache.json`, keyed by the normalized predicate string. Two plugins with the same predicate share one entry.

Algorithm:

1. Look up predicate in cache
2. If found and all watched files match fingerprints → use cached result
3. Otherwise → evaluate, store result + watch paths if declared
4. On write, prune entries for predicates no longer in any manifest

Cache is discarded on Symposium version upgrade. `watch = []` (empty) means static for the current invocation only — no persistent caching without at least one file to stat.

## Built-in predicates

- `workspace-member()` → cached, watch = [] (static per run)
- `path_exists(path)` → cached, watches the path itself
- `depends-on(name)` → cached, watches PM manifest (e.g. `Cargo.lock`)
- `env(...)` → never cached (env isn't file-observable)
- `shell(...)` → cached only with explicit watch declaration

## PM integration

`list_deps` return type expands to include optional watch paths:

```rust
struct DepsResult {
    ids: Vec<PackageId>,
    watch: Option<Vec<WatchEntry>>,
}
```

`CargoPm` returns `[Cargo.lock]`. This is a return-type change, not a new method.

## Implementation steps

1. Extend `PredicateResult` with `watch: Option<Vec<WatchEntry>>`. All evaluators return `None` initially — no behavior change.
2. Cache storage: read/write, fingerprint comparison, dead-entry pruning.
3. Built-in predicates return their natural watch sets.
4. Shell runtime output: parse stdout JSON, fall back to exit code for non-JSON.
5. Syntactic `watch` field on `[[skills]]` groups.
6. PM `list_deps` watch support (coordinate with pm-split).
