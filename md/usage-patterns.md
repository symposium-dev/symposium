# Usage patterns

This page describes how Symposium fits into your day-to-day workflow.

## Skills activate automatically

When you ask your AI assistant about a crate in your project, Symposium checks your dependencies and loads matching skills. You don't need to do anything special — just work as you normally would.

For example, if your project depends on `tokio` and a skill exists for it, your assistant will receive that guidance whenever it's relevant.

## Getting guidance for a crate

To get guidance for a specific crate:

```bash
symposium crate tokio
```

## Hooks run in the background

If your agent supports hooks (e.g., Claude Code), Symposium can intercept events like tool use and apply checks automatically. Hooks are configured by plugins — you don't need to set them up yourself.

## Keeping things up to date

Plugin sources are checked for updates on startup. You can also update manually:

```bash
symposium plugin sync
```

This fetches the latest skills and hooks from all configured git-based plugin sources.

## Want to tweak how Symposium works?

[Check out the configuration section.](./reference/configuration.md)
