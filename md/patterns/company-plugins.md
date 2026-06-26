# Company-wide plugins

Distribute internal guidelines, hooks, and tools to your team via a private plugin crate.

## Setup

### 1. Create a plugin crate

```bash
cargo new my-company-plugins --lib
```

Add `SYMPOSIUM.toml` files for each set of guidelines you want to distribute:

```
my-company-plugins/
  Cargo.toml
  internal-api/
    SYMPOSIUM.toml
    skills/
      error-handling/
        SKILL.md
      auth-patterns/
        SKILL.md
  observability/
    SYMPOSIUM.toml
    skills/
      logging/
        SKILL.md
```

Each `SYMPOSIUM.toml` can use predicates to target specific crates:

```toml
# internal-api/SYMPOSIUM.toml
crates = ["my-company-api"]

[[skills]]
source.path = "skills"
```

### 2. Publish privately

Publish to your private registry, or host in a git repository your team can access.

### 3. Team members add the plugin

```bash
# From a private registry
cargo agents use my-company-plugins

# Or from git
cargo agents use --git https://github.com/my-company/symposium-plugins
```

## Auto-discovery for internal crates

If your internal crates ship their own `SYMPOSIUM.toml`, you can add them to the allow list so team members get skills automatically when they depend on them:

```toml
# In my-company-plugins/SYMPOSIUM.toml
[discovery.allow]
crates = { my-company-api = "*", my-company-auth = "*", my-company-db = "*" }
```

Or team members can opt in globally:

```toml
# In ~/.symposium/config.toml
[discovery]
allow = "*"
```

## Hooks for enforcement

Beyond skills (guidance), you can add hooks to enforce standards:

```toml
# internal-api/SYMPOSIUM.toml
crates = ["my-company-api"]

[[skills]]
source.path = "skills"

[[hooks]]
name = "check-error-handling"
event = "PreToolUse"
matcher = "Bash"
command = "company-linter"
```

## Keeping everyone up to date

Team members tracking latest (`cargo agents use my-company-plugins` without `@`) automatically get updates on sync. For controlled rollouts, have team members pin to a version:

```bash
cargo agents use my-company-plugins@1
```

They'll get patch/minor updates within 1.x but won't jump to 2.x until they explicitly upgrade.
