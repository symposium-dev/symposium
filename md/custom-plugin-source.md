# Custom plugin sources

Custom plugin sources let you define your own sets of plugins without putting them in the [central recommendations repository][rr].

[rr]: https://github.com/symposium-dev/recommendations

Custom plugin sources are useful for:

- **Company-specific plugins** — internal tools and guidelines for your organization
- **Development plugins** — local plugins you're working on

## Custom plugins in your home directory

[plugin definitions](./reference/plugin-definitions.md) or [standalone skills](./reference/skill-definitions.md) added to the `~/.symposium/plugins` directory will be registered by default and propagated appropriately to your other projects.

## Adding your own custom sources

You can also define a custom plugin source in a git repository or at another path on your system.

### Git repository

Add a remote Git repository as a plugin source:

```toml
# In ~/.symposium/config.toml
[[plugin-source]]
name = "my-company"
git = "https://github.com/mycompany/symposium-plugins"
auto-update = true
```

We recommend creating a CI tool that runs [`symposium plugin validate`](./reference/cargo-agents-plugin.md) on your repository with every PR to ensure it is properly formatted.

### Local directory

Add a local directory as a plugin source:

```toml
[[plugin-source]]
name = "local-dev"
path = "./my-plugins"
auto-update = false
```

## Structure of a plugin source

See the [reference section](./reference/plugin-source.md) for details on what a plugin source looks like.

## Managing sources

The [`symposium plugin`](./reference/cargo-agents-plugin.md) command allows you to perform operatons on the installed plugin sources, like synchronizing their contents or validating their structure.
