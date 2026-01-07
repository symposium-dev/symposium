# Ferris

Ferris provides tools for inspecting Rust crate source code, helping your agent understand actual implementations rather than guessing at APIs.

## Quick Reference

| What | How |
|------|-----|
| Fetch crate sources | Agent uses `crate_sources` tool |
| Check workspace version | Automatic - defaults to version in your Cargo.toml |
| Specify version | Agent can request specific versions or semver ranges |

## How It Works

When your agent needs to understand how a crate works, Ferris can fetch the source code directly from crates.io. This is useful when:

- Working with an unfamiliar crate
- Checking exact API signatures
- Understanding internal implementation details
- Finding usage examples in the crate's own code

## Tips

**Encourage source checking** - If Claude seems uncertain about a crate's API or is making incorrect assumptions, prompt it to "check the sources" for that crate. This often leads to more accurate code.

**Version awareness** - Ferris automatically uses the crate version from your workspace's Cargo.toml. If you need a different version, you can ask for a specific version or semver range.

## Future Plans

Ferris is a work in progress. Future versions will include guidance on strong Rust coding patterns to help your agent write more idiomatic Rust.
