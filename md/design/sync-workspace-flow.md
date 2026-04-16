# `cargo agents sync --workspace`

Updates `.symposium/config.toml` to reflect the current workspace dependencies.

## Flow

1. **Check `Cargo.lock` mtime** — compare the mtime of `Cargo.lock` against the cached value (stored in the global cache directory). If unchanged, skip the rest — there's nothing to do.

2. **Read workspace dependencies** — run `cargo metadata` to get the full dependency list for the workspace.

3. **Load plugin sources** — read the user config's `[[plugin-source]]` entries and load their plugin manifests. For git sources, fetch/update as needed.

4. **Match extensions to dependencies** — for each plugin, evaluate skill group crate predicates and individual skill `crates` frontmatter against the workspace dependencies. Also discover available workflows.

5. **Merge with existing config** — load the current `.symposium/config.toml` and reconcile:
   - **New extensions**: add entries with the resolved `sync-default` value (from project config if set, else user config).
   - **Removed dependencies**: remove entries for extensions whose crate predicates no longer match.
   - **Existing entries**: preserve the user's on/off choices.

6. **Write config** — write the updated `.symposium/config.toml`.

7. **Cache mtime** — store the current `Cargo.lock` mtime so future runs can skip work if nothing changed.
