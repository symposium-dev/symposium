# Plugin sources

A **plugin source** is a directory or repository containing plugins and standalone skills that Symposium discovers automatically. Plugin sources can be local directories or remote Git repositories, and Symposium searches them recursively to find all available extensions.

## Discovery rules

Symposium scans a plugin source recursively to find plugins and standalone skills:

* A [plugin](./plugin-definition.md) is a directory that contains a `Symposium.toml` file;
* A directory that contains a `SKILL.md` and no `Symposium.toml` is loaded as a plugin with default values — named for the skill's frontmatter `name`, with the skill's `depends-on` acting as the plugin's activation gate. `depends-on` is optional: a skill that names no dependency loads *dormant* (it activates once you enable it by name with [`cargo agents use`](./cargo-agents-use.md)), exactly like a gateless `Symposium.toml` plugin.

**We do not allow these entries to be nested within one another.** When we find a directory that is either a plugin or a skill, we do not search its contents any further.

### Example structure

```text
plugin-source/
  my-plugin/
    Symposium.toml        # ✓ Plugin
    skills/               # ✗ Not searched (parent claimed)
      basic/
        SKILL.md
  serde-skill/
    SKILL.md              # ✓ Standalone skill
  nested/
    deep/
      tokio-skill/
        SKILL.md          # ✓ Standalone skill (found recursively)
  mixed/
    Symposium.toml        # ✓ Treated as plugin
    SKILL.md              # ✗ Ignored (plugin takes precedence)
```

## Configuration

Plugin sources are configured in your `config.toml` file. See the [Configuration reference](./configuration.md) for details on setting up local directories, Git repositories, and built-in sources.

## Validation

You can validate a plugin source directory:

```bash
# Validate all plugins and skills in a directory
cargo agents plugin validate path/to/plugin-source/

# Also verify that crate names exist on crates.io (on by default; use --no-check-crates to skip)
cargo agents plugin validate path/to/plugin-source/ --no-check-crates
```

This scans the directory, attempts to load all plugins and skills, and reports any errors found.
