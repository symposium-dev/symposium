# Agent Plugins packages

Symposium loads packages in the [Agent Plugins](https://agent-plugins.org/) format, versions 1.0.0 and 1.1.0, next to its own [plugin definitions](./plugin-definition.md). One of these packages is a folder with a `plugin.json` manifest and a `skills/` folder, the same package Codex CLI or the Copilot CLI read, so a package written for those agents works with Symposium unchanged, and its skills and MCP servers reach every agent you have set up.

Symposium reads both component types of the format: skills, and MCP servers in an `mcp.json`. Hooks, subcommands, installations and custom predicates are not in the format at all; use a [`Symposium.toml`](./plugin-definition.md) plugin when you need those.

## Layout

```text
pdf-tools/
  plugin.json          required
  skills/              optional, fixed location
    extract-tables/
      SKILL.md
  mcp.json             optional, fixed location
```

The folder and file names are fixed by the format. `plugin.json` cannot point somewhere else, and cannot declare components inline.

## Where symposium looks

These packages are picked up in the same three places as a symposium plugin, and each place keeps its usual meaning:

| Where it sits | When it turns on |
|---------------|------------------|
| An entry in a configured [plugin source](./plugin-source.md) | Dormant until a [`use`](./cargo-agents-use.md) entry names it, unless it [declares a gate](#telling-symposium-when-it-applies) |
| A workspace member's folder | Whenever that project is in your workspace |
| A dependency's source | Offered for consent like any plugin found in a dependency, and enabled by the crate's name |

A folder holding both a `Symposium.toml` and a `plugin.json` is read as a symposium plugin, because `Symposium.toml` is the richer manifest. It takes its `name`, `version` and `description` from `plugin.json` where the TOML leaves them out, and ignores the rest of `plugin.json`, including its `dev.symposium` gate. Its `mcp.json` is not read either: list those servers as [`[[mcp_servers]]`](./plugin-definition.md#mcp_servers) in the TOML. A `plugin.json` that is not valid there is reported and the TOML plugin still loads. Such a folder in a plugin source gets no default skill group, so its `Symposium.toml` declares its own `[[skills]]` group for the skills it ships.

## Manifest

The manifest is closed, meaning only known fields are allowed:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `$schema` | string | yes | `https://agent-plugins.org/schemas/1.0.0/plugin.schema.json`, or the same with `1.1.0`. Any other value, including a newer or draft version of the format, rejects the package. |
| `name` | string | yes | The plugin name. See [Names](#names). |
| `version` | string | no | Shown by `search` and `status`. Not required to be a semantic version. |
| `description` | string | no | Shown by `search`. |
| `author` | object | no | Only `name`, `email` and `url`, each a string. |
| `homepage` | string | no | |
| `repository` | string | no | |
| `license` | string | no | |
| `keywords` | array of strings | no | |
| `extensions` | object | no | Client-specific data, keyed by a reverse-domain namespace. See [Telling symposium when it applies](#telling-symposium-when-it-applies). |

Fields are only checked for their JSON type: a version that is not semantic versioning, a URL or email that does not look like one, or a license that is not an SPDX identifier is accepted. `null` is not a string, so it does not satisfy a string field.

```json
{
  "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
  "name": "pdf-tools",
  "version": "1.2.0",
  "description": "Table extraction guidance",
  "author": {
    "name": "Alex Doe",
    "url": "https://example.com"
  },
  "license": "MIT",
  "keywords": ["pdf", "tables"]
}
```

### Names

A name is 1 to 64 characters of lowercase letters, digits, hyphens and periods. It starts and ends with a letter or digit, and does not contain `--` or `..`. `pdf-tools`, `acme.tools` and `a` are valid; `PDF-Tools`, `-tools` and `pdf--tools` are not.

## Telling symposium when it applies

The manifest has no way to say when a package applies, so one that comes from a plugin source stays dormant until you `use` it. To tie it to a dependency or a predicate while keeping it portable, say so under the `dev.symposium` key of `extensions`:

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

The two fields take the same syntax as the `depends-on` and `predicates` of a `Symposium.toml` plugin, and the same [dormancy rule](./plugin-definition.md#top-level-fields) applies: a package whose gate names no dependency stays dormant. See [Predicates](./predicates.md). Any other key under `dev.symposium`, or a value that does not parse, rejects the package, so a typo cannot make it apply more widely than you meant.

Other clients ignore keys they do not know, so this costs you nothing in portability. Symposium likewise ignores every other namespace under `extensions` without looking inside it.

## Skills

Every direct child of `skills/` holding a `SKILL.md` file is one skill, in the usual [skill format](./skill-definition.md). Deeper folders are not searched, and a `SKILL.md` at the package root is not a skill.

## MCP servers

`mcp.json` at the package root lists the package's MCP servers. Whenever the package applies, Symposium registers them in your agents like the servers of any other plugin (see [how registration works](./plugin-definition.md#how-registration-works)).

```json
{
  "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
  "mcpServers": {
    "pdf-tables": {
      "type": "stdio",
      "command": "./bin/pdf-server",
      "args": ["--config", "${PLUGIN_ROOT}/config.json"],
      "env": { "CACHE": "${PLUGIN_DATA}/cache" }
    },
    "pdf-api": {
      "type": "streamable-http",
      "url": "https://pdf.example.com/mcp"
    }
  }
}
```

The `$schema` names the same version of the format as `plugin.json`. Each server has a `type` and only the fields of that type:

| `type` | Fields |
|--------|--------|
| `stdio` | `command` (required), `args`, `env` |
| `streamable-http` | `url` (required), `headers` |
| `sse` | `url` (required), `headers` |

- A `command` is a bare name, which the agent looks up on its `PATH`, or a path starting with `./`, which must exist inside the package.
- In `args` and in the values of `env`, `${PLUGIN_ROOT}` becomes the package folder and `${PLUGIN_DATA}` a folder kept across updates of the package: `~/.symposium/plugin-data/<source>/<name>`, where `<source>` is the plugin source, `cargo` for a dependency (named by its crate), or `local` for a workspace member. The server also receives both as environment variables; a server whose `env` sets either one is skipped.
- A stdio server starts in the agent's working directory, not in the package folder, so one that sets `cwd` is skipped.
- A `url` uses `https`, or `http` on `localhost` or a loopback address, and has no user name, password or fragment. Headers must be valid HTTP header fields, and each name, compared without case, appears once.

## What happens when something is wrong

Problems are contained to the smallest part affected. Each one is reported as a warning by the command that loads the package, such as `sync`, `use` or `status`:

| Problem | Result |
|---------|--------|
| `plugin.json` is not valid JSON, misses `$schema` or `name`, has a field of the wrong type, or targets another version of the format | That package is rejected; a dependency carrying it offers no plugin. Others still load. |
| `extensions.dev.symposium` is not valid | That package is rejected. |
| An unknown top-level field, or an `extensions` that is not an object | Ignored. The package loads. |
| `skills` is not a folder | The package loads with no skills. |
| A `SKILL.md` at the package root | Not a skill, with a hint to move it under `skills/`. |
| One skill does not meet the skill format | That skill is skipped, and `cargo agents plugin validate` reports it. In a workspace member the usual [informal skill](../workspace-skills.md#informal-skills) rules apply instead. |
| `mcp.json` is not valid JSON, misses `$schema` or `mcpServers`, has another top-level field, or names another version than `plugin.json` | No MCP servers load from that package. Its skills still load. |
| One MCP server does not follow the [rules above](#mcp-servers), or has a `type` symposium does not know | That server is skipped. |

## Security

Symposium reads a package's manifest, skills and MCP servers only from inside the package folder. A `plugin.json`, a `skills` folder, an `mcp.json`, a skill's `SKILL.md` or a server's `./` command that resolves outside it, for instance through a link, is refused at the smallest level that applies: the whole package, its skills, its MCP servers, that one skill or that one server.

Installing one of its skills follows the links in it only as far as the package folder: a file or folder that resolves outside it is left out of the installed copy and reported, and the rest of the skill installs.

## Validation

```bash
cargo agents plugin validate path/to/pdf-tools
```

The path can be the package folder, its `plugin.json`, or a plugin source holding several packages. Warnings, such as an unknown field or a skipped MCP server, do not make the command fail; a rejected package or a broken skill does.
