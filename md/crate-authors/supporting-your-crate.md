# Supporting your crate

If you maintain a Rust crate, you can extend Symposium with skills, MCP servers, or other extensions that will teach agents the best way to use your crate.

## Embed skills in your crate

The recommended approach is to ship skills directly in your crate's source tree. Add a `skills/` directory with `SKILL.md` files and publish your crate. Users who depend on it directly are asked whether to enable its plugin when they run `cargo agents sync`, and then get guidance that matches the exact version of your crate they're using. An optional entry in our [central recommendations repository](https://github.com/symposium-dev/recommendations) makes it load without asking.

See [Authoring a plugin](./authoring-a-plugin.md) for the full walkthrough.

## Skill layout metadata

By default Symposium looks in your crate's `skills/` directory. To customize the layout (a different subdirectory, several skill groups, a git source, or delegating to another crate), you describe your crate's plugin inline under `[package.metadata.symposium]` in `Cargo.toml`. **This block uses the exact same schema as a [`SYMPOSIUM.toml` plugin manifest](../reference/plugin-definition.md)**: it is just that manifest embedded in `Cargo.toml`:

```toml
# Optional — absence means "look in skills/ by default"
[[package.metadata.symposium.skills]]
source.path = "guidance"   # custom subdirectory for skills
```

Because it is a plugin manifest, the same rules apply as for a crate-embedded `SYMPOSIUM.toml`: `name` defaults to the crate, a top-level `depends-on` is unnecessary (reaching your crate, through the user's enablement or a chained reference, is the gate), and the default `skills/` group is appended unless you opt out with `[package.metadata.symposium.defaults] skills = false`.

If you ship *both* a `[package.metadata.symposium]` block and a `SYMPOSIUM.toml`, they are combined — list entries (skill groups, chained references, …) from both are kept; where the two set the same scalar, the `SYMPOSIUM.toml` file wins.

### Resolution rules

1. **No metadata section and no `SYMPOSIUM.toml`**: Symposium uses the default `skills/` subdirectory.
2. **Opt out of the default group**: `[package.metadata.symposium.defaults] skills = false` and declare no skill groups: no skills from this crate.
3. **`source.path` groups**: look in that subdirectory of your crate's source, in addition to the default `skills/` group unless you opt out of it.
4. **`[[package.metadata.symposium.plugins]]` chained references**: load another crate's plugin (see [Delegating to another crate](#delegating-to-another-crate)).

### Delegating to another crate

A `[[package.metadata.symposium.plugins]]` chained reference lets your crate delegate skill hosting to another crate. This is useful when:

- Your main crate is small but skills live in a larger companion package.
- Multiple crates in a workspace want to share a single set of skills.
- You want to version skills separately from the library.

```toml
# In dial9-tokio-telemetry/Cargo.toml
[[package.metadata.symposium.plugins]]
source.cargo = "dial9-viewer"   # or { name = "dial9-viewer", version = ">=1.0" }
```

A chained reference can target any crate, not just workspace dependencies. The referenced crate is itself resolved as a plugin (its own metadata / `SYMPOSIUM.toml` / default `skills/`), so delegation composes transitively. When a user enables your crate, the crates it delegates to load with it.

Cycles are safe: a crate reached a second time is skipped, so A -> B -> A loads each crate once. Crate name comparison is hyphen/underscore-insensitive (`my-crate` and `my_crate` are the same crate).

### Edge cases

- **Malformed metadata**: If `[package.metadata.symposium]` or `SYMPOSIUM.toml` doesn't parse as a manifest (wrong types, unknown fields), Symposium ignores that layer and still resolves the rest (at minimum the default `skills/` group). A manifest that parses but fails validation, for example a hook naming an installation that doesn't exist, disables your crate's whole plugin, default `skills/` group included. Both problems are reported only in Symposium's log (`~/.symposium/logs/`), so [try your plugin in a project](./authoring-a-plugin.md#step-4-try-it-in-a-project) before publishing.
- **Missing path directory**: If a `source.path` group references a directory that doesn't exist, Symposium silently produces zero skills for that group. Other groups are still processed.
- **Diamond references**: If multiple crates all delegate to the same target crate, the target loads once and its skills are installed once (a skill's identity is the location of its `SKILL.md`, not who referenced it).

## Want to write a skill for someone else's crate?

We prefer crates to ship their own skills, but some crates may not want to or may not be actively maintained. We also accept skills for those crates to help bootstrap the ecosystem. External skills must be uploaded directly into our [central recommendations repository](https://github.com/symposium-dev/recommendations) so that we can vet them.

See [Authoring a plugin](./authoring-a-plugin.md) for the details.

## Moar power!

Beyond skills, there are three more extension types you can publish through a plugin:

- **Hooks**: checks and transformations that run when the AI performs certain actions, like writing code or running commands.
- **MCP servers**: tools and resources exposed to agents via the Model Context Protocol.
- **Subcommands**: commands that users and agents run as `cargo agents <name>`.

See [Authoring a plugin](./authoring-a-plugin.md) for how to set one up.
