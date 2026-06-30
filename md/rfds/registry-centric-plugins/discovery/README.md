# Discovery policy

## TL;DR

Add an allow/deny policy that controls which workspace dependencies are automatically loaded as plugins. Default deny-all. Policy can come from user config or from already-loaded plugins.

## Change in a nutshell

Workspace dependencies are not automatically loaded as plugins — that would mean any crate in your `Cargo.lock` could inject agent behavior. Instead, a discovery policy controls which dependencies are eligible.

**Default: deny all.** No workspace dependency becomes a plugin unless explicitly allowed.

### Allow/deny rules

Policy can be declared in two places:

1. In the user's `~/.symposium/config.toml`:

```toml
# Allow everything (opt-in to full auto-discovery)
[discovery]
allow = "*"

# Or be selective
[discovery.allow]
cargo = { my-internal-api = "*", my-internal-db = "*" }

# Block specific packages even if otherwise allowed
[discovery.deny]
cargo = { abandoned-crate = "*" }
```

2. In a plugin's `SYMPOSIUM.toml` (plugin-provided policy):

```toml
# symposium-recommendations can vouch for packages it knows about
[discovery.allow]
cargo = { serde = "*", tokio = "*", axum = "*" }
```

**Important:** Plugin-provided allow/deny rules are only processed from plugins with `used` provenance. A discovered dependency cannot vouch for other dependencies — only explicitly installed plugins (from config) can influence what gets discovered. This prevents a dependency from bootstrapping its own transitive discovery.

### Precedence

A specific rule beats a wildcard rule. If allow and deny have the same specificity, deny wins. This means an explicit deny always blocks, even if a plugin's allow rule covers the crate.

### Why plugin-provided policy?

This is the key to the curated experience. `symposium-recommendations` can vouch for crates whose authors have added `SYMPOSIUM.toml` files, without requiring every user to manually allow-list them. Enterprise teams can publish their own curator crate that allow-lists internal dependencies.

**Forward reference to a future RFD.** Note that [custom registry implementations](../custom-registries/README.md) will control how allow-list values are interpreted — the `discover` operation receives the accumulated policy and decides what it means. This gives registry authors another knob of control (e.g., an enterprise registry could interpret `allow = "*"` as "allow anything from our internal mirror only").

### Relationship to `source = "crate"` (removed)

Previously, `source = "crate"` on a skill group was how a plugin opted in to scanning workspace deps. This conflated activation gating, source resolution, and discovery authorization into one construct. Discovery policy replaces the authorization aspect cleanly.

## Frequently asked questions

### Why are allow-lists only honored from `used`-provenance plugins?

This is a security consideration. Discovery controls which code gets fetched and executed (via hooks, MCP servers, installations). We want it to be straightforward to audit what will be loaded: you look at your config (`[[plugins]]` entries) and the allow-lists declared by those plugins. That's a small, explicit set.

If discovered dependencies could themselves expand the allow-list, a single allowed crate could bootstrap arbitrary further discovery — making the effective set hard to reason about. By limiting policy influence to `used`-provenance plugins, the trust boundary stays at what the user (or their organization) explicitly opted into.

## Implementation plan

### Step 1: Policy data structures

Define `DiscoveryPolicy` with allow/deny rule sets. Parse from both config and plugin manifests.

### Step 2: Policy accumulation during graph expansion

As plugins are loaded, collect their `[discovery.allow]`/`[discovery.deny]` declarations into a combined policy.

### Step 3: Candidate evaluation

After explicit sources are resolved, enumerate workspace dependencies as candidates. Evaluate each against the accumulated policy. Approved candidates enter the plugin worklist with `dependency` provenance.

## Tests

### Parsing

- `discovery_allow_wildcard_parses` — `[discovery] allow = "*"` parses as allow-all.
- `discovery_allow_specific_cargo` — `[discovery.allow] cargo = { serde = "*" }` parses.
- `discovery_deny_specific_cargo` — `[discovery.deny] cargo = { sketchy = "*" }` parses.
- `discovery_allow_and_deny_coexist` — both in same config file.

### Precedence

- `specific_deny_beats_wildcard_allow` — allow `*` + deny `sketchy` = sketchy blocked.
- `specific_allow_without_wildcard` — only `serde` allowed; `tokio` not discovered.
- `deny_wins_at_same_specificity` — both allow and deny name the same crate; deny wins.

### Policy accumulation

- `policies_from_multiple_plugins_merge` — plugin A allows `serde`, plugin B allows `tokio`; combined allows both.
- `user_deny_overrides_plugin_allow` — user config denies `serde`; plugin allows it; result: denied.
- `discovered_plugin_allow_list_ignored` — a plugin loaded via discovery (`dependency` provenance) has `[discovery.allow]`; its rules are NOT applied.
- `used_plugin_allow_list_applied` — same plugin loaded via config (`used` provenance); its rules ARE applied.
- `transitive_from_used_still_applies` — a plugin transitively loaded from a `used` plugin (inherits `used` provenance) has its allow list applied.

### Integration

- `discovery_default_deny_all` — workspace with `serde` dep, no policy; serde skills NOT installed.
- `discovery_allow_enables_dep_plugin` — config allows serde; serde crate has skills; skills installed with `dependency` provenance.
- `plugin_provided_allow_list` — `symposium-recommendations` allows serde; workspace depends on serde; serde skills installed.
- `deny_blocks_even_with_plugin_allow` — plugin allows serde, user denies it; skills NOT installed.
- `discovery_only_after_explicit_sources` — explicit plugins have `used` provenance; discovered ones have `dependency`.
