# Agent interaction tests

Agent interaction tests exercise Symposium with scripted users, real processes, and selected real coding agents. They complement ordinary tests: use deterministic tests for exhaustive Symposium logic and these journeys when the process, terminal, user, or agent boundary is itself under test.

The feature is experimental. Real-agent runs consume provider capacity and are opt-in.

## Discover scenarios

```console
cargo xtask agent-test --list
```

The scenario list reports required agent, environment, operating-system, and witness capabilities. The contract table maps the tracer's Symposium promises to executable scenarios and linked product gaps.

Running `cargo xtask agent-test` without a scenario prints an execution plan and does not start an agent.

Repeat `--scenario` to select more than one journey:

```console
cargo xtask agent-test --agent claude --environment container --auth api-key --confirm-paid-run --scenario dependency-consent-accept --scenario dependency-consent-decline
```

## Run on the host

```console
cargo xtask agent-test --agent claude --environment host --auth local --confirm-paid-run --scenario dependency-consent-accept
```

The host runner creates fresh project, Symposium, agent, cache, and temporary directories. Local authentication is used only when explicitly requested. If the adapter cannot separate credentials from normal agent configuration, the result is marked non-authoritative. Local authentication also cannot claim the tracer key's $5 provider cap; the execution plan reports that limitation while retaining the scenario's hard token and operation limits.

Host runs are useful for debugging but may still be affected by installed tools and the operating system.

## Run Linux conformance

```console
$env:ANTHROPIC_API_KEY = "..."
cargo xtask agent-test --agent claude --environment container --auth api-key --confirm-paid-run --scenario dependency-consent-accept
```

The runner prepares a pinned base image and one content-addressed Linux `cargo-agents` build from the checkout. Each scenario receives a fresh restricted container. Scenario runtime is hermetic except for the selected agent provider.

To test an existing compatible Linux artifact:

```console
cargo xtask agent-test --agent claude --environment container --auth api-key --confirm-paid-run --symposium-bin ./artifacts/cargo-agents-linux-x86_64 --scenario dependency-consent-accept
```

The override is checked for operating system, architecture, executable format, and available version metadata. The runner never substitutes a released package, PATH binary, host environment, or different execution backend silently.

## Read the execution plan

Before a paid run, the plan reports information such as:

```text
Selected scenarios:       2
CLI-only scenarios:       1
Real-agent scenarios:     1
Maximum agent turns:      1
Maximum provider requests: 4
Maximum tool calls:       3
Input-side token guard:   25,000
Output-token guard:       1,000
Per-run cost allowance:   $0.20
Monthly provider cap:     $5.00
Environment:              Linux container
Agent/runtime:            Claude, pinned
```

The [cost and runtime controls](../coverage-and-ci/README.md#cost-and-runtime-controls) are authoritative. The values above are the initial experimental tracer defaults.

Real-agent scenarios enforce cumulative input, cache-read, cache-write, and output tokens as well as provider-request, turn, tool-call, deadline, and run-wide limits. Cached tokens still count even when they cost less. A paid run requires explicit selection, an agent name, and `--confirm-paid-run`.

The initial tracer permits one user turn, at most four provider requests, three tool calls, 25,000 total input-side tokens, and 1,000 output tokens. At the standard post-introductory Sonnet 5 price, its base-token ceiling is approximately $0.09 per run. Its conservative allowance including cache-price differences is $0.20, and its dedicated provider key has a $5 monthly cap. Initial manual runs record usage so the limits can be reduced. The prompt and fixture are reduced before a limit is raised.

## Read a result

Each run ends as:

- `Passed`: the journey and assertions succeeded.
- `Failed`: the environment worked, but the behavior violated the contract.
- `InfrastructureError`: setup, authentication, provider, runtime, harness, or an operator-imposed budget stopped the run.
- `Unavailable`: the selected combination lacks a required capability.

Results may carry modifiers. `non-authoritative(contaminated-auth-context)` means local authentication could not be isolated from agent configuration. `stability-warning(recovered-infrastructure-error)` means a complete fresh-state retry recovered from a recognized infrastructure failure. A modifier cannot turn a product failure into a pass or satisfy a conformance requirement with non-authoritative evidence.

A scenario that cannot produce its witness within its own token budget is `Failed`. Oversized harness context or a lower operator limit is `InfrastructureError` owned by `runner.budget`. Paid execution is `Unavailable` when the adapter cannot report trustworthy usage.

The summary also names the owning phase. An agent-free scenario may retry once from fresh state after a recognized transient infrastructure error. A scenario that contacted a paid provider is never retried automatically. Product failures and individual steps are never retried. A known-gap reproducer still returns `Failed` when run directly.

Artifacts are under `target/agent-tests/<run-id>/`. Failure artifacts contain only allowlisted, sanitized evidence and a redaction report. Complete homes, authentication directories, and process environments are never archived. Use `--keep-artifacts` to retain rich evidence for a passing run.

## Write a scenario

Each scenario registers declarative metadata and an asynchronous Rust body returning `Result`. The metadata names fixtures, requirements, contract IDs, permissions, budgets, and external endpoints so the runner can preflight without executing the body. The body uses a constrained `ScenarioContext`; it cannot reach undeclared host state, credentials, or agent-specific APIs. Every behavioral branch is a separate fresh-state scenario.

A typical consent journey:

1. Compose a Rust fixture whose dependency embeds a plugin awaiting consent.
2. Start with empty Symposium and agent configuration.
3. Run real `cargo agents init --add-agent <agent>` and assert setup.
4. Run real `cargo agents sync` under a parsed PTY.
5. Select the intended prompt option with explicit keys.
6. Assert terminal anchors, structured events, exit status, and final state.
7. Run one bounded agent query when delivery is under test.
8. Assert a narrow capability witness such as a fixture nonce, hook trace, or MCP server log.

Scenarios declare contract IDs, required capabilities, permissions, scenario-owned token and operation budgets, and external endpoints. They do not contain Claude-specific paths or judge general response quality. An operator-supplied lower budget is shown separately and cannot manufacture a Symposium failure.

Time-dependent scenarios mutate controlled persisted inputs instead of sleeping or changing the production clock. They may set a cache expiry into the past, write a fixture `state.toml`, set a file mtime, or disable the sync debounce. Process and agent deadlines always use real monotonic time.

## CI operation

Fast ordinary tests block pull requests. Stable agent-free process, PTY, and small Linux-container scenarios may graduate after meeting runtime and reliability criteria. Real-agent tracer journeys are manually selected and non-gating while the command is experimental.

Scheduled execution, triage ownership, quarantine, pass-rate targets, and release gating are not part of the experimental command. They require a separate policy informed by measured reliability, runtime, and cost.
