# Plugin sources

A **plugin source** is a crate that Symposium scans for plugins and standalone skills. Plugin sources can come from crates.io, a git repository, or a local path.

## How crates become plugin sources

There are four ways a crate (or directory) becomes a plugin source:

1. **The workspace itself** — The workspace root and each member crate are always scanned. Gated by the `workspace()` predicate.
2. **Explicit install** — `cargo agents install <crate>` adds it to your config. Gated by `installed()`.
3. **Dependency allow list** — A workspace dependency matches an entry in the `dependency-allow-list` (configured in `config.toml` or declared by an installed plugin crate). Gated by `dependency()`.
4. **Default** — `symposium-recommendations` is installed by default during `cargo agents init`.

## Discovery rules

When Symposium loads a plugin source crate, it scans from the crate root:

1. **Walk recursively for `SYMPOSIUM.toml`** — Each directory containing one is a [plugin](./plugin-definition.md). That subtree is not searched further.

2. **If no `SYMPOSIUM.toml` found anywhere** — Fall back to scanning `$ROOT/skills/` recursively for `SKILL.md` files. These become an anonymous, skills-only plugin that installs unconditionally (for explicit installs) or requires frontmatter predicates (for allow-list discovery).

We do not allow plugins or standalone skills to be nested within one another. When we find a directory that is either a plugin or a skill, we do not search its contents any further.

### Example structure

```text
my-plugin-crate/
  Cargo.toml
  SYMPOSIUM.toml            # ✓ Plugin (at crate root)
  skills/                   # ✗ Not searched (parent claimed by SYMPOSIUM.toml)
    basic/
      SKILL.md
```

```text
multi-plugin-crate/
  Cargo.toml
  serde/
    SYMPOSIUM.toml          # ✓ Plugin
    skills/
      basics/
        SKILL.md
  tokio/
    SYMPOSIUM.toml          # ✓ Plugin
    skills/
      async-patterns/
        SKILL.md
```

```text
skills-only-crate/
  Cargo.toml
  skills/                   # ✓ Fallback: scanned because no SYMPOSIUM.toml exists
    basics/
      SKILL.md
    advanced/
      SKILL.md
```

## Managing plugin sources

```bash
# Install a plugin crate
cargo agents install my-plugin

# Install from git
cargo agents install --git https://github.com/org/my-plugin

# Install from a local path (for development)
cargo agents install --path ./my-plugins

# Uninstall
cargo agents uninstall my-plugin

# See what's installed and active
cargo agents status
```

## Validation

You can validate a plugin source:

```bash
# Validate a single plugin manifest
cargo agents plugin validate path/to/SYMPOSIUM.toml

# Validate a directory containing plugins
cargo agents plugin validate path/to/crate-root/

# Skip crates.io name checking (for private crates)
cargo agents plugin validate path/to/SYMPOSIUM.toml --no-check-crates
```
