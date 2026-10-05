# Agent interaction testing

## Summary

Add an experimental `cargo xtask agent-test` orchestration command alongside the existing bimodal test harness for scripted journeys through real Symposium processes, native terminals, isolated environments, and selected coding-agent binaries.

The first tracer tests dependency consent and skill delivery. A scripted user drives the real Symposium CLI, and a real pinned Claude binary talks to a local scripted model. The provider-free path is deterministic and can become a CI lane after its runtime and reliability are measured. Existing simulation and live-agent tests remain valid.

## Decision sought

This RFD asks for agreement on three decisions:

1. Add an orchestration runner alongside the bimodal harness introduced in [PR #178](https://github.com/symposium-dev/symposium/pull/178) for interactions that require a compiled process, PTY, isolated user profile, container, or fixture-backed real agent binary.
2. Test agent integration deterministically by placing a local fixture model beneath the real agent, rather than faking the agent or requiring a paid provider.
3. Prove the architecture with accept and decline consent journeys before expanding to the rest of the registry contract.

Acceptance does not commit Symposium to release gating, every agent, every registry behavior, or an effectiveness benchmark.

The shared architecture is capability-based. Claude is the first implementation, not the definition of an agent. Adding a later adapter must not require changing existing scenario bodies, the runner's result vocabulary, or the meanings of advertised, loaded, and, when applicable, hook-completed evidence.

## Motivation

The current integration suite has strong fixture and in-process coverage, but some production boundaries remain untested. `symposium-testlib::with_fixture` can simulate hooks or ask a configured live model to trigger them. It does not provide a fresh agent home, a real consent interaction, or container isolation.

The discovery prompt is one concrete blind spot. `Output::is_interactive` requires terminal stdin and stdout, so captured tests cannot choose the options a user sees. The compiled process has another blind spot: report output can contaminate hook stdout before the hook protocol response is written. Calling `execute_hook` in-process cannot observe that binary-level failure.

The missing evidence is not more coverage of internal functions. We need to start from controlled files and configuration, execute the checkout binary, provide user input, start a supported agent binary, and inspect the terminal, events, requests, and final state.

This RFD does not evaluate whether an agent writes better Rust. Effectiveness evaluation needs baselines, repeated samples, and quality judgments. This harness tests whether specified Symposium interactions and delivery boundaries work.

## Guide-level walkthrough

The tracer starts with a Rust project whose dependency offers a fixture skill. The runner creates controlled Symposium, user-profile, Cargo, and agent roots, disables external updates and public registries, and then acts like a user:

```text
run cargo agents init for the selected agent over pipes
run cargo agents sync under a PTY
wait for the consent prompt
select Enable
wait for the process to exit
assert the decision, configuration, and installed files
```

The decline journey selects the persistent decline option. It runs `sync` again, waits for completion, and verifies that the prompt did not return. The [tracer chapter](./tracer/README.md) owns the exact prompt and key sequences.

The fixture-agent variants continue the journey. The first adapter starts the real pinned Claude CLI against a local Anthropic-compatible endpoint. A project-scoped control skill proves that the adapter can still observe skill advertisement and loading. Because Symposium registers hooks for Claude, an independent control hook also proves that Claude loaded and executed the configured `SessionStart` hooks. The accepted Symposium skill must then be advertised and loaded; the declined skill must remain absent before and after Symposium's hook.

No provider key or paid model is involved in this conformance path. Existing live-provider tests remain available through their current explicit gate; the new runner does not add another live path.

## Design principles

### Preserve the existing suite

The new runner reuses `symposium-testlib` fixtures and assertions. It does not replace `TestContext`, `TestMode`, the `cargo test-agent` alias, or existing tests from [PR #178](https://github.com/symposium-dev/symposium/pull/178).

Existing `AgentOnly` tests and the agent half of `Any` remain the sole live-provider path. Their natural-language prompts depend on model judgment and cannot run against a scripted fixture unchanged. New fixture-agent scenarios declare their scripted model responses explicitly.

### Test the production boundary that matters

Library logic stays in library tests. Pipes-only binary regressions stay under `cargo test`. Interactive journeys use a native PTY through `cargo xtask agent-test`. Agent delivery is tested only when the behavior can fail inside the agent.

### Make negative evidence meaningful

The fixture-agent journey always proves that a project-scoped control skill works before interpreting Symposium evidence. When Symposium supports hooks for the selected agent, an independent control hook must work as well; skills-only integrations report the hook assertion as unsupported instead of failing or weakening the skill witness. A decline cannot pass merely because the agent started with skills disabled, used a different configuration scope, or changed its request protocol.

### Separate profiles from environments

Simulation, real-process, and fixture-agent are the profiles relevant to the new runner. Existing live-agent tests retain their current frontend and configuration. Host and Linux container are environments. A scenario declares its profile; selecting another command-line option never changes the meaning of that scenario.

### Keep agent protocols behind adapters

Scenarios request semantic capabilities such as project-skill installation, fixture-model execution, skill advertisement, skill loading, and, where Symposium supports it, hook completion. An adapter owns the config name, inventory paths, environment variables, lifecycle events, and protocol fields that prove those outcomes. Claude's `Skill` tool, `hook_response`, and configuration files are therefore adapter details, not shared scenario concepts.

### Prefer evidence over prose

The runner checks the rendered screen, structured events, process identity, exact files, hook lifecycle, and model requests. Model prose is diagnostic rather than a product assertion.

## Scope

This RFD is complete when:

- hook stdout contains only the selected agent protocol;
- accept and decline run through parsed PTYs on Windows and Linux hosts;
- structured consent evidence does not change normal human output;
- every process proves that it ran the planned checkout binary;
- host execution cannot resolve agent configuration through the developer's real user profile;
- Claude global hooks follow `CLAUDE_CONFIG_DIR`, including cleanup of the legacy location;
- pinned Claude completes against the local fixture without public network access;
- the project-scoped control skill is advertised and loaded;
- the independent control hook survives Symposium configuration writes and completes successfully;
- the accepted Symposium skill is advertised and loaded;
- the declined Symposium skill remains absent before and after `SessionStart`; and
- the fixture-agent journey passes in a fresh Linux container.

This RFD does not commit to every consent branch, exhaustive registry scenarios, persistent conversations, fixture support for ACP or other agents, native Windows agent conformance, native macOS coverage, scheduled live-provider execution, or release gating.

## Reading guide

The RFD is organized in the order a reviewer should read it:

1. **[The first tracer](./tracer/README.md)** defines the exact interactions and evidence this RFD commits to.
2. **[Harness architecture](./architecture/README.md)** generalizes the reusable profiles, scenario model, adapters, environments, and result contract.
3. **[Implementation plan](./implementation/README.md)** divides the work into independently mergeable packages with explicit verification.
4. **[Proposed guide](./proposed-guide/README.md)** shows the developer-facing workflow after implementation.

The root document owns the decision and scope. The tracer owns concrete behavior. The architecture owns reusable interfaces. The implementation chapter owns sequencing and rollout.

## Drawbacks

The runner creates a second test frontend that must remain aligned with ordinary fixtures and assertions.

The fixture-model adapter depends on Claude behavior that is not a stable public request-shape contract. A feasibility spike, version pin, project-scoped control skill, and independent control hook contain that risk, but every pin update still requires adapter maintenance.

Docker, pinned agent artifacts, and native PTYs increase setup and runtime compared with `cargo test`. The Linux fixture-agent lane is therefore only a CI candidate until measured.

The tracer is narrow. Passing it proves consent, `SessionStart`, and skill delivery for the named journey, not broad registry conformance or improved agent output.

## Rationale and alternatives

### Extend only the existing bimodal harness

[PR #178](https://github.com/symposium-dev/symposium/pull/178) established a useful model: the same hook-oriented test can simulate an agent with `execute_hook` or ask a configured live agent to cause the hook. Adding a fixture-agent mode there would improve hook tests, and remains a future option.

It is not sufficient for this tracer. Consent is an interactive process that occurs before the agent starts, and process identity, PTY state, isolated user profiles, and containers are run-wide concerns. Putting those responsibilities inside `with_fixture` would make library-oriented tests own external orchestration. The new frontend therefore complements the bimodal harness instead of replacing it.

### Fake the agent instead of the model

A fake agent can test a driver interface but cannot prove that a supported agent binary discovers Symposium hooks and skills. Running the real agent over a scripted model preserves that boundary while controlling the nondeterministic part.

### Stop at process and PTY tests

This would prove consent and installed files without proving that an agent can advertise or load the installed skill. The fixture-agent profile crosses that final boundary without a provider call.

### Generalize every agent now

Agent CLIs use different configuration and model protocols. Generalizing before one adapter works would design an abstraction without implementation evidence. Claude is the first adapter; a later second adapter will test whether the boundary is portable.

If that second adapter requires changes to existing scenario bodies or result meanings, the adapter boundary was wrong and must be revised. Agent-specific implementation work, such as a new protocol fixture, version pin, configuration resolver, and evidence parser, is expected.

## Prior art

[`cli-testing-library`](https://github.com/crutchcorn/cli-testing-library) supplies the useful vocabulary of waiting for rendered screen state and sending user input. This RFD adopts that interaction model with a native Rust PTY implementation.

[Symposium PR #178](https://github.com/symposium-dev/symposium/pull/178) introduced the existing simulation-or-live-agent harness. This RFD preserves that boundary and adds orchestration only for journeys it cannot express.

[Codex response fixtures](https://github.com/openai/codex/blob/main/codex-rs/core/tests/common/responses.rs) run agent logic against scripted model responses and retain outbound requests for structured assertions. They are a direct precedent for putting a deterministic model fixture beneath real agent behavior.

[Claude plugin evals](https://code.claude.com/docs/en/plugin-evals) use fresh isolated sessions, with and without arms, tool-use graders, fixtures, and bounded runs. Symposium adopts the positive-control and negative-probe ideas while avoiding provider calls in its conformance path.

[SWE-bench](https://github.com/SWE-bench/SWE-bench/blob/main/swebench/harness/constants/__init__.py) distinguishes a failed test from an errored execution. The runner similarly separates Symposium contract failures from infrastructure errors.

[agent-rules issue #19](https://github.com/yasuyuki/agent-rules/issues/19) separates installed files, native advertisement, loaded content, and rollback-time absence, and uses per-run witnesses plus negative controls to prevent vacuous success. The tracer applies the same evidentiary pattern to Symposium delivery.

Rust's experimental [libtest JSON output RFC](https://rust-lang.github.io/rfcs/3558-libtest-json.html) separates structured events from presentation and stages a new harness before stabilization. This RFD similarly keeps the runner experimental and preserves existing test behavior.

Claude documents its supported [LLM gateway](https://code.claude.com/docs/en/llm-gateway), [configuration directory](https://code.claude.com/docs/en/claude-directory), and [CLI](https://code.claude.com/docs/en/cli-reference) surfaces. Exact skill request placement remains a pinned adapter assumption established by the first implementation step.

## Unresolved questions

No policy question blocks implementation. Two implementation checkpoints remain.

### Claude fixture checkpoint

Package 1 must establish:

- the request shapes that advertise and load skills for the pinned Claude version;
- how the local fixture completes every request without public network access;
- whether Claude passes the Symposium event-directory variable to hook subprocesses or requires the controlled settings `env` fallback;
- whether `hook_response` exposes hook-output parsing or only lifecycle success; and
- which agent settings suppress unrelated traffic and persistence.

If the checkpoint cannot satisfy those requirements, fixture-agent implementation stops and the RFD returns to discussion. The runner must not substitute a paid provider or weaken the witness.

### Windows PTY checkpoint

Before package 3 commits to Windows consent journeys, a focused experiment must establish that `portable-pty` can inject the current `dialoguer` arrow and Enter inputs through ConPTY and that `vt100` observes the rendered selection. Failure returns the Windows PTY design to discussion; it does not silently skip the platform.

## Future possibilities

Existing `HookStep` values could later compile into scripted model tool calls, giving selected live-only tests deterministic real-agent coverage.

Further registry, cache, predicate, hook, and MCP journeys can reuse the harness. Additional agents can implement the semantic adapter capabilities after Claude establishes the boundary. A second adapter should be added before the interface is treated as stable. CI and release policy can be designed from measured reliability and runtime.

These are directions, not commitments of this RFD.
