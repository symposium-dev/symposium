# `cargo agents search`

Find plugins across every configured registry.

## Usage

```bash
cargo agents search <QUERY>
```

## Options

| Flag | Description |
|------|-------------|
| `<QUERY>` | Name, or name fragment, to look for |

## Behavior

The query is a case-insensitive substring match — the same looseness
`cargo search` has. Results come from two arms and are printed grouped by the
instance each hit came from:

1. **Already loaded** — plugin and standalone-skill names in the plugin
   registry. A configured registry is a trust root, so a hit here is available
   now, with no `use` needed (unless the plugin is dormant, which is noted).
2. **Offered by a package manager** — each configured
   [registry's](./plugin-source.md) package manager is searched in turn.

A package manager without a searchable registry contributes nothing rather
than failing, and an instance that errors outright (an offline registry, say)
is skipped — so `search` degrades to the results it can get instead of failing
the command.

With `--json`, each hit is emitted as a `search_match` event carrying its
`origin`, `name`, and — where the registry provides them — `version` and
`description`.

## Example

```bash
$ cargo agents search widget
ℹ️  from user-plugins:
  widget-guidance
      Guidance for working with widgets
ℹ️  from symposium-recommendations:
  widget-skills 1.2.3
      Skills for widget
```

Pass a name from the output to [`cargo agents use`](./cargo-agents-use.md) to
enable it.
