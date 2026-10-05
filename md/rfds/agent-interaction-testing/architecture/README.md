# Harness architecture

## Responsibilities

The harness has five boundaries:

1. scenario registration describes what a journey requires and proves;
2. a Rust scenario body drives the journey;
3. process and agent drivers cross production boundaries;
4. environment backends provide controlled host or container state; and
5. evidence collectors classify the result.

Scenarios depend on capabilities exposed by a constrained context. They do not manipulate agent-specific paths, container APIs, credentials, or process-global environment directly.

## Execution profiles

| Profile | Symposium boundary | Agent binary | Model | Frontend |
|---|---|---:|---|---|
| Simulation | Rust library | no | none | `cargo test` |
| Real process, pipes | compiled CLI | no | none | `cargo test` |
| Real process, PTY | compiled CLI and native terminal | no | none | `cargo xtask agent-test` |
| Agent fixture | compiled CLI and real agent | yes | local scripted model | `cargo xtask agent-test` |
| Existing agent live | current hook harness and real agent | yes | provider | `cargo test` with the existing live-agent gate |

Profile is part of new scenario registration. The existing live-agent row describes the retained harness, not a new runner profile. Environment selection is orthogonal. A requested profile or environment never silently falls back to another.

CI may invoke either frontend. A PTY journey remains an xtask scenario if it later becomes CI-blocking.

## Compatibility with the current suite

The runner reuses `symposium-testlib` fixture composition and assertions. It does not invalidate existing tests.

`TestMode` keeps its current meaning:

- `SimulationOnly` runs the simulated body;
- `AgentOnly` runs once per explicitly configured live agent; and
- `Any` runs simulation plus the configured live agents.

