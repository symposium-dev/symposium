# Recommending Agent Mods

Recommendations tell Symposium when to suggest mods to users.

## TL;DR

### You want to add recommendations for your own use

Create [`~/.config/symposium/config/recommendations.toml`](#local-recommendations) (Linux) or equivalent:

```toml
# Always recommend this mod
[[recommendation]]
source.cargo = "my-favorite-mod"

# Recommend when a file exists
[[recommendation]]
source.cargo = "my-rust-mod"
when.file-exists = "Cargo.toml"

# Recommend when using a specific crate
[[recommendation]]
source.cargo = "my-tokio-helper"
when.using-crate = "tokio"
```

### You want to recommend mods for a specific workspace

Create `.symposium/recommendations.toml` in the project workspace directory:

```toml
# Always recommend for this workspace
[[recommendation]]
source.cargo = "team-internal-mod"

# Recommend when using a specific crate
[[recommendation]]
source.cargo = "our-graphql-tools"
when.using-crate = "async-graphql"
```

### You maintain a library and want to recommend a companion mod for your users

Add this into your project's `Cargo.toml`:

```toml
# In your Cargo.toml
[package.metadata.symposium]
recommended = ["my-companion-mod"]
```

### You want to recommend a mod for someone else's crate

Submit a PR to [symposium-dev/recommendations](https://github.com/symposium-dev/recommendations) adding a new file like `extensions/your-mod-name.toml` that looks like:

```toml
[[recommendation]]
source.cargo = "your-mod-name"
when.using-crate = "their-library"
```

## Ways to Create Recommendations

1. [**Local recommendations**](#local-recommendations) - add your own recommendations for personal use
2. [**Workspace recommendations**](#workspace-recommendations) - add recommendations for a specific workspace
3. [**Crate metadata**](#crate-metadata) - add recommendations directly in your crate's Cargo.toml (for library authors)
4. [**Central recommendations**](#central-recommendations) - submit to the Symposium recommendations registry (for mods that complement someone else's crate)

### Local Recommendations

Add your own recommendations for internal or proprietary mods. Create a file at:

| Platform | Location |
|----------|----------|
| Linux | `~/.config/symposium/config/recommendations.toml` |
| macOS | `~/Library/Application Support/symposium/config/recommendations.toml` |
| Windows | `%APPDATA%\symposium\config\recommendations.toml` |

Example:

```toml
# Always recommend our internal tooling mod
[[recommendation]]
source.cargo = "company-internal-mod"

# Recommend our GraphQL mod when using async-graphql
[[recommendation]]
source.cargo = "company-graphql-mod"
when.using-crate = "async-graphql"
```

Local recommendations are merged with central recommendations.

### Workspace Recommendations

Projects can include workspace-specific recommendations by creating a `.symposium/recommendations.toml` file in the project root:

```toml
# .symposium/recommendations.toml

# Always recommend for this workspace
[[recommendation]]
source.cargo = "my-internal-mod"

# Conditional - only when using a specific crate
[[recommendation]]
source.cargo = "my-graphql-mod"
when.using-crate = "async-graphql"
```

This is useful for:

- Recommending internal or proprietary mods to team members
- Project-specific mods that aren't relevant globally
- Mods that should be suggested only for this workspace

Workspace recommendations are merged with central and user recommendations, with the same `when` condition filtering applied.

### Crate Metadata

If you maintain a Rust library and want Symposium to suggest a companion mod to users of your library, add the recommendation to your Cargo.toml:

```toml
[package]
name = "my-library"
version = "1.0.0"

# Shorthand syntax for crates.io mods:
[package.metadata.symposium]
recommended = ["my-mod", "another-mod"]

# Full syntax for other sources:
[[package.metadata.symposium.recommended]]
source.cargo = "some-mod"

[[package.metadata.symposium.recommended]]
source.registry = "some-acp-mod"

[[package.metadata.symposium.recommended]]
source.url = "https://example.com/mod.json"
```

Users who depend on your library will see these suggestions in Symposium.

### Central Recommendations

If you want to recommend a mod for a crate you don't maintain (and can't add Cargo.toml metadata to), submit a PR to [symposium-dev/recommendations](https://github.com/symposium-dev/recommendations):

```toml
[[recommendation]]
source.cargo = "my-mod"
when.using-crate = "their-library"

# Or for multiple trigger crates:
[[recommendation]]
source.cargo = "my-mod"
when.using-crates = ["their-library", "their-library-derive"]
```

This tells Symposium: "If a project depends on `their-library`, suggest `my-mod`."

## Reference

A recommendation has two fields:

- **source** - where to find the agent mod to install
- **when** - conditions for when to suggest the mod (optional)

```toml
[[recommendation]]
source.cargo = "my-mod"        # required: where to find the mod
when.using-crate = "tokio"     # optional: when to suggest it
```

### Source Types

| Source | Syntax | Description |
|--------|--------|-------------|
| crates.io | `source.cargo = "name"` | Rust crate installed via cargo |
| ACP Registry | `source.registry = "id"` | Mod from the ACP registry |
| Direct URL | `source.url = "https://..."` | Direct link to mod.json |
| Built-in | `source.builtin = "name"` | Symposium built-in mod |

### Conditions

The `when` field controls when the recommendation is shown:

| Condition | Syntax | Description |
|-----------|--------|-------------|
| Using a crate | `when.using-crate = "tokio"` | Project depends on this crate |
| Using any of several crates | `when.using-crates = ["tokio", "async-std"]` | Project depends on any of these |
| File exists | `when.file-exists = "Cargo.toml"` | File exists in the workspace |
| Any file exists | `when.files-exist = ["Cargo.toml", "Cargo.lock"]` | Any of these files exist |

Without a `when` clause, the recommendation is always shown (unconditional).

### How Recommendations Are Loaded

Symposium loads recommendations from multiple sources and merges them:

1. **Remote recommendations** - downloaded from `recommendations.symposium.dev` and cached locally
2. **User's local recommendations** - from your config directory
3. **Workspace recommendations** - from `.symposium/recommendations.toml` in the project

All sources are merged, and conditions are evaluated against the current workspace.

Remote recommendations are cached so Symposium works offline:

- On success: the downloaded file is cached to `<config_dir>/cache/recommendations.toml`
- On failure: the cached version is used (if available)
- If no cache exists and download fails: Symposium shows an error
