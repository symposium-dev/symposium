# Agent Plugins packages

> Proposed reference page for the [Agent Plugins interoperability](./README.md) RFD. This is how `md/reference/` would read once support lands.

Symposium can load packages in the [Agent Plugins 1.0.0](https://agent-plugins.org/) format, next to its own [plugin definitions](../../reference/plugin-definition.md). One of these packages is a folder with a `plugin.json` manifest and a `skills/` folder.

Loading one gives you its skills in every agent you have set up, including the agents that cannot read the format themselves.

Symposium reads the skills half of the format. The format's other component type, MCP servers, is reported as unsupported. Declare those in a [`SYMPOSIUM.toml`](../../reference/plugin-definition.md) plugin.

## Layout

```text
pdf-tools/
  plugin.json          required
  skills/              optional, fixed location
    extract-tables/
      SKILL.md
```

The folder and file names are fixed by the format. `plugin.json` cannot point somewhere else, and cannot declare components inline.

## Where symposium looks

These packages are picked up in the same three places as a symposium plugin:

| Where it sits | When it turns on |
|---------------|------------------|
| An entry in a configured `[[registry]]` | Dormant until a `use` entry names it |
| A workspace member's folder | On whenever that project is in your workspace |
| A dependency's source | Offered for consent, like any plugin found in a dependency |

A folder holding both a `SYMPOSIUM.toml` and a `plugin.json` is read as a symposium plugin. `SYMPOSIUM.toml` is the richer manifest and wins.

## Manifest

The manifest is closed, meaning only known fields are allowed. `$schema` and `name` are required. The only other permitted fields are `version`, `description`, `author`, `homepage`, `repository`, `license`, `keywords`, and `extensions`. A name is 1 to 64 characters, lowercase letters and digits plus hyphens and periods, starting and ending with a letter or digit.

```json
{
  "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
  "name": "pdf-tools",
  "version": "1.2.0",
  "description": "Table extraction guidance",
  "license": "MIT"
}
```

## Telling symposium when it applies

The manifest has no way to say when a package applies, so one that comes from a registry stays dormant until you `use` it. To tie it to a dependency or a predicate while keeping it portable, say so under the `dev.symposium` key:

```json
{
  "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
  "name": "pdf-tools",
  "extensions": {
    "dev.symposium": {
      "depends-on": ["lopdf"],
      "predicates": ["path_exists(pdftotext)"]
    }
  }
}
```

Other clients must ignore keys they do not know, so this costs you nothing in portability. The fields take the same syntax as a `SYMPOSIUM.toml` plugin gate. See [Predicates](../../reference/predicates.md).

## Skills

Every direct child of `skills/` that holds a `SKILL.md` is one skill, in the usual [skill format](../../reference/skill-definition.md). Deeper folders are not searched. A broken skill — malformed frontmatter, or missing a field the skill format requires — is skipped and reported, and the rest of the package still loads.

Where those skills end up per agent is covered in [How extensions are installed](./proposed-install.md).

## What the format cannot carry

Hooks, subcommands, installations, and custom predicates are not in the format, so one of these packages cannot declare them. Use a [`SYMPOSIUM.toml`](../../reference/plugin-definition.md) plugin when you need those.

## What happens when something is wrong

Problems are contained to the smallest part affected, and reported rather than hidden:

| Problem | Result |
|---------|--------|
| `plugin.json` breaks its schema | That package is rejected. Others in the registry still load. |
| An unknown top-level field in the manifest | Reported and ignored. The package loads. |
| One skill is broken | That skill is skipped. The others load. |
| A path that resolves outside the package folder | Access is denied, at the smallest level that applies. |
| An `mcp.json` is present | Reported as unsupported. Skills still load. |

## Validation

```bash
cargo agents plugin validate path/to/pdf-tools
```

Reports each problem at the level where it would take effect, so you can tell a rejected package from a skipped skill.
