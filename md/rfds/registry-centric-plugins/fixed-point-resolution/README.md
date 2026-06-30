# Fixed-point resolution

## TL;DR

Add a convergence loop to the plugin loader that handles ordering dependencies between plugins and the custom predicates they define.

## Motivation

Plugins may define custom predicates that other plugins' `[[plugins]]` entries depend on. This creates ordering dependencies that can't be resolved in a single pass. For example, an enterprise plugin might define `org_approved()` and other plugins gate their `[[plugins]]` entries on it.

## Change in a nutshell

The resolution algorithm extends the graph-based loader:

1. Seed the worklist from user config entries (evaluating builtin predicates immediately, deferring entries with unknown custom predicates).
2. Seed from workspace root/members (unconditional).
3. Process the worklist: resolve sources, scan for plugins, collect custom predicate definitions.
4. After the worklist drains, retry deferred entries against the grown set of known predicates.
5. If progress, re-enter the loop. If stuck (deferred entries reference predicates nobody defines), warn and skip them.

This handles chains (A defines a predicate B needs) and detects cycles (A and B each need the other's predicate).

## Implementation plan

### Step 1: Track custom predicate definitions during expansion

As plugins are loaded, collect the names of custom predicates they define into a growing set.

### Step 2: Defer and retry

When a `[[plugins]]` entry references an unknown custom predicate, defer it instead of skipping. After the worklist drains, retry deferred entries. Warn on stall.

## Tests

### Unit tests

- `known_predicates_resolve_immediately` — builtin predicates don't defer.
- `unknown_predicate_defers` — entry referencing undefined `org_approved()` is deferred, not skipped.
- `deferred_resolves_after_defining_plugin_loads` — plugin A defines `org_approved()`; deferred entry resolves on retry.
- `chain_resolves` — A defines X, B uses X and defines Y, C uses Y; all resolve across multiple iterations.
- `self_referential_cycle_warns` — entry gated on a predicate defined inside itself; stalls and warns.
- `mutual_cycle_warns` — A needs pred from B, B needs pred from A; both deferred; warn emitted.
- `max_iterations_bounded` — pathological case terminates (no infinite loop).

### Integration tests

- `fixed_point_cross_plugin_predicate` — plugin A defines `[[custom_predicates]] name = "team_internal"`. Plugin B's `[[plugins]]` uses `where.predicates = ["team_internal()"]`. A loads first; B resolves on retry.
- `fixed_point_transitive_chain` — predicate definition is two hops deep (A loads B loads C which defines the pred); entry gated on that pred resolves after full expansion.
- `fixed_point_stall_warns_and_continues` — plugin X gated on `mystery()` (nobody defines it); sync completes, warning emitted, X's sub-plugin not loaded.
- `fixed_point_order_independent` — entries declared in various orders in config all produce the same result.
