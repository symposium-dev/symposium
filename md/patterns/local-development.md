# Local plugin development

Develop and test plugins locally before publishing.

## Quick start

Create a directory with your plugin content and install it as a path source:

```bash
mkdir ~/my-plugin
# Add SYMPOSIUM.toml and skills...
cargo agents install --path ~/my-plugin
```

Path sources always check mtime — changes are picked up immediately on the next sync, with no version bumping or re-install needed.

## Developing inside a crate

If you're building a plugin as a publishable crate, develop with the standard Cargo workflow:

```bash
cargo new my-symposium-plugin --lib
cd my-symposium-plugin

# Add your plugin content
mkdir skills
# ...

# Install locally for testing
cargo agents install --path .
```

When you're happy with it, publish to crates.io and switch to the published version:

```bash
cargo publish
cargo agents uninstall my-symposium-plugin
cargo agents install my-symposium-plugin
```

## Testing with `validate`

Check your plugin for errors without installing:

```bash
cargo agents plugin validate ./my-plugin/SYMPOSIUM.toml
```

This catches missing fields, bad predicates, and unreachable skill paths.

## Testing hooks locally

Use the hook CLI to test a hook with sample input:

```bash
echo '{"tool": "Bash", "input": "cargo test"}' | cargo agents hook claude pre-tool-use
```

## Iterating on skills

Since path sources check mtime, the workflow is:

1. Edit `SKILL.md` in your local plugin directory
2. Run `cargo agents sync` (or let auto-sync do it)
3. Check the result in your agent's skills directory

No reinstall, no version bump — just edit and sync.

## Structure for multiple plugins

A single local crate can contain multiple plugins by placing `SYMPOSIUM.toml` at different paths:

```
my-plugins/
  Cargo.toml
  serde-guidance/
    SYMPOSIUM.toml        # crates = ["serde"]
    skills/
      SKILL.md
  tokio-patterns/
    SYMPOSIUM.toml        # crates = ["tokio"]
    skills/
      SKILL.md
```

Each `SYMPOSIUM.toml` is discovered independently and can target different crates.
