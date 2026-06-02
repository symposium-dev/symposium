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
