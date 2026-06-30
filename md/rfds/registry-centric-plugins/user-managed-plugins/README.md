# User-managed plugins

## TL;DR

Add `cargo agents use`/`remove` commands for explicitly installing and removing plugins. Local-by-default scoping via a `workspace-directory()` predicate. Add `cargo agents status` for inspecting what's active and why.

## Change in a nutshell

### `cargo agents use` / `cargo agents remove`

```bash
cargo agents use my-plugin          # scoped to current workspace
cargo agents use --global my-plugin # user-wide
cargo agents remove my-plugin
```

Following the npm convention: local by default, `--global` for user-wide.

To avoid adding config files into the project directory, per-project dependencies are stored centrally in `~/.symposium/config.toml` as conditional `[[plugins]]` entries gated on a `workspace-directory()` predicate:

```toml
[[plugins]]
where.predicate = "workspace-directory(/home/me/dev/my-project)"
source.cargo = "my-plugin"
```

This keeps the project repo clean while still scoping plugins to specific workspaces.

### The `workspace-directory(D)` predicate

The `workspace-directory(D)` predicate tests the directory of the active workspace. It is true if the workspace is at or below `D` (prefix match).

Both sides are canonicalized before comparison. If canonicalization fails (path doesn't exist), the predicate evaluates to `false`.

### `cargo agents status`

Shows what's currently loaded, active, and inactive (with reasons). The debugging tool for "why isn't my plugin activating?" — shows predicate evaluation results and provenance information.

## Implementation plan

### Step 1: `workspace-directory()` predicate

Add the predicate variant. Prefix match, canonicalization, failure → false.

### Step 2: CLI `use`/`remove` with `--global`

`use` without `--global` writes a directory-scoped entry; `remove` searches all entries and removes matches.

### Step 3: `cargo agents status`

Show loaded/active/inactive plugins with predicate evaluation results.

## Tests

### `workspace-directory()` predicate

- `workspace_directory_exact_match` — cwd is exactly `D`; predicate true.
- `workspace_directory_subdirectory_match` — cwd is below `D`; predicate true.
- `workspace_directory_sibling_no_match` — cwd is sibling of `D`; predicate false.
- `workspace_directory_canonicalizes_symlinks` — paths with symlinks/`..` still match.
- `workspace_directory_nonexistent_path_false` — `D` doesn't exist on disk; predicate false.

### CLI commands

- `use_local_writes_scoped_entry` — `cargo agents use foo` writes `[[plugins]]` with `where.predicate = "workspace-directory(...)"`.
- `use_global_writes_unscoped_entry` — `cargo agents use --global foo` writes `[[plugins]]` without predicate.
- `use_appends_to_existing_scoped_entry` — running `use bar` from same workspace appends to existing entry.
- `remove_deletes_matching_entry` — `cargo agents remove foo` removes the entry.
- `remove_cleans_empty_entry` — if removing the last source from an entry, the entry itself is removed.
- `remove_nonexistent_warns` — removing a plugin not in config produces helpful message.

### Integration

- `use_and_sync_installs_locally` — `use` from workspace, sync installs skills only in that workspace.
- `use_global_installs_everywhere` — `use --global`, sync installs regardless of cwd.
- `scoped_entry_inactive_in_other_workspace` — plugin scoped to `/home/me/project-a` does NOT load when syncing in `/home/me/project-b`.
- `use_then_remove_cleans_up` — `use foo`, sync (skills installed), `remove foo`, sync (skills removed).
- `status_shows_active_and_inactive` — two plugins (one passing predicates, one not); output labels both correctly.
