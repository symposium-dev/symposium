# Supporting your crate

If you maintain a Rust crate, you can extend Symposium with skills, MCP servers, or other extensions that will teach agents the best way to use your crate.

## Embed skills in your crate

The recommended approach is to ship skills directly in your crate's source tree. Add a `skills/` directory with `SKILL.md` files, then add a small plugin manifest to our [central recommendations repository](https://github.com/symposium-dev/recommendations). Users will get guidance that matches the exact version of your crate they're using.

See [Authoring a plugin](./authoring-a-plugin.md) for the full walkthrough.

## Want to write a skill for someone else's crate?

We prefer crates to ship their own skills, but some crates may not want to or may not be actively maintained. We also accept skills for those crates to help bootstrap the ecosystem. External skills must be uploaded directly into our [central recommendations repository](https://github.com/symposium-dev/recommendations) so that we can vet them.

See [Authoring a plugin](./authoring-a-plugin.md) for the details.

## Moar power!

Beyond skills, there are two more extension types you can publish through a plugin:

- **Hooks** — checks and transformations that run when the AI performs certain actions, like writing code or running commands.
- **MCP servers** — tools and resources exposed to agents via the Model Context Protocol.

See [Authoring a plugin](./authoring-a-plugin.md) for how to set one up.
