# Agent adapters

## Adapter contract

An agent adapter implements the `AgentDriver` boundary. Scenarios request capabilities through `ScenarioContext`; they do not call an adapter or agent SDK directly.

An `AgentDriver`:

- reports its capabilities and supported witness forms;
- prepares only agent-specific runtime and authentication state;
- runs a bounded query and waits for protocol completion;
- reports input, cache-read, cache-write, and output usage for every provider request;
- enforces supported output and operation limits and responds to runner cancellation;
- applies scenario-declared permission policy; and
- returns normalized events plus sanitized raw provider artifacts.

The driver interface is capability-based. A scenario requiring an unsupported capability or witness is `Unavailable` for that adapter rather than weakened to a filesystem-only check. An adapter without trustworthy usage accounting cannot run a paid tracer query. The [result contract](../evidence/README.md#results-and-failure-ownership) owns this classification.

## Adapter scope

| Adapter | Role | Scope |
|---|---|---|
| Claude | First production adapter | One fresh, bounded skill-delivery query |
| Fake | Deterministic contract adapter | Follow-up before stabilizing `AgentDriver` |
| ACP fixture | Second protocol adapter and future session input | Follow-up |

The Claude adapter uses its structured SDK so completion, tool activity, and provider usage are observable. The tracer does not add a separate interactive-entry-point smoke test.

The tracer needs one fresh, bounded Claude query to prove delivery of the fixture capability. The provisional driver therefore does not introduce persistent-conversation machinery. A later scenario that genuinely depends on multiple turns or an agent restart must add that capability deliberately and test it before the driver contract grows to include it.

The tracer implements Claude behind a provisional capability-based interface. It does not claim cross-agent behavioral consistency from one production adapter.

Before the interface is declared stable, a follow-up adds deterministic fake adapters for success, failure, timeout, malformed-event, and missing-capability paths. The existing persistent ACP path can then inform a separately tested session capability and a second adapter. Only the bounded Claude query belongs to the tracer.

## Capability witnesses

Every real-agent journey defines bounded evidence that an installed capability crossed into the running agent:

- A skill witness exposes a scenario nonce through a structured load or tool event, with a narrow exact response as fallback.
- A hook witness is the corresponding hook trace.
- An MCP witness is the fixture server's initialization, tool-list, or explicitly requested tool-call log.
- A subcommand witness is the observed subprocess invocation and exit status.

Capability witnesses do not grade general prose, code quality, or whether Symposium makes an agent write better Rust. Those are effectiveness-evaluation questions outside this RFD.

## Permissions

Every real-agent scenario declares allowed read and write roots, executable commands, MCP tools, and non-provider network access. Unexpected requests are denied and fail the scenario. Adapters normalize permission requests and outcomes into the event journal and may not automatically select an arbitrary approval option.

The runner also verifies that the agent did not write outside allowed roots or leave undeclared processes. A denial scenario names the forbidden operation and expected result explicitly. Witnesses use the least powerful operation available.

## Runtime reproducibility

Pinned conformance runs fix the agent CLI or SDK version, dependency lock, base-image digest, and model identifier where the provider supports one. Results record requested and provider-returned model metadata, agent version, Symposium revision, and scenario version.

A follow-up may add a smaller latest-agent canary for current supported releases. Canary failures indicate upstream compatibility work and do not rewrite pinned conformance results. Advancing a pin is a reviewed compatibility change.

Provider behavior cannot always be frozen completely. Assertions therefore remain limited to stable protocol boundaries and capability witnesses.
