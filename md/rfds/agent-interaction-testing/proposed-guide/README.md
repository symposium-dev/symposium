# Agent interaction tests

Agent interaction tests script user input through real Symposium processes and, when needed, run a real coding-agent binary against a local model fixture. They complement ordinary tests; they do not replace them.

The feature is experimental.

## Discover scenarios

```console
cargo xtask agent-test --list
```

The list reports each scenario's execution profile, supported environments, agent capabilities, available adapters, and contract IDs. Running the command without `--scenario` prints an execution plan and exits without starting a process.

`--scenario` is repeatable:

```console
cargo xtask agent-test \
  --agent claude \
  --scenario dependency-consent-accept \
  --scenario dependency-consent-decline
```

## Run the host consent journeys

```console
cargo xtask agent-test \
  --environment host \
  --agent claude \
  --scenario dependency-consent-accept
```

The runner creates fresh project, Symposium, agent, cache, event, and artifact directories. Before the first command it writes only harness policy, including disabled auto-update, telemetry, and public built-in registries.

Initialization runs over pipes. Consent sync runs under a native PTY. The runner verifies that the PTY was actually created, waits for the rendered prompt, sends explicit keys, waits for exit, and compares the terminal, events, configuration, and files.

The runner waits for the rendered consent prompt and selects the scenario's declared answer. It does not send input before the prompt is observable.

## Run with a real agent and fixture model

```console
cargo xtask agent-test \
  --environment host \
  --agent claude \
  --scenario dependency-consent-accept-agent-fixture
```

This selects the first fixture adapter, which starts the pinned real Claude CLI but sends its model requests to a local scripted endpoint. It uses no provider credentials and incurs no model charge.

The runner checks the Claude version before starting. The selected adapter installs a control skill beside Symposium's skill in the same project scope and, because Symposium supports Claude hooks, installs an independent control hook in the isolated Claude profile. Both controls must work before the Symposium result is interpreted.

Scenario names are agent-neutral. `--agent claude` chooses the adapter that translates shared evidence such as advertised, loaded, and hook completed into Claude-specific request and lifecycle observations. A later adapter can run the same scenario without changing its behavior or result meanings.

The host fixture-agent path is supported on the development platforms established by the adapter spike. Windows fixture-agent execution is best effort during the tracer; Windows agent-free PTY execution is required.

## Run Linux conformance

```console
cargo xtask agent-test \
  --environment container \
  --agent claude \
  --scenario dependency-consent-accept-agent-fixture
```

The runner prepares pinned Symposium and Claude artifacts and starts a fresh restricted Linux container. The scenario has no public egress and no provider credentials. Its only network connection is to the local model fixture and any scenario-declared local service.

To test an existing compatible Symposium artifact:

```console
cargo xtask agent-test \
  --environment container \
  --agent claude \
  --symposium-bin ./artifacts/cargo-agents-linux-x86_64 \
  --scenario dependency-consent-accept-agent-fixture
```

The runner validates the override's platform, version metadata, and digest, stages it first on the controlled `PATH`, and proves which binary Cargo and agent-spawned hooks selected. It never silently uses an ambient released binary.

## Read the execution plan

A fixture-agent plan includes information such as:

```text
Scenarios:                 dependency-consent-accept-agent-fixture
Profile:                   agent fixture
Environment:               Linux container
Symposium artifact:        checkout build, sha256:...
Agent:                     Claude 2.1.283, pinned
Provider credentials:      none
Public network:            disabled
Maximum captured bytes:    2 MiB
Scenario deadline:         90 seconds
```

The exact limits are scenario metadata derived from the pinned-adapter spike. The plan reports main-loop, recognized auxiliary, and total request ceilings before starting the container or agent.

## Read a result

Each run ends as:

- `Passed`: the journey and required controls succeeded.
- `Failed`: the environment and controls worked, but Symposium violated its contract.
- `InfrastructureError`: the runner, environment, fixture, or matching pinned adapter failed.
- `Unavailable`: preflight found that a required executable, version, or capability was absent.

Examples:

```text
Unavailable
  claude version 2.1.284 does not match pinned 2.1.283

InfrastructureError(adapter.claude.fixture)
  project-scoped control skill was not advertised

InfrastructureError(adapter.claude.hooks)
  independent SessionStart control hook did not complete

Failed(delivery.skill.accept)
  control skill loaded, but Symposium skill was not advertised

Failed(delivery.hook.session-start)
  control hook succeeded, but Symposium hook failed

InfrastructureError(environment.provenance)
  cargo agents did not execute the planned checkout artifact

Failed(settings.preserve-foreign)
  Symposium removed an agent setting it did not own
```

The summary records process identity, agent version, phase timing, PTY backend, hook evidence, skill witnesses, and artifact location.

## How consent is scripted

The scenario waits for the actual rendered prompt and then sends the input associated with its declared choice. It never assumes the default means Enable, and it never synchronizes with fixed sleeps.

To prove a declined prompt does not return, the runner waits for the second sync process to exit and searches its completed rendered screen. A period of silence is not evidence.

## How skill delivery is proved

The fixture creates two skills:

```text
control skill:    installed directly by the harness
Symposium skill:  installed or withheld by Symposium
```

Both use the same project skill scope. The control lacks Symposium's ownership marker, so sync must preserve it. Each skill has a unique frontmatter advertisement token and a unique body token. Tokens occur only inside `SKILL.md`.

For accept with the first Claude adapter, the runner proves:

1. explicit sync installed the Symposium skill before Claude started;
2. sync preserved the independent control hook;
3. Claude successfully invoked both the control and Symposium SessionStart hooks;
4. Claude advertised and loaded the control skill;
5. Claude advertised and loaded the Symposium skill; and
6. the exact skill inventory remains correct after Claude exits.

For decline, both controls must still work, while the Symposium tokens must appear in no request and the skill must remain absent after Claude exits.

For a later skills-only adapter, the same scenario retains the inventory, advertisement, loading, and absence proofs. Its hook assertion is reported as `Skipped(not-supported-by-agent)` because Symposium does not register a hook for that integration.

## Existing agent tests

Existing `AgentOnly` tests and the agent half of `Any` continue to run under `cargo test` with their current live-model gate. The `cargo test-agent` alias is a convenience for that targeted invocation, not a second frontend. These tests are the only live-provider path in this design. They are not fixture-model scenarios because their natural-language prompts rely on model judgment.

Ordinary simulation and pipes-only black-box tests continue to run under `cargo test`. Interactive PTY and agent journeys use `cargo xtask agent-test`, including when a stable journey later runs in CI.

## Artifacts

Results live under `target/agent-tests/<run-id>/`. Fixture runs may retain terminal output, per-process Symposium events, local model requests and responses, fixture logs, and controlled workspace diffs. Complete homes and process environments are never archived.
