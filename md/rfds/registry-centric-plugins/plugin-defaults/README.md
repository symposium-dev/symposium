# Plugin defaults

## TL;DR

Every plugin gets default content (recursive sub-plugin search, skill directories) unless explicitly suppressed. When a `SYMPOSIUM.toml` sits alongside a `Cargo.toml`, binary targets become implicit installations.

## Change in a nutshell

### Synthesized manifests

If a plugin directory has no `SYMPOSIUM.toml`, we synthesize an empty one. Since all fields are optional, this means the defaults below still apply. The simplest plugin is just a directory with a `skills/` subdirectory — no manifest needed.

### `[[skills]]` entries

A `[[skills]]` entry declares a source of skills within a plugin. It uses the same `where` and `source` structure as `[[plugins]]`:

```toml
[[skills]]
where.cargo = { serde = "*" }
source.path = "skills/serde"
```

The key difference from `[[plugins]]`: a `[[plugins]]` source resolves to a *plugin directory* (scanned for `SYMPOSIUM.toml`), while a `[[skills]]` source resolves to a *skill directory* (scanned recursively for `SKILL.md` files). In practice, `source.path` is the common case for skills (pointing at a subdirectory within the plugin), though `source.git` or `source.cargo` could work too.

### Default entries

Unless explicitly suppressed with a `defaults` section, the following content is added to each plugin:

```toml
# Suppressable with `defaults.plugins = false`
[[plugins]]
source.path = "."

# Suppressable with `defaults.skills = false`
[[skills]]
source.path = "skills"

[[skills]]
where.predicate = "workspace()"
source.path = ".agents/skills"
```

`defaults.plugins = false` and `defaults.skills = false` only suppress the *default* entries shown above. Explicit `[[plugins]]` or `[[skills]]` entries written by the user are never affected.

The net impact on the user is as follows. Given a plugin `P` defined by `$P/SYMPOSIUM.toml` found in the directory `$P`:

* Any `SYMPOSIUM.toml` files found in subdirectories of a plugin (e.g., `$P/Q/SYMPOSIUM.toml`) is itself loaded as a plugin.
* Any skills found in `$P/skills` are loaded into the agent whenever the plugin `P` is loaded (e.g., if it is found in the dependencies of the current workspace, or if it is in the current workspace).
* Any skills found in `$P/.agents/skills` are loaded into the agent whenever the plugin `P` is part of the active workspace.
  * If this plugin is obtained via "use" or from a dependency, then `workspace()` will evaluate to false and the skills will not be loaded.

### Default installations

When a `SYMPOSIUM.toml` sits alongside a `Cargo.toml`, the `[[bin]]` targets declared in `Cargo.toml` are automatically available as installations. This means a hook can reference a binary by name without an explicit `[[installations]]` entry:

```toml
# Cargo.toml
[[bin]]
name = "my-linter"

# SYMPOSIUM.toml — no [[installations]] needed
[[hooks]]
name = "lint-check"
event = "PreToolUse"
command = "my-linter"
```

Symposium resolves `command = "my-linter"` against the crate's binary targets, builds/fetches the binary as needed, and invokes it. This removes boilerplate for the common case where a plugin crate ships its own tooling.

### Recursive directory search

All directory search is transitive. A directive like `source.path = "skills"` searches recursively for `SKILL.md` files (but not within a directory that already contains a `SKILL.md` file). Similarly `source.path = "."` on a `[[plugins]]` entry searches recursively for `SYMPOSIUM.toml` files (but not within the subdirectory of another plugin).

## Implementation plan

### Step 1: Default injection

After parsing a `SYMPOSIUM.toml`, inject default entries unless suppressed by `[defaults]`. For missing manifests, synthesize an empty one first.

### Step 2: Recursive skill scanning and flattening

Change `discover_skills()` from flat one-level scan to recursive. Stop recursion at `SKILL.md` boundaries. When installing into flat agent skill directories, hoist nested skills to the top level by skill name. Disambiguate with a hash suffix on collision.

**Test cases:**

- **Nested skill is hoisted.** Given `.agents/skills/subdir/foo/SKILL.md`, Symposium installs `.claude/skills/foo/SKILL.md` (with `.symposium` marker and `.gitignore`).

- **Same-name skills from different subdirectories are disambiguated.** Given:
  ```
  .agents/skills/
    guides/deploy/SKILL.md
    runbooks/deploy/SKILL.md
  ```
  One gets `.claude/skills/deploy/` and the other gets `.claude/skills/deploy-<hash>/`.

- **User-managed skill takes priority.** If `.claude/skills/foo/SKILL.md` already exists without a `.symposium` marker, Symposium installs the discovered skill as `.claude/skills/foo-<hash>/SKILL.md` instead.

- **Same subdirectory and skill name.** Given:
  ```
  .agents/skills/
    foo/
      SKILL.md
    subdir/
      foo/
        SKILL.md
  ```
  Both are named "foo" — one gets the unsuffixed slot, the other gets the hash suffix.

### Step 3: Default installations from `Cargo.toml`

Detect `[[bin]]` targets in a sibling `Cargo.toml` and register them as implicit installations.

## Tests

### Synthesized manifests

- `missing_symposium_toml_gets_empty_manifest` — directory with no `SYMPOSIUM.toml` produces a valid empty plugin.
- `empty_symposium_toml_gets_defaults` — empty file gets default entries injected.

### Default entries

- `default_plugins_entry_added` — `[[plugins]] source.path = "."` added by default.
- `default_skills_entries_added` — both `skills/` and `.agents/skills` (with workspace gate) added.
- `defaults_plugins_false_suppresses` — `defaults.plugins = false` suppresses default `[[plugins]]` only.
- `defaults_skills_false_suppresses` — `defaults.skills = false` suppresses default `[[skills]]` only.
- `explicit_entries_not_affected_by_suppression` — user-written entries preserved when defaults suppressed.

### Recursive scanning

- `recursive_skill_discovery` — `skills/a/SKILL.md` and `skills/b/sub/SKILL.md` both found.
- `recursion_stops_at_skill_boundary` — `skills/a/SKILL.md` exists; `skills/a/nested/SKILL.md` NOT found.
- `recursion_stops_at_plugin_boundary` — subdirectory with `SYMPOSIUM.toml` not scanned for skills.

### Flattening and disambiguation

- `nested_skill_hoisted_to_top_level` — `.agents/skills/subdir/foo/SKILL.md` installs as `.claude/skills/foo/`.
- `same_name_different_subdirs_disambiguated` — two `deploy` skills from different parents; one gets hash suffix.
- `user_managed_skill_not_clobbered` — pre-existing skill without `.symposium` marker keeps its slot.
- `same_subdirectory_and_skill_name` — `foo/SKILL.md` and `subdir/foo/SKILL.md`; one gets hash suffix.

### Default installations

- `cargo_toml_bin_targets_become_installations` — sibling `Cargo.toml` with `[[bin]] name = "my-tool"` creates implicit installation.
- `no_cargo_toml_no_implicit_installations` — no sibling `Cargo.toml`, no installations.
- `explicit_installation_overrides_implicit` — user declares `[[installations]] name = "my-tool"`; implicit one not duplicated.
- `hook_resolves_implicit_binary` — `[[hooks]] command = "my-linter"` resolves against `Cargo.toml` binary targets.

### Integration

- `bare_directory_with_skills_installs` — plugin directory with only `skills/greet/SKILL.md` (no manifest); skill installed.
- `workspace_only_skills_not_loaded_from_dependency` — plugin loaded via config (`used`); `.agents/skills` gated on `workspace()` NOT installed.
