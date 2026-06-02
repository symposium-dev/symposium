# Supporting your crate

If you maintain a Rust crate, you can extend Symposium with skills, MCP servers, or other extensions that will teach agents the best way to use your crate.

## Embed skills in your crate

The recommended approach is to ship skills directly in your crate's source tree. Add a `skills/` directory with `SKILL.md` files, then add a small plugin manifest to our [central recommendations repository](https://github.com/symposium-dev/recommendations). Users will get guidance that matches the exact version of your crate they're using.

See [Authoring a plugin](./authoring-a-plugin.md) for the full walkthrough.

## Skill layout metadata

You can control where Symposium looks for skills (and redirect to other crates) by adding `[package.metadata.symposium]` to your `Cargo.toml`:

```toml
# Optional — absence means "look in skills/ by default"
[[package.metadata.symposium.skills]]
path = "guidance"   # custom subdirectory for skills

[[package.metadata.symposium.skills]]
crate = { name = "other-crate", version = ">=1.0" }  # redirect to another crate
```

Each `[[package.metadata.symposium.skills]]` entry specifies either `path` (a subdirectory of your crate source) or `crate` (a redirect to another crate). The two are mutually exclusive within a single entry.

### Resolution rules

1. **No metadata section** — Symposium falls back to the default `skills/` subdirectory.
2. **Metadata present but `skills = []`** — no skills from this crate (NOT a fallback to `skills/`).
3. **`path` entries** — look in that subdirectory of your crate's source.
4. **`crate` entries** — fetch that crate's source and follow its metadata recursively.

### Redirects

Redirects allow a crate to delegate skill hosting to another crate. This is useful when:

- Your main crate is small but skills live in a larger companion package.
- Multiple crates in a workspace want to share a single set of skills.
- You want to version skills separately from the library.

```toml
# In dial9-tokio-telemetry/Cargo.toml
[[package.metadata.symposium.skills]]
crate = { name = "dial9-viewer" }
```

Redirects can target any crate, not just workspace dependencies. If the target isn't in the workspace, Symposium fetches it from the registry using the specified version constraint (or latest if omitted).

Cycle detection prevents infinite loops (A → B → A stops and warns). Redirect chains are capped at 10 hops. Crate name comparison is hyphen/underscore-insensitive (`my-crate` and `my_crate` are the same crate for cycle detection purposes).

### Edge cases

- **Malformed metadata** — If `[package.metadata.symposium]` is present but unparseable (wrong types, missing fields), Symposium logs a warning and falls back to the default `skills/` subdirectory. Fix any warnings to ensure your intended layout is respected.
- **Missing path directory** — If a `path` entry references a directory that doesn't exist, Symposium silently produces zero skills for that entry. Other entries in the same metadata section are still processed.
- **Diamond redirects** — If multiple crates in the workspace all redirect to the same target crate, the target's skills are installed once (deduplication is based on crate name + version, not who redirected).

## Want to write a skill for someone else's crate?

We prefer crates to ship their own skills, but some crates may not want to or may not be actively maintained. We also accept skills for those crates to help bootstrap the ecosystem. External skills must be uploaded directly into our [central recommendations repository](https://github.com/symposium-dev/recommendations) so that we can vet them.

See [Authoring a plugin](./authoring-a-plugin.md) for the details.

## Moar power!

Beyond skills, there are two more extension types you can publish through a plugin:

- **Hooks** — checks and transformations that run when the AI performs certain actions, like writing code or running commands.
- **MCP servers** — tools and resources exposed to agents via the Model Context Protocol.

See [Authoring a plugin](./authoring-a-plugin.md) for how to set one up.
