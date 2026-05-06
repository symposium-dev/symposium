<div class="home-hero">
    <div class="home-hero-media">
        <div class="home-hero-logo-panel">
            <img src="./artwork/symposium5_vase-ferris.svg" alt="Symposium logo" class="home-hero-logo"/>
        </div>
    </div>
    <div class="home-hero-info">
        <p>
            Symposium makes Rust dependencies actionable for AI agents.
            It discovers crate-matched plugins and wires in skills, hooks,
            and MCP servers so your agent can work with project-specific context.
        </p>
        <div class="home-hero-links">
            <a href="./about.md">About</a>
            <span>⁄</span>
            <a href="./install.md">Install</a>
            <span>⁄</span>
            <a href="./blog/outline.md">Blog</a>
        </div>
    </div>
</div>

## Getting started

```bash
cargo binstall symposium # or: cargo install symposium
cargo agents init
```

After initialization, start your agent in a Rust project as usual.

- For first-time setup details, see [Installing Symposium](./install.md).
- For command reference, see [The `cargo agents` command](./reference/cargo-agents.md).
- For agent compatibility, see [Supported agents](./reference/supported-agents.md).

## Recent posts

<div class="home-posts">
    <a href="./blog/announcing-symposium.md" class="home-post-card">
        <span class="home-post-date">April 21, 2026</span>
        <span class="home-post-title">Announcing Symposium</span>
        <span class="home-post-desc">A launch overview of how Symposium loads crate-aware agent extensions and where the project is headed.</span>
    </a>
    <a href="./blog/outline.md" class="home-posts-more">View all posts →</a>
</div>
