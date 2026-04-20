# Plugin sources

A **plugin source** is a directory or repository containing plugins and standalone skills that Symposium discovers automatically. Plugin sources can be local directories or remote Git repositories, and Symposium searches them recursively to find all available extensions.

## Discovery rules

Symposium scans a plugin source recursively to find plugins and standalone skills:

* A [plugin](./plugin-definition.md) is a directory that contains a `SYMPOSIUM.toml` file;
* A [standalone skill](./skill-definition.md) is a directory that contains a `SKILL.md` and does not contain a `SYMPOSIUM.toml` file. Standalone skills must have `crates` metadata in their frontmatter.

**We do not allow plugins or standalone skills to be nested within one another.** When we find a directory that is either a plugin or a skill, we do not search its contents any further.

### Example structure

```text
plugin-source/
  my-plugin/
    SYMPOSIUM.toml        # ✓ Plugin
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
    SYMPOSIUM.toml        # ✓ Treated as plugin
    SKILL.md              # ✗ Ignored (plugin takes precedence)
```

## Configuration

Plugin sources are configured in your `config.toml` file. See the [Configuration reference](./configuration.md) for details on setting up local directories, Git repositories, and built-in sources.

## Validation

You can validate a plugin source directory:

```bash
# Validate all plugins and skills in a directory
symposium plugin validate path/to/plugin-source/

# Also verify that crate names exist on crates.io (on by default; use --no-check-crates to skip)
symposium plugin validate path/to/plugin-source/ --no-check-crates
```

This scans the directory, attempts to load all plugins and skills, and reports any errors found.
