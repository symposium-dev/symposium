# Plugin sources

A **plugin source** is a directory or repository containing plugins and standalone skills that Symposium discovers automatically. Plugin sources can be local directories or remote Git repositories, and Symposium searches them recursively to find all available extensions.

## Discovery rules

Symposium scans a plugin source recursively to find plugins and standalone skills:

* A [plugin](./plugin-definition.md) is a directory that contains a `Symposium.toml` file;
* An [Agent Plugins package](./agent-plugins.md) is a directory that contains a `plugin.json` and no `Symposium.toml`. It is loaded as a plugin with one skill per direct child of its `skills/` folder, and stays dormant until you `use` it unless its manifest declares a [`dev.symposium` gate](./agent-plugins.md#telling-symposium-when-it-applies);
* A directory that contains a `SKILL.md` and neither of those manifests is loaded as a plugin with default values: named for the skill's frontmatter `name`, with the skill's `depends-on` acting as the plugin's activation gate. `depends-on` is optional: a skill that names no dependency loads *dormant* (it activates once you enable it by name with [`cargo agents use`](./cargo-agents-use.md)), exactly like a gateless `Symposium.toml` plugin.

**We do not allow these entries to be nested within one another.** When we find a directory that is a plugin, a package or a skill, we do not search its contents any further. The root of a plugin source cannot itself be one of them.

A directory that Symposium installed itself, marked by a `.symposium` file inside it, is skipped: installed copies are never read back as sources.

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
  pdf-tools/
    plugin.json           # ✓ Agent Plugins package
    skills/
      extract-tables/
        SKILL.md          # ✓ One of its skills
```

## Configuration

Plugin sources are configured in your `config.toml` file. See the [Configuration reference](./configuration.md) for details on setting up local directories, Git repositories, and built-in sources.

## Validation

You can validate a plugin source directory:

```bash
# Validate all plugins and skills in a directory
cargo agents plugin validate path/to/plugin-source/

# Validate a single entry
cargo agents plugin validate path/to/plugin-source/pdf-tools/

# Also verify that crate names exist on crates.io (on by default; use --no-check-crates to skip)
cargo agents plugin validate path/to/plugin-source/ --no-check-crates
```

This scans the directory, attempts to load all plugins and skills, and reports any errors found. A directory that is itself a plugin, a package or a skill is validated as that one entry.
