# Agent interaction testing

## TL;DR

- Add an experimental `cargo xtask agent-test` command for scripted journeys through real Symposium processes, terminals, isolated environments, and selected coding agents.
- Reuse the existing fixture infrastructure. Ordinary `cargo test` remains the fast, exhaustive layer for Symposium logic.
- Prove one tracer: accept or decline a dependency suggestion, repeat it in a fresh Linux container, and use one bounded Claude query to prove skill delivery.
- Keep scenarios agent-neutral, treat paid runs as explicit and non-gating, and defer broader registry coverage and release policy until the tracer provides implementation evidence.

## Motivation

The current integration suite has strong fixture and in-process coverage, but it cannot exercise every production boundary. `symposium-testlib::with_fixture` runs agent tests only when `SYMPOSIUM_ENABLE_AGENT_TESTING` is set, and the current agent path does not provide a fresh agent home, a real terminal conversation, or container isolation.

The discovery prompt is a concrete unreachable branch. `Output::is_interactive` requires terminal stdin and stdout, and `discovery::prompt_for_consent` returns without asking when that condition is false. Existing tests can verify discovery and noninteractive behavior, but they cannot select Enable or No through the interface a user sees.

The compiled process also has behavior that an in-process assertion cannot observe. `src/bin/cargo-agents.rs` installs the normal report layer before identifying a hook command, so report output can precede the hook protocol payload on stdout even though the hook later uses `Output::quiet()`. A black-box process test is required to expose that failure.

The missing evidence is therefore not more unit coverage. We need to start from controlled directories and configuration, execute the production command, provide user input, and inspect the visible output, structured decisions, and resulting state. A selected real agent is needed only to prove that Symposium-delivered capability crosses into the agent.

This RFD does not evaluate whether an agent writes better Rust. Effectiveness evaluation requires a baseline, repeated samples, and statistical analysis. The tracer proves integration behavior, not causal improvement.

## Change in a nutshell

The first accepted journey starts with an empty Symposium home, an empty agent configuration, and a Rust project whose dependency embeds a plugin awaiting consent:

```text
run cargo agents init --add-agent claude
run cargo agents sync under a PTY
wait for the dependency suggestion
choose Enable
assert the visible prompt, structured events, exit status, config, and files
repeat the journey in a fresh Linux container
run one bounded query through the Claude adapter
assert the scenario nonce from the installed fixture skill
```

A separate decline scenario starts from fresh state, selects "No, don't ask again," restarts the CLI, and proves that the decision persists and the prompt does not return.

The design follows these invariants:

- Authoritative CLI assertions execute the compiled `cargo-agents` binary. Interactive commands use a PTY; hooks use pipes.
- Scenario registration metadata declares fixtures, capabilities, permissions, contracts, and budgets before execution. An asynchronous Rust body drives the journey through a constrained context.
- Host and container backends run the same scenario body. A requested environment never silently falls back to another.
- A real agent is used only when delivery into that agent is the behavior under test. Every paid query has a narrow capability witness and hard resource limits.
- Visible terminal output, structured events, and final state must agree. No one observation channel substitutes for the others.

## Detailed plans

### Behavioral contract

The harness is intended to prove that Symposium:

- discovers extensions relevant to the current project;
- respects trust and explicit user choices;
- installs and removes the expected configuration and files;
- delivers selected skills, hooks, MCP servers, and subcommands across the agent boundary; and
- reports failures without leaking host state or credentials.

