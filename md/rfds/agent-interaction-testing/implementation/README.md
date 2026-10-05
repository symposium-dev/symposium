# Implementation plan

## Delivery strategy

The tracer is delivered through five independently mergeable work packages. Each leaves the repository in a passing state and produces evidence needed by the next package.

The plan has two stop-checkpoints. Package 1 must prove that the pinned Claude CLI can complete offline against the fixture and expose stable control witnesses. Package 3 must prove that ConPTY can drive the current `dialoguer` prompt before Windows PTY coverage is committed. Failure at either checkpoint returns the affected design to discussion.

## Command interface

The orchestration entry point is:

```console
cargo xtask agent-test [OPTIONS]
```

The initial interface includes:

```text
--list
--scenario <name> ...
--environment <host|container>
--agent <agent>
--symposium-bin <path>
--keep-artifacts
--max-agent-turns <count>
--max-model-requests <count>
```

`--scenario` is repeatable. No scenario means print the execution plan and exit. Scenario metadata declares the execution profile. Incompatible selections fail preflight rather than changing the scenario.

Every new scenario requires `--agent` to select the Symposium integration descriptor that owns its config name and installation paths. Process-only scenarios do not require that agent's executable. Fixture-agent scenarios additionally require the explicitly pinned executable, but no credentials or paid-run confirmation. The existing bimodal harness remains the only live-provider frontend in this RFD.

The plan reports selected scenarios, profiles, environment, binary provenance, agent pin, network requirements, deadlines, resource limits, and whether credentials can be used before starting any process.

## Work package 1: Prove the Claude fixture seam

### Purpose

Establish that one pinned Claude CLI can run against a logging local model fixture and expose the signals required by the tracer.

### Dependencies

None. This package does not depend on the new scenario runner or product event changes.

### Work

- Pin the Claude CLI version used by the experiment.
- Start a minimal local Anthropic-compatible endpoint that records every request.
- Give Claude a controlled user profile and a project containing one directly installed control skill without a `.symposium` marker.
- Install an independent `SessionStart` control hook that invokes a cross-platform helper by absolute path.
- Run Claude in print mode with stream JSON, hook lifecycle output, and no session persistence.
- Determine how to identify the main request by content.
- Script a Skill tool call and locate the loaded body in a later request.
- Determine which settings suppress unrelated traffic and persistence.
- Verify whether hook subprocesses inherit `SYMPOSIUM_EVENT_DIR`.
- If they do not, prove that the controlled Claude settings `env` block passes it to the Symposium hook.
- Determine how a successful and unsuccessful hook appears in Claude's stream, whether `hook_response` exposes protocol output or its parse outcome, and correlate it with the independent control receipt or Symposium process evidence as appropriate.
- Measure main-loop and auxiliary model requests separately, identify every accepted auxiliary shape, and derive a bounded total ceiling.
- Verify clean completion with public network access disabled.
- Record the version-specific request and lifecycle assumptions as adapter tests or fixtures.

The experiment must not assume that the first request is the main request or that the loaded body appears in one predetermined JSON field.

### Verification

- `claude --version` matches the pin.
- The control skill is advertised and loaded.
- The control hook produces both a successful lifecycle response and a matching independent nonce-bearing helper receipt.
- The complete exchange succeeds without provider credentials or public egress.
- An intentional request-shape mismatch is reported as an adapter failure.
- Hook environment behavior is recorded rather than assumed.

### State after merge

The feasibility evidence and minimal fixture exist. The general runner, PTY journeys, Symposium product events, and container remain absent.

## Work package 2: Establish safe process evidence

### Purpose

Make compiled Symposium processes observable and redirect every user-profile lookup without altering user-facing or hook-protocol output.

### Dependencies

None on the fixture spike for the hook stdout regression and Symposium event work. The work can proceed in parallel, but package 4 depends on both packages 1 and 2.

### Work

This package lands as several reviewable PRs rather than one harness-sized change. The stdout regression, user-home resolver, Claude configuration migration, and structured event sink each remain independently testable and keep the repository passing.

- Fix hook execution so stdout contains only the selected agent protocol.
- Add a pipes-only black-box regression under `cargo test`.
- Add one user-profile resolver that honors internal, unstable `SYMPOSIUM_USER_HOME`, and route all current `dirs::home_dir()` call sites through it.
- Test the resolver on Windows and Unix, including empty and relative override rejection.
- Track the Claude `CLAUDE_CONFIG_DIR` behavior as its own issue and implementation PR within this package.
- Make Claude global hook registration and unregistration use `<CLAUDE_CONFIG_DIR>/settings.json` when relocated.
- During that migration, remove only Symposium-owned hook entries from the legacy home-based settings file and preserve unrelated hooks in both locations.
- Add `ProcessStarted`, `ConsentRequested`, and `ConsentAnswered` report events.
- Emit consent events at debug level and preserve the existing human `Info` messages.
- Add the per-process JSONL sink enabled by `SYMPOSIUM_EVENT_DIR` as a third per-layer-filtered tracing layer following the existing pattern.
- Capture report-tagged events at every level independently of visible output mode.
- Record process ID, source, version, and canonical executable identity.
- Add merge helpers for the runner without introducing a larger canonical journal schema.