Existing agent-mode tests remain live-only because `prompt_or_hook` sends a natural-language prompt and expects a model to choose actions. Fixture-agent execution is available only to new scenarios that declare scripted model responses. This preserves the simulation-or-live design introduced in [PR #178](https://github.com/symposium-dev/symposium/pull/178); the new runner supplies external orchestration rather than a replacement test mode.

The existing Claude SDK and ACP implementations remain operational until their scenarios have explicit replacements. ACP is not required to support a fixture model in this RFD.

## Scenario registration

Each scenario has declarative metadata and an imperative asynchronous Rust body. The runner can list and preflight metadata without executing the body.

Metadata declares:

- stable name and description;
- execution profile and supported environments;
- fixture layers and local services;
- required agent, PTY, and adapter capabilities;
- contracts proved by the scenario;
- deadlines and resource limits; and
- retained artifacts.

The body receives `ScenarioContext` and returns `Result`. Rust control flow, shared helpers, and `?` keep failures local to the operation that caused them.

```rust,ignore
async fn dependency_consent_accept(cx: &mut ScenarioContext) -> Result<()> {
    cx.run_init_piped(cx.agent().config_name()).await?;

    let mut sync = cx.spawn_sync_pty().await?;
    sync.choose_consent(ConsentDecision::Enable).await?;

    let completed = sync.wait_for_exit().await?;
    completed.assert_success()?;
    cx.events().assert_consent_answered(
        "fixture-plugin",
        ConsentDecision::Enable,
    )?;
    cx.agent().assert_skill_installed("fixture-skill")?;
    Ok(())
}
```

Complete journeys are not loaded from TOML or YAML. Only registration metadata, fixture-model rules, execution plans, and results require serialization.

## Scenario context

`ScenarioContext` exposes operations rather than backend objects. Its initial capabilities include:

- run a noninteractive Symposium command;
- start and drive a Symposium PTY;
- inspect structured events and completed terminal output;
- inspect controlled configuration and files;
- start a declared local fixture service;
- run a bounded agent query; and
- retain declared artifacts.

The context rejects undeclared external endpoints, agent operations, and host paths. It passes explicit environment maps to children and never mutates the runner's process-global environment.

## PTY driver

Interactive scenarios use `portable-pty` and parse output with `vt100`. Matching the rendered screen handles cursor movement, ANSI styling, and Windows ConPTY redraws more reliably than searching raw stdout.

The driver supports explicit keys such as Enter, Escape, arrows, EOF, and interrupt. Every wait has a monotonic deadline and an observable condition. Absence is checked only after a positive terminal condition such as process exit.

The runner performs a startup probe and records the selected native PTY backend. A requested PTY that was not actually created is `InfrastructureError(environment.pty)`, never a passing skip.

Before the general PTY driver is accepted on Windows, a focused ConPTY experiment must prove that the current `dialoguer` selection receives injected arrow and Enter keys and that `vt100` observes the resulting screen. Failure returns the RFD to design rather than silently weakening Windows coverage.

Agent-free pipes and PTY journeys support Windows and Linux in this RFD. Native macOS coverage is follow-up work.

## Agent adapter

An agent adapter prepares isolated agent state, starts the real agent binary, and translates agent-specific observations into shared evidence. It reports capabilities such as:

- fixture-model endpoint override;
- noninteractive execution;
- structured output;
- hook lifecycle reporting;
- custom-skill installation locations;
- version reporting; and
- clean cancellation.

A scenario that requires an unsupported core capability is `Unavailable`. Hook checks are different: skill-delivery scenarios require them only when Symposium registers hooks for the selected agent. For a skills-only integration such as OpenCode or Goose, the runner records the hook assertion as `Skipped(not-supported-by-agent)` and continues with the skill witnesses. It does not mark the whole scenario unavailable or pretend that a hook ran.

Agent neutrality means shared scenario concepts, not a shared wire format. Each adapter owns its executable arguments, configuration locations, model protocol, and request interpretation.

Conceptually, each adapter has two parts. Its descriptor supplies the config name, documented project skill locations, and hook support without requiring an agent executable. Its execution part supplies version checks, fixture-model behavior, and observation parsing. Process-only scenarios use only the descriptor; fixture-agent scenarios use both.

The descriptor is a hand-maintained harness oracle. It must not import paths or capabilities from Symposium's production `Agent` table, because comparing product output with product-owned expectations would let the same wrong path pass both sides of the assertion. Changes to a production integration therefore update the descriptor explicitly and are reviewed against the agent's external layout.

The first implementation must establish responsibilities, not freeze a Rust trait derived from one agent. The adapter owns:

- the config name passed to `init --add-agent`;
- project skill locations, exact inventory, and presence checks;
- whether Symposium supports hooks for that agent and, when it does, the agent's hook configuration and foreign-entry checks;
- fixture-model setup, version checks, execution, and raw observations; and
- normalization of skill and hook observations by subject, distinguishing the harness control from the Symposium capability.

Scenario bodies express fixture intent in agent-neutral terms. The initial illustrative request contains the ordered skills to load, the witnesses forbidden from every request, and the point at which the fixture should finish. The accept journey asks to load `[control, product]`. The decline journey asks to load `[control]` and forbids the product advertisement and body witnesses. The adapter translates that intent into Claude `Skill` calls, a Codex file-read call, or another agent-specific exchange.

```rust,ignore
struct FixtureIntent {
    load_skills: Vec<SkillSubject>,
    forbidden_witnesses: Vec<WitnessToken>,
    finish_after_loads: bool,
}

enum SkillSubject {
    Control,
    Product,
}

struct HookDeliveryEvidence {
    subject: HookSubject,
    started: bool,
    completed: bool,
}

enum HookSubject {
    Control,
    Symposium,
}
```

Shared skill evidence distinguishes `advertised` from `loaded`. Shared hook evidence distinguishes `started` from `completed` and identifies `control` or `symposium` as its subject. Controls use the same evidence vocabulary as Symposium-delivered capabilities, so each adapter proves that its observation path works before the runner assigns a product failure. These shapes remain illustrative until a second adapter tests the boundary.

### Claude adapter

Claude is the first fixture adapter. It runs from Rust in print mode with stream JSON, hook lifecycle events, and no session persistence. The feasibility spike fixes the final command. The adapter must not use `--bare`, because that mode skips hooks and plugin synchronization.

Isolation uses controlled `HOME`, `USERPROFILE`, and `CLAUDE_CONFIG_DIR` paths, explicit settings, and a filtered environment. On Windows, both `USERPROFILE` and `HOME` point at the controlled user root because Claude's Node runtime does not use Symposium's home resolver. The fixture endpoint uses Claude's documented gateway configuration.

For the pinned Claude version, the adapter maps the shared evidence as follows:

- advertised means a unique frontmatter token appeared in a main-loop request that exposes Claude's `Skill` tool;
- loaded means the skill-body token appeared in a later captured request;
- a control hook starts when its independent helper writes a matching nonce-bearing receipt;
- a Symposium hook starts when the Symposium subprocess emits matching `ProcessStarted` evidence; and
- either hook completes when Claude emits a successful matching `hook_response`.

Hook-protocol purity is not shared adapter evidence. Package 1 determines whether Claude exposes a useful parse outcome in `hook_response`. Regardless of that result, `hook.stdout-protocol` is proved by package 2's compiled-process regression; the agent journey does not claim to observe stdout that Claude consumes internally.

Those mappings are version-specific Claude adapter assumptions. Another adapter may use different tools, lifecycle events, or request fields while returning the same shared evidence.

Claude global hook registration and unregistration honor `CLAUDE_CONFIG_DIR`. The product migration also removes Symposium-owned legacy hook entries from the former home-based location, without removing hooks Symposium does not own. This is a Claude integration contract, not a requirement on other adapters.

## Model fixture

The model fixture is an adapter-owned local service, not a fake agent. The real agent sends requests to it and processes its responses normally.

Fixture scripts match requests by content. They can return text, tool calls, errors, or protocol termination. They also retain bounded request and response records for assertions.

Request shapes are version-specific adapter assumptions. The control capability in each agent scenario determines whether those assumptions still hold before product evidence is interpreted. Fixture scripts are adapter-owned translations of scenario intent, not protocol logic embedded in scenario bodies.

## Environment backends

### Host

The host backend creates fresh project, Symposium, agent, cache, service, event, and artifact directories and passes a filtered environment to every child.

`SYMPOSIUM_USER_HOME` is an internal, unstable test seam that redirects every Symposium lookup of the user's profile through one resolver. `SYMPOSIUM_HOME` continues to select Symposium configuration and cache state; the two variables are not aliases. The runner also sets `HOME`, `USERPROFILE`, agent-native relocation variables, and a fresh `CARGO_HOME` to the matching controlled roots. Host runs preserve the effective `RUSTUP_HOME` explicitly as a declared read-only toolchain dependency, because changing `HOME` must not disconnect Cargo from its rustup proxy. Container runs provide their own pinned toolchain.

It plants a harmless decoy capability outside the controlled home and verifies that the decoy does not enter copied state or the custom-skill inventory. Before and after the run it also records a bounded projection of real-profile state that Symposium could write: Symposium-owned `cargo-agents hook` entries extracted from known agent settings files, and skill directories containing a `.symposium` marker. The projection must not change. Unrelated agent state, such as Claude session and usage fields, is ignored; file contents are neither copied nor retained. The harness never modifies the developer's actual configuration.

Host runs expose installed tools and operating-system behavior, so they do not claim container-strength isolation.

### Linux container

The container copies fixtures rather than mounting the repository. It runs as non-root with a read-only root filesystem, dropped capabilities, no Docker socket, explicit writable directories, and CPU, memory, process, and time limits.

Fixture conformance contains pinned Symposium and agent artifacts, no provider credentials, and no public egress. Scenario networking is limited to the private model fixture and declared local services.

A fresh container is created per scenario. Immutable build layers and content-addressed artifacts may be reused. Cleanup is idempotent and verifies that no labelled process, service, or container remains.

Fixture-agent execution on Windows is best effort during this RFD. Native Windows agent conformance would also have to settle MSYS shell and hook-executable portability.

## Provenance

The runner builds or accepts an explicit Symposium artifact and computes its digest. The preparation key includes source or dirty-tree digest, `Cargo.lock`, toolchain, target, profile, features, and container image when applicable.

The artifact directory is first on the controlled `PATH`, and the scenario uses `cargo agents`, so the production external-subcommand dispatch and `agents` argument stripping both execute. The fresh `CARGO_HOME` has no competing `cargo-agents` in its `bin` directory. Cargo remains on `PATH` because Symposium invokes it for workspace metadata.

A process-start event records the version and canonical current executable. The runner checks the executable path and its before-and-after digest against the execution plan. A mismatch is `InfrastructureError(environment.provenance)` and product assertions do not run. Agent-spawned hooks resolve `cargo-agents` independently through `PATH`, so their process-start events receive a separate path and digest assertion.

`--symposium-bin` is an explicit override. The runner verifies its platform, format, version metadata, and digest. It never silently selects an ambient or released binary.

Agent preflight records the executable and version. A required version mismatch is `Unavailable`. Advancing a pin is a reviewed adapter-maintenance change.

## Symposium event channel

`SYMPOSIUM_EVENT_DIR=<run-directory>` enables an additive reporting sink. Each Symposium process creates a distinct file:

```text
<pid>-<process-nonce>.jsonl
```

Per-process files avoid cross-process append assumptions when agent-spawned hooks inherit the directory.

Each line contains a small source envelope and the existing snake-case `ReportEvent` representation:

```json
{
  "pid": 1234,
  "source": "cargo-agents",
  "event": {
    "kind": "consent_answered",
    "plugin": "fixture-plugin",
    "decision": "enable"
  }
}
```

The sink is a third per-layer-filtered tracing layer, following the existing file and report-layer pattern. Its permissive filter captures report-tagged events at every tracing level independently of visible reporting. It does not enable `--json`, quiet the terminal, bypass decisions, or write to hook protocol stdout.

`ConsentRequested` and `ConsentAnswered` are debug-level structured events. The existing post-save `Info` messages keep their current human formatting, so ordinary output does not change.

Only a displayed candidate receives consent events. Escape records `cancelled` for the candidate on screen; candidates never shown have no requested or answered event.

## Result contract

| Result | Meaning |
|---|---|
| `Passed` | The journey and all required controls completed. |
| `Failed` | The environment and controls worked, but Symposium violated the contract. |
| `InfrastructureError` | The runner, environment, fixture service, or matching pinned adapter failed. |
| `Unavailable` | Preflight found that a required executable, version, or capability was absent. |

`Skipped(not-supported-by-agent)` is an assertion outcome within a run, not a fifth run result. A skill-delivery run can still be `Passed` when its hook assertion is skipped because Symposium does not register hooks for that agent.

Failures name an owning phase such as `environment.prepare`, `environment.provenance`, `environment.pty`, `symposium.cli`, `symposium.state`, `adapter.<agent>.fixture`, `adapter.<agent>.hooks`, `agent.start`, `agent.query`, `assertion`, or `cleanup`.

Known product gaps still return `Failed` when their reproducers run directly. The result vocabulary has no expected-failure status.

## Deadlines and cleanup

Every wait has a monotonic deadline. Product and assertion failures are not retried automatically. Agent-free infrastructure setup may retry once from fresh state for a recognized transient error. Fixture failures never fall back to a provider, and provider work is never retried automatically.

On timeout, the runner captures current evidence, attempts graceful termination, kills the process tree after a bounded cleanup period, and verifies that no labelled process or container remains.

## Artifacts

Artifacts live under `target/agent-tests/<run-id>/`. Every summary records scenario, profile, environment, Symposium digest, agent version, PTY backend, phase timing, result, owner, and evidence outcomes.

Fixture conformance handles no real credentials. It may retain terminal output, per-process events, local model requests and responses, fixture logs, and controlled workspace diffs. Complete homes and process environments are never archived. The existing live-agent harness retains ownership of its own provider-facing artifact policy.

## Time-dependent scenarios

The tracer does not add a production clock seam. Later cache and throttle scenarios can mutate controlled persisted inputs before starting the process that observes them. Process, PTY, fixture, and agent deadlines always use real monotonic time.
