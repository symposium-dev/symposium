# Supporting your crate

If you maintain a Rust crate, you can extend Symposium with skills, MCP servers, or other extensions that will teach agents the best way to use your crate.

## Embed skills in your crate

The recommended approach is to ship skills directly in your crate's source tree. Add a `SYMPOSIUM.toml` and a `skills/` directory — users will get guidance that matches the exact version of your crate they're using.

### Minimal setup (skills only)

If you just want to ship skills, create a `skills/` directory with `SKILL.md` files:

```
my-crate/
    Cargo.toml
    src/
        lib.rs
    skills/
        basics/
            SKILL.md
        advanced-patterns/
            SKILL.md
```

No `SYMPOSIUM.toml` needed — Symposium discovers `SKILL.md` files recursively under `skills/` when no manifest is present. This is the zero-ceremony path for crate authors.

### With a manifest (recommended)

For more control, add a `SYMPOSIUM.toml` at your crate root:

```
my-crate/
    Cargo.toml
    SYMPOSIUM.toml
    src/
        lib.rs
    skills/
        basics/
            SKILL.md
        advanced-patterns/
            SKILL.md
```

```toml
# SYMPOSIUM.toml
[[skills]]
source.path = "skills"
```

The `name` defaults to your crate name, and `crates` defaults to `["*"]` (always active). With a manifest you can also add hooks, MCP servers, predicates, and more — see [Authoring a plugin](./authoring-a-plugin.md).

### Custom skills directory

Want to use a directory other than `skills/`? Just point to it in your manifest:

```toml
# SYMPOSIUM.toml
[[skills]]
source.path = "docs/agent-skills"
```

## How users get your skills

Users get your crate's skills in two ways:

1. **Explicit install** — `cargo agents install my-crate` fetches and scans your crate.
2. **Allow-list discovery** — If your crate is listed in a `dependency-allow-list` (e.g., in `symposium-recommendations`), users who depend on your crate get skills automatically.

To get your crate added to the default allow list, submit a PR to the `symposium-recommendations` crate.

## Redirecting to a companion crate

If you don't want to include plugin content in your main crate (to keep it lean), you can redirect via `[[auto-install]]`:

```toml
# SYMPOSIUM.toml in my-crate (minimal, no skills of its own)
[[auto-install]]
crates = { symposium-my-crate = "1.0" }
```

This tells Symposium: "when you install me, also install `symposium-my-crate` which has the actual plugins."

## Want to write a skill for someone else's crate?

We prefer crates to ship their own skills, but some crates may not want to or may not be actively maintained. You can publish a standalone plugin crate (e.g., `symposium-serde`) and submit it for inclusion in the `symposium-recommendations` allow list.

See [Authoring a plugin](./authoring-a-plugin.md) for the details.

## Moar power!

Beyond skills, there are two more extension types you can publish through a plugin:

- **Hooks** — checks and transformations that run when the AI performs certain actions, like writing code or running commands.
- **MCP servers** — tools and resources exposed to agents via the Model Context Protocol.

See [Authoring a plugin](./authoring-a-plugin.md) for how to set one up.
