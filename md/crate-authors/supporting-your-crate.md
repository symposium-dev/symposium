# Supporting your crate

If you maintain a Rust crate, you can extend Symposium with skills, MCP servers, or other extensions that will teach agents the best way to use your crate.

## Just want to add a skill?

If all you need is to publish guidance for your crate, you don't need to set up a plugin. Just write a `SKILL.md` with a few lines of frontmatter (name, description, which crate it's for) and a markdown body, then open a PR to the [symposium-dev/recommendations](https://github.com/symposium-dev/recommendations) repository.

See [Publishing skills](./publishing-skills.md) for the details.

## Moar power!

There are three kinds of extensions you can publish:

- **Skills** — guidance documents that AI assistants receive automatically when a user's project depends on your crate.
- **Hooks** — checks and transformations that run when the AI performs certain actions, like writing code or running commands.
- **MCP servers** — tools and resources exposed to agents via the Model Context Protocol.

See [Creating a plugin](./creating-a-plugin.md) for how to set one up.