### Verification

- Hook stdout parses as the expected protocol and contains no report text.
- Every production user-profile lookup honors `SYMPOSIUM_USER_HOME`.
- Claude hook registration follows `CLAUDE_CONFIG_DIR`, legacy Symposium entries are removed, and unrelated entries survive.
- Normal consent output is byte-for-byte unchanged for existing cases.
- The side channel contains snake-case structured events in separate process files.
- A hook subprocess can write evidence without contaminating stdout.
- Existing hook and integration tests remain green.

### State after merge

The compiled process has stable evidence, safe user-root isolation, and provenance primitives. The xtask runner, PTY driver, real agent, and container remain absent.

## Work package 3: Run host consent journeys

### Purpose

Drive the accept and decline interactions as a user would through the compiled checkout binary.

### Dependencies

Package 2.

### Work

- Extract reusable fixture preparation from `symposium-testlib` without changing existing `TestMode` behavior.
- Add scenario registration metadata and imperative Rust bodies.
- Add the `cargo xtask agent-test` frontend and execution-plan preflight.
- Seed controlled harness policy before the first command.
- Select an agent integration descriptor for every scenario. It supplies the config name, project skill location, and hook support without requiring the external agent executable for process-only scenarios.
- Maintain descriptor expectations independently in the harness; do not import paths or capabilities from Symposium's production agent table.
- Set controlled `SYMPOSIUM_HOME`, `SYMPOSIUM_USER_HOME`, `HOME`, `USERPROFILE`, agent-native configuration, and `CARGO_HOME` roots before starting a child.
- Preserve the effective host `RUSTUP_HOME` explicitly as a declared read-only toolchain dependency.
- Put the planned checkout artifact first on `PATH`, keep Cargo available, and invoke `cargo agents` so production subcommand dispatch executes.
- Require fixture Cargo dependencies to be local path dependencies.
- Run `init --add-agent` over pipes.
- First prove in a focused Windows experiment that ConPTY-injected selection keys reach the current `dialoguer` prompt and that `vt100` observes the result.
- If that experiment succeeds, implement the general `portable-pty` plus `vt100` driver and Linux PTY probe. If it fails, return the Windows commitment to design review.
- Implement the exact accept and decline key sequences.
- Assert structured events, exit status, configuration, files, and prompt suppression.
- Ask the selected integration descriptor to inspect the project skill inventory; scenario bodies contain no agent-specific path.
- Add a focused process assertion that init and sync preserve seeded foreign skill and hook entries not owned by Symposium.
- Before and after the host journey, compare only the real-profile state Symposium could write: extracted `cargo-agents hook` entries and `.symposium`-marked skill directories. Ignore unrelated mutable agent state.
- Produce a compact result summary and bounded failure artifacts.

### Verification

- Accept and decline send the tracer-defined inputs only after the exact prompt is visible.
- A completed second sync contains no prompt anchor.
- The PTY probe cannot silently pass when no terminal was created.
- Windows and Linux host runs pass.
- The actual executable path and digest prove that Cargo selected the checkout artifact despite ambient installations.

### State after merge

The project has useful agent-free integration journeys. Claude, the fixture model, and Docker are not required to run them.

## Work package 4: Add fixture-agent scenarios

### Purpose

Extend the consent journeys across a real agent boundary without contacting a provider.

### Dependencies

Packages 1, 2, and 3.

### Work

- Implement the provisional capability-based Claude adapter using the proven spike assumptions. The concrete Rust interface remains private and revisable until a second adapter tests it.
- Keep Claude paths, environment variables, model request parsing, `Skill` tool matching, and `hook_response` parsing inside that adapter.
- Accept agent-neutral fixture intent: ordered control or product skills to load, witnesses forbidden from every request, and completion after the requested loads.
- Return shared skill evidence (`advertised`, `loaded`) and subject-labelled hook evidence (`started`, `completed`) to scenario bodies.
- Add separate fixture-agent scenario registrations with scripted responses.
- Isolate Claude through controlled `HOME`, `USERPROFILE`, `CLAUDE_CONFIG_DIR`, settings, and environment.
- Forbid `--bare`.
- Install the control skill in the project scope used by Symposium, without a `.symposium` marker, and generate distinct control and Symposium tokens.
- Install the independent control hook before Symposium writes settings and assert that init and sync preserve it.
- Require the control advertisement and body before product assertions.
- When Symposium supports hooks for the selected agent, require a successful lifecycle response plus an independent receipt for the control hook and Symposium process evidence for the product hook.
- When Symposium supports skills but not hooks for the selected agent, record the hook assertion as `Skipped(not-supported-by-agent)` and continue with skill evidence.
- Assert hook binary provenance separately from direct CLI provenance.
- Assert the accepted skill before Claude starts, in model requests, and after Claude exits.
- Assert the declined skill is absent before Claude, from every request, and after Claude exits.
- Bound fixture requests, responses, captured bytes, tools, processes, and wall-clock time.