Accepted RFDs and reference documentation define the expected behavior when the implementation disagrees. For example, the [accepted discovery contract](../registry-centric-plugins/discovery-sync/README.md#enablement-configuration) gives `disable` precedence over `use` and `auto-enable`. The [coverage table](./coverage-and-ci/README.md#contract-table) records an implementation discrepancy as a gap or follow-up instead of treating current behavior as the contract.

### Reference-level design

The new engine is additive. Existing fixtures, `TestContext`, simulations, and deterministic tests remain. `cargo test` owns fast coverage, including deterministic black-box process regressions. `cargo xtask agent-test` orchestrates explicit environments, credentials, containers, filtering, artifacts, and real-agent execution.

The supporting chapters are the authoritative homes for the detailed contracts:

- [Scenario model](./scenario-model/README.md) defines registration metadata, imperative Rust bodies, the production process boundary, PTY scripting, and controlled time-dependent state.
- [Agent adapters](./agent-adapters/README.md) defines the provisional driver, Claude, later ACP and fake adapters, capability witnesses, permissions, and runtime pinning.
- [Execution environments](./environments/README.md) defines host and container isolation, native operating-system coverage, binary provenance, networking, fixture trust, and authentication.
- [Evidence and results](./evidence/README.md) defines observation channels, the event journal, assertions, result classification, retries, cleanup, and artifact safety.
- [Coverage and CI](./coverage-and-ci/README.md) defines the contract table, coverage layers, tracer obligations, command interface, cost controls, and CI boundaries.
- [Proposed guide](./proposed-guide/README.md) shows the intended developer-facing workflow.

### Scope and compatibility

This RFD is complete when:

- the accept and decline journeys execute as real native processes through a parsed PTY;
- visible output, structured events, exit status, configuration, and filesystem state agree;
- the same journeys pass in a fresh Linux container;
- the accepted branch produces one bounded Claude capability witness containing its scenario nonce; and
- failures produce sanitized, useful artifacts with measured phase timing and provider usage.

The tracer also fixes the known hook stdout contamination and adds its process-level regression. This provides an immediately useful result before the larger harness is complete.

This RFD does not commit to every consent branch, exhaustive registry scenarios, persistent agent conversations, fake or ACP conformance, native Windows and macOS agent runs, hook and MCP delivery witnesses, scheduled real-agent execution, or release gating. Those remain [post-tracer direction](./coverage-and-ci/README.md#post-tracer-direction).

The new command does not replace or reinterpret existing test results. Existing `cargo test` fixtures and assertions remain valid, and deterministic regressions continue to belong there when they can observe the production boundary. The agent-test runner adds a second, explicitly selected frontend for journeys that require a real terminal, controlled home, container, or agent.

### Safety and interpretation boundaries

Scenario fixtures are repository-owned and reviewed. An extension awaiting user consent is not treated as hostile code. The container improves reproducibility and least privilege; it is not a sandbox for testing malicious plugins, hooks, MCP servers, or agents.

The isolation canary proves that a named decoy capability did not enter the controlled agent state. The scenario nonce proves that the selected fixture capability did enter the agent. Neither witness proves that the agent followed general instructions or produced high-quality code.

Claude is the first production adapter because it is available to current developers. The scenario and driver contracts use agent-neutral concepts, but one adapter does not prove behavioral consistency across agents. The driver remains provisional until later fake and ACP implementations test the boundary.

### Drawbacks

The runner creates a second test frontend with its own scenario registration, preflight, artifact, and result code. Even though it reuses fixtures and assertions, maintainers must keep its behavior aligned with `cargo test` and the production CLI.

Docker and provider credentials raise the contribution barrier. Most contributors can run the host, agent-free journeys, but reproducing Linux isolation or the Claude witness requires additional software, credentials, and provider access.

Real-agent execution is nondeterministic, slower, and paid. Narrow witnesses and hard budgets limit those risks; they do not eliminate provider outages, model changes, or occasional inconclusive runs.

The tracer covers only consent and skill delivery. A passing tracer could create false confidence if it is presented as broad registry or agent conformance. Reports must identify the exact contracts and evidence each journey proves.

PTY behavior differs across operating systems, so Linux container success cannot satisfy native Windows or macOS requirements. The tracer proves Linux container behavior and the host platform used during development; native expansion remains separate work.

Container preparation adds runtime beyond the current in-process suite. The runner reports checkout build or image preparation separately from warm startup, agent execution, and evidence processing so the team can decide which agent-free scenarios are suitable for pull-request CI.

### Rationale and alternatives

The selected design keeps exhaustive deterministic coverage in `cargo test` and adds an opt-in orchestrator only for evidence that the existing frontend cannot obtain. This separates inexpensive product logic from process, terminal, isolation, and provider costs.

#### Extend only the existing test harness

One test command and one fixture API would be simpler. The existing harness should continue to gain deterministic process regressions where practical, including the hook stdout test. It cannot make provider credentials, Docker, PTY interaction, and paid execution safe defaults for ordinary `cargo test`, however. Keeping those concerns in an explicit command preserves the current suite's speed and accessibility.

#### Stop at black-box process and PTY tests

This would prove discovery, consent, hook output, and persisted state without provider cost. It would not prove that a capability installed by Symposium is visible inside a supported agent. One bounded nonce query is retained because crossing that final boundary is a central claim of the integration.

#### Describe complete journeys as data

A fully declarative format could be serialized and generated by external tools. It would also require a scenario interpreter and concentrate errors from an entire journey at the interpreter boundary. Declarative registration metadata is retained for preflight and discovery, while an imperative asynchronous Rust body provides normal control flow and local `?` failure sites.

#### Add persistent agent sessions immediately

Persistent sessions will be useful for confirmation and restart journeys that span multiple agent turns. The tracer uses one query, so a session abstraction would be designed without an exercising scenario. The adapter begins with a bounded single-query capability and grows only when a committed journey requires persistence.

#### Make the container conditional on host leakage

The host canary can show whether a named decoy entered the controlled agent home. It cannot control installed tools, networking, system libraries, or operating-system behavior. The Linux container is therefore retained as a reproducibility boundary even when the host canary passes.

#### Do nothing

The current suite would remain unable to select both consent outcomes through the terminal, detect some compiled-process output failures, or prove that an installed capability reaches a real agent. Those are the specific blind spots this RFD exists to close.

### Prior art

[`cli-testing-library`](https://github.com/crutchcorn/cli-testing-library) provides a useful interaction vocabulary based on querying visible screen state and sending user events. This RFD adopts that model for parsed PTY interaction, but not its Node implementation or platform limitations.

[`cli-testing-specialist`](https://github.com/sanae-abe/cli-testing-specialist) demonstrates generated tests for general CLI behavior. Symposium journeys need product-specific fixtures, discovery contracts, persisted consent, hook protocols, MCP evidence, and agent delivery, so generic command validation is not the central abstraction here.

The existing Symposium integration harness supplies the fixture composition and deterministic assertions that the new runner reuses. Its strengths argue for an additive frontend rather than replacement; its inability to provide controlled interactive and agent boundaries identifies where the addition begins.

Rust's experimental [libtest JSON output RFC](https://rust-lang.github.io/rfcs/3558-libtest-json.html) separates structured test events from presentation and validates a new harness interface before stabilization. This RFD follows the same lessons through a structured evidence channel and an experimental runner, without adopting libtest's event protocol.

### Unresolved questions

No design question currently blocks acceptance. The tracer contract, scope, evidence layers, and ownership boundaries are defined.

Implementation must still establish:

- which PTY implementation satisfies the parsed-terminal contract on the initial host platform;
- whether the Claude adapter can report trustworthy usage and expose the controlled custom-skill inventory without agent-specific behavior leaking into scenarios; and
- the measured cold preparation, warm startup, provider usage, and total cost of the container-backed witness.

These measurements may refine internal interfaces and limits. If an implementation result makes a required witness or isolation guarantee infeasible, the contract returns to discussion rather than being weakened silently.

Scheduling, release gating, persistent sessions, additional adapters, and native operating-system coverage are deliberately deferred. They are future design questions, not acceptance blockers for the tracer.

### Future possibilities

Additional registry journeys can register new metadata and bodies against the same `ScenarioContext`. New agents can implement the adapter contract without adding agent-specific paths to scenarios. Persistent conversations can become an adapter capability when the first multi-turn journey supplies a concrete test. CI and release policy can be designed from measured reliability, runtime, and cost instead of estimates.

These extensions build on the process, scenario, environment, adapter, and evidence boundaries established here; none requires replacing the tracer architecture. They are not commitments of this RFD and are not independent reasons to accept it.

### Proposed documentation

The [agent interaction test guide](./proposed-guide/README.md) is written as the developer documentation should read once the experimental command exists. It explains scenario discovery, host and container execution, paid-run confirmation, budgets, results, artifacts, scenario authoring, and the initial CI boundary.

## Frequently asked questions

### What does a passing real-agent journey prove?

It proves the contracts named by that journey and only those contracts. For the tracer, matching terminal, event, exit, and state evidence proves the consent behavior; an exact controlled inventory plus the scenario nonce proves that the fixture skill crossed into the selected agent. It does not prove general response quality or all registry behavior.

### Is this an agent-effectiveness evaluation?

No. Effectiveness evaluation compares outcomes, needs a baseline and repeated samples, and may judge code quality. This harness verifies that specified Symposium interactions and delivery boundaries work. Effectiveness studies may later use the harness as execution infrastructure, but their claims and methodology remain separate.

## Implementation plan and status

Implementation has not begun. Each step leaves the repository with an independently useful, passing result.

### Step 1: Fix hook stdout at the process boundary

Change hook execution so stdout contains only the selected agent's protocol payload. Add a deterministic black-box regression that spawns the compiled hook command with piped stdin, stdout, and stderr.

The new agent-test runner, PTY support, containers, and agent adapters remain absent.

- [ ] Verify that stdout parses as the expected hook protocol and that human report output is absent.
- [ ] Run the existing hook and integration tests.

### Step 2: Run the host consent journeys

Add scenario registration metadata, the constrained asynchronous `ScenarioContext`, the `cargo xtask agent-test` frontend, the structured side channel, and the minimum PTY driver needed for dependency-consent accept and decline. Reuse the current fixture composition and assertion helpers.

Containers and real-agent execution remain absent. Reconcile the contract table with the executable scenario names after the journeys pass; do not add catalog code generation.

- [ ] Verify accept and decline against terminal anchors, structured events, exit status, configuration, filesystem state, the host-state canary, and exact fixture-controlled custom-skill inventory.
- [ ] Verify every `Covered` contract row names an executable scenario and every `Gap(issue)` row names an issue and failing reproducer.

### Step 3: Run the same journeys in Linux

Add Docker execution, a content-addressed Linux Symposium binary, least-privilege container rules, disabled scenario networking, cleanup, and infrastructure diagnostics. Run the Step 2 scenario bodies unchanged.

Provider egress, credentials, and real-agent execution remain absent.

- [ ] Verify cold preparation and warm startup separately.
- [ ] Verify the host-state canary, custom-skill inventory, cleanup, and parity with the remaining host assertions.

### Step 4: Add one real-agent delivery witness

Add the bounded Claude adapter and extend the container-backed accepted branch with one fixture-skill query. Pin the runtime and add only the allowlisted provider egress and restricted API-key handling required by that query.

Persistent conversations, other agents, scheduled execution, and release gating remain absent.

- [ ] Verify the capability nonce, exact pre-query custom-skill inventory, installation and hook-registration evidence, usage limits, redaction, cleanup, and error classification.
- [ ] Record phase timing, provider usage, and conservative cost from an explicitly confirmed manual run without automatic paid retries.

Before closing the RFD, correct `md/design/running-tests.md` so it documents the `SYMPOSIUM_ENABLE_AGENT_TESTING` gate. Keep `TestMode::AgentOnly`, `test-agents.toml`, and `tests/agent_harness/run_scenario.py` temporarily for existing Claude and ACP coverage, but mark that path as superseded and add no new scenarios to it. File its removal with the ACP follow-up after the remaining scenarios migrate.

Closing the RFD also requires follow-up issues or RFDs for catalog automation, fake and ACP conformance, remaining scenario families, native operating-system expansion, and release CI graduation.
