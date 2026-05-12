# Important flows

This section describes the logic of each `cargo agents` command.

## Crate-sourced skill resolution

When a skill group uses `source = "crate"` or `source.crate_path`, the sync flow takes an additional path:

1. `predicate::union_matched_crates()` resolves plugin-level and group-level predicates against the workspace to produce a set of concrete crate name/version pairs.
2. For each crate in the set, `RustCrateFetch` fetches the source — checking path overrides (for local path deps), then the cargo registry cache, then crates.io.
3. `discover_skills()` scans the specified subdirectory within each fetched crate source.

The key code paths are in `skills.rs` (`load_skills_for_group`), `predicate.rs` (`matched_crates`, `union_matched_crates`), and `crate_sources/mod.rs` (`RustCrateFetch`, `WorkspaceCrate`).