### Verification

- A Claude version mismatch is `Unavailable` during preflight.
- Broken control advertisement and loading are adapter infrastructure failures.
- A control skill or hook entry removed by Symposium is `Failed(settings.preserve-foreign)` before adapter health is classified.
- A missing or failing required control hook is an adapter infrastructure failure.
- Once the required control hook works, a missing or failing Symposium hook is `Failed(delivery.hook.session-start)`.
- Once the control works, missing or unexpected Symposium evidence is `Failed`.
- For the initial Claude adapter, `SessionStart` is positively observed in both branches.
- Fixture failure never falls back to a provider.
- Existing `AgentOnly`, `Any`, Claude SDK, and ACP behavior is unchanged.
- Scenario bodies and result classification contain no Claude config name, paths, request fields, tool names, or lifecycle event names.

### State after merge

The host can run deterministic real-agent scenarios on platforms established by the spike. Windows fixture-agent execution remains best effort. Container conformance and CI graduation remain absent.

## Work package 5: Add Linux-container conformance

### Purpose

Run the fixture-agent journey in a fresh reproducible Linux environment and measure whether it is suitable for CI.

### Dependencies

Package 4.

### Work

- Add a container backend behind the existing environment interface.
- Build or accept explicit content-addressed Symposium and Claude artifacts.
- Provide Cargo on the controlled `PATH` for external-subcommand dispatch and Symposium workspace metadata.
- Provide a pinned container toolchain and require fixture manifests to resolve through path dependencies only.
- Copy fixtures instead of mounting the repository.
- Run as non-root with a read-only root filesystem, dropped capabilities, explicit writable paths, and resource limits.
- Disable public egress and expose only the local model fixture and declared local services.
- Create a fresh container for every scenario.
- Implement idempotent cleanup and leak checks.
- Report checkout preparation, image preparation, warm startup, agent execution, and evidence processing separately.

### Verification

- The container runs the planned Symposium and Claude artifacts.
- No provider credential is mounted or accepted.
- Only fixture-local network traffic occurs.
- The same scenario bodies and evidence assertions used on the host pass in Linux.
- Cleanup leaves no labelled process, service, or container.
- Cold and warm timings are recorded.

### State after merge

All tracer completion criteria are implemented. The Linux fixture-agent lane is still a CI candidate until its observation period establishes acceptable runtime and reliability.

## Resource limits

Fixture-model scenarios bound main-loop requests, recognized auxiliary requests, total requests, response count, captured bytes, tool calls, child processes, and wall-clock time. Package 1 establishes the request shapes and ceilings for the pinned adapter. Unknown request shapes fail immediately; limits prevent protocol loops and unbounded artifacts.

Scenario limits define the intended conformance run. A lower operator ceiling that interrupts an otherwise valid run yields `InfrastructureError`, not a Symposium product failure.

The tracer does not implement monthly token ledgers or release accounting. The deterministic conformance path has no provider charge.

## CI graduation

The initial policy is:

- ordinary deterministic tests block every pull request;
- stable pipes-only regressions run with ordinary tests;
- agent-free PTY scenarios may graduate after measured Windows and Linux reliability;
- the Linux fixture-agent lane is a CI candidate after measured runtime, cleanup, pinned-agent installation, and adapter stability; and
- existing live-provider tests remain under their current explicit gate and are not duplicated by the new runner.

This RFD does not define scheduled ownership, quarantine, pass-rate, or release-gate policy. Those decisions require operational evidence from the tracer.

## Existing harness migration

Before closing the RFD, correct `md/design/running-tests.md` so it documents the current `SYMPOSIUM_ENABLE_AGENT_TESTING` gate and `cargo test-agent` alias accurately.

Keep the bimodal harness introduced in [PR #178](https://github.com/symposium-dev/symposium/pull/178), including `TestMode::AgentOnly`, `test-agents.toml`, the Claude SDK path, and ACP support, operational until their scenarios have explicit replacements. Do not label them fixture-compatible.

A follow-up may compile `HookStep` values into scripted model tool calls. That could migrate selected existing tests without changing their scenario intent.

## Follow-up work

Tracked follow-ups can add:

- ask-later and Escape consent branches;
- enablement precedence, cache, predicate, and registry failure scenarios;
- non-workspace package-search fixtures;
- additional hook events and MCP witnesses;
- a second fixture-model adapter;
- native Windows agent and native macOS coverage;
- CI graduation and release policy; and
- catalog automation after the scenario set is large enough to justify it.

These are not completion criteria for this RFD.

The second adapter is the portability test for the interface. It may add its own executable pin, environment resolver, fixture protocol, configuration paths, and evidence parser. It should not require edits to the existing consent scenario bodies or runner result contract; if it does, the shared boundary must be revised before the interface is considered stable.
