# Plugin sources

A **plugin source**, configured as a [`[[registry]]`](./configuration.md#registry) entry and called a *registry* in the rest of these docs, is a directory or git repository containing plugins and standalone skills that Symposium discovers automatically. Registries can be local directories or remote Git repositories, and Symposium searches them recursively to find all available extensions.

## Discovery rules

Symposium scans a registry recursively to find plugins and standalone skills:

* A [plugin](./plugin-definition.md) is a directory that contains a `SYMPOSIUM.toml` file. A registry plugin gets no default `skills/` group: its skills load only through the `[[skills]]` groups its manifest declares (for example `source.path = "skills"`).
* A directory that contains a `SKILL.md` and no `SYMPOSIUM.toml` is loaded as a plugin with default values: named for the skill's frontmatter `name` (required, as for every registry skill), with the skill's frontmatter `depends-on` and `predicates` acting as the plugin's activation gate. `depends-on` is optional: a skill that names no dependency loads *dormant* (it activates once you enable it by name with [`cargo agents use`](./cargo-agents-use.md)), exactly like a gateless `SYMPOSIUM.toml` plugin.

**We do not allow these entries to be nested within one another.** When we find a directory that is either a plugin or a skill, we do not search its contents any further. The registry root itself must not be an entry: a root that contains a `SYMPOSIUM.toml` or a `SKILL.md` is an error (`cargo agents plugin validate` fails, and a configured registry loads nothing). Put each plugin or skill in a subdirectory instead.

### Example structure

```text
plugin-source/
  my-plugin/
    SYMPOSIUM.toml        # plugin
    skills/               # not a separate entry (loaded through my-plugin's [[skills]] groups)
      basic/
        SKILL.md
  serde-skill/
    SKILL.md              # standalone skill
  nested/
    deep/
      tokio-skill/
        SKILL.md          # standalone skill (found recursively)
  mixed/
    SYMPOSIUM.toml        # plugin
    SKILL.md              # ignored (the plugin takes precedence)
```

## Configuration

Registries are configured in your `config.toml` file. See the [Configuration reference](./configuration.md) for details on setting up local directories, Git repositories, and built-in sources.

## Validation

You can validate a registry directory:

```bash
# Validate all plugins and skills in a directory, checking that crate names exist on crates.io
cargo agents plugin validate path/to/plugin-source/

# Same, but skip the crates.io check
cargo agents plugin validate path/to/plugin-source/ --no-check-crates
```

This scans the directory, attempts to load all plugins and skills, and reports any errors found.
