# Recommending Agent Extensions

There are two ways to recommend agent extensions to users:

1. **Central recommendations** - submit to the Symposium recommendations registry
2. **Crate metadata** - add recommendations directly in your crate's Cargo.toml

## Extension Sources

Extensions are referenced using a `source` field:

| Source | Syntax | Description |
|--------|--------|-------------|
| crates.io | `source.cargo = "name"` | Rust crate installed via cargo |
| ACP Registry | `source.registry = "id"` | Extension from the ACP registry |
| Direct URL | `source.url = "https://..."` | Direct link to extension.json |

## Central Recommendations

Submit a PR to [symposium-dev/recommendations](https://github.com/symposium-dev/recommendations) adding an entry:

```toml
[[recommendation]]
source.cargo = "my-extension"
when.using-crate = "my-library"

# Or for multiple trigger crates:
[[recommendation]]
source.cargo = "my-extension"
when.using-crates = ["my-library", "my-library-derive"]
```

This tells Symposium: "If a project depends on `my-library`, suggest `my-extension`."

Users can also add their own local recommendation files for internal/proprietary extensions.

## Crate Metadata Recommendations

If you maintain a library, you can recommend extensions directly in your Cargo.toml. Users of your crate will see these suggestions in Symposium.

### Shorthand Syntax

For crates.io extensions:

```toml
[package.metadata.symposium]
recommended = ["some-extension", "another-extension"]
```

### Full Syntax

For extensions from other sources:

```toml
# Recommend a crates.io extension
[[package.metadata.symposium.recommended]]
source.cargo = "some-extension"

# Recommend an extension from the ACP registry
[[package.metadata.symposium.recommended]]
source.registry = "some-acp-extension"

# Recommend an extension from a direct URL
[[package.metadata.symposium.recommended]]
source.url = "https://example.com/extension.json"
```

### Example

If you maintain `tokio`, you might add:

```toml
[package]
name = "tokio"
version = "1.0.0"

[package.metadata.symposium]
recommended = ["symposium-tokio"]
```

Users who depend on tokio will see "Tokio Support" suggested in their Symposium settings.
