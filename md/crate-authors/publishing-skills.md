# Publishing skills

Skills are guidance documents that teach AI assistants how to use your crate. When a user's project depends on your crate, Symposium loads your skills automatically.

## Background: what is a skill

As defined on [agentskills.io](https://agentskills.io/), a *skill* is a directory that contains a `SKILL.md` file, along with potentially other content. The `SKILL.md` file defines metadata to help the agent decide when the skill should be activated; activating the skill reads the rest of the file into context.

## Define the *target crates* for a skill with `crates` frontmatter

Symposium extends the Skill frontmatter with one additional field, `crates`. This field specifies a predicate indicating the crates that the skill applies to; this predicate can also specify versions of the create, if that is relevant:

```markdown
---
name: widgetlib-basics
description: Basic guidance for widgetlib usage
crates: widgetlib
---

Prefer using `Widget::builder()` over constructing widgets directly.
Always call `.validate()` before passing widgets to the runtime.
```

The `crates: widgetlib` in this example says that this skill should be installed in any project that depends on any version of `widgetlib`.

If you wanted to limit your skill to v2.x of `widgetlib`, you could write this:

```
crates: widgetlib=2.0
```

You can read more about crate predicates in the plugin documentation.

## Publishing skills

You can publish skills for your crate in two ways:

* You can upload **standalone skill directories** directly into the [symposium-dev/recommendations][rr], as described below (you can also publish them to a [custom plugin source][ps] in the same fashion).
* Or, you can [**create a plugin**](./creating-a-plugin.md), which is a TOML file that defines where to find skills, MCP servers, etc. In this case, the skills can be hosted either on the central recommendations repository or on your own repositorys. See the [creating a plugin](./creating-a-plugin.md) chapter for more details.

[rr]: https://github.com/symposium-dev/recommendations

## Example: publishing 3 standalone skills for widgetlib

As an example, consider a hypothetical crate `widgetlib` that wishes to post 3 standalone skills into the central repository. These skills support using widgetlib 1.x, widgetlib 2.x (which is quite different), and how to upgrade from 1.x to 2.x (which is nontrivial).

To do this, you would add the following directories to the [recommendations repo][rr] or your own [custom plugin source][ps]:

[ps]: ../custom-plugin-source.md

```
widgetlib/
    1x-basics/
        SKILL.md, containing:
           ---
           name: widgetlib-1x-basics
           description: How to use widgetlib 1.x
           crates: widgetlib=1.0
           ---
    2x-basics/
        SKILL.md, containing:
           ---
           name: widgetlib-2x-basics
           description: How to use widgetlib 2.x
           crates: widgetlib=2.0
           ---
    upgrade-1x-to-2x/
        SKILL.md, containing:
           ---
           name: widgetlib-upgrade-1x-to-2x
           description: How to upgrade from widgetlib 1.x to 2.x
           crates: widgetlib=1.0
           ---
```

Each skill uses `crates` to limit itself to specific versions.

## Reference: common skill frontmatter fields

| Field | Description | Symposium specific? |
|-------|-------------|---|
| `name` | Skill identifier. | |
| `description` | Short description shown in skill listings. | |
| `crates` | Which crate(s) this skill is about. Comma-separated: `crates: serde, serde_json`. | Yes |
| `compatibility` | English-language list of agents or editors this skill works with, if it doesn't apply universally. See the [compatibility field spec](https://agentskills.io/specification#compatibility-field). |

See the [Skill definition reference](../reference/skill-definition.md) for the full format, the [Skill matching reference](../reference/skill-matching.md) for version constraint syntax, and the [agentskills.io quickstart](https://agentskills.io/skill-creation/quickstart) for general guidance on writing effective skills.
