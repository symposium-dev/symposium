# Evidence and results

## Dual observation

Interactive journeys observe two channels from the same process:

- The normal PTY proves that a suggestion or prompt was visible and accepted real user input.
- A harness-controlled JSONL side channel reports stable Symposium decisions and state transitions.

The side channel attaches an additional reporting sink. It must not enable quiet mode, bypass confirmation, change command decisions, or write into a hook's protocol stdout. Events needed by the initial scenarios include discovery, confirmation requested and answered, enablement, installation, and hook dispatch. Later scenarios may add predicate and cache events without changing the product-facing stream.

Terminal assertions use only the stable text necessary to prove that a user could understand and answer the prompt. Colors, wrapping, and complete screens are not ordinary snapshots. Detailed behavioral assertions use structured events and final state.

## Canonical event journal

The runner merges harness events, Symposium side-channel events, agent protocol events, and observed state changes into one coarse journal while retaining sanitized source artifacts.

Each event envelope contains:

- schema version;
- run, scenario, and attempt identifiers;
- source and source-local sequence;
- correlated operation identifier;
- event kind;
- real monotonic offset; and
- normalized payload.

A confirmation event has this shape:

```json
{
  "schema_version": 1,
  "run_id": "run-fixture-01",
  "scenario_id": "dependency-consent-accept",
  "attempt": 1,
  "source": "symposium",
  "source_sequence": 4,
  "operation_id": "sync-01",
  "kind": "confirmation.answered",
  "monotonic_offset_ms": 184,
  "payload": {
    "decision": "enable"
  }
}
```

Provider-operation events record requested limits and reported input, cache-read, cache-write, and output tokens. Aggregate token counts and derived cost are evidence, not estimates substituted for missing accounting.

Sequence is strict within one source. Receipt order is diagnostic and does not imply causal order across processes. Assertions express partial order within a source or correlated operation, such as discovery before confirmation and confirmation before installation. Unrelated sources remain unordered unless explicitly correlated.

Dynamic paths, process IDs, ports, and timestamps are normalized before comparison. Unknown additive event kinds are retained and ignored unless required. Breaking envelope changes increment the schema version. Event payloads use stable identifiers and exclude secrets.

## Assertions

Authoritative assertions prefer:

- exact configuration and allowed filesystem state;
- process exit status and normalized system events;
- protocol completion and tool activity;
- hook stdout containing only the selected agent's protocol output;
- discovery, consent, predicate, and cache decisions; and
- hook, skill, MCP, and subcommand capability witnesses.

Model prose is checked only through a narrow fixture-defined nonce or fact when that is the available witness. Full responses are diagnostic, not gating.

## Results and failure ownership

A run has four base results:

| Result | Meaning |
|---|---|
| `Passed` | The requested journey and assertions completed. |
| `Failed` | The environment ran, but Symposium or the interaction violated the contract. |
| `InfrastructureError` | Credentials, provider, runtime, environment, runner budget, or harness failed. |
| `Unavailable` | Preflight found that the selected adapter or environment lacks a required capability. |

A result may also carry modifiers that preserve important qualifications without creating another base result:

- `non-authoritative(contaminated-auth-context)` means local credentials could not be separated from user or agent configuration.
- `stability-warning(recovered-infrastructure-error)` means a recognized infrastructure failure occurred before the complete fresh-state retry passed.

Modifiers are recorded in the summary, journal, and aggregate reports. They never turn `Failed` into `Passed` or make a non-authoritative run satisfy a conformance requirement.

Explicitly requesting an unavailable combination exits unsuccessfully; ordinary `cargo test` remains unaffected. There is no expected-failure scenario result. A known product-gap reproducer still returns `Failed` when run directly.

The [coverage table](../coverage-and-ci/README.md#contract-table) records a `Gap(issue)` separately from completed tracer coverage. Its executable reproducer still returns `Failed`; the result vocabulary has no expected-failure state.

Failures name an owning phase such as `environment.prepare`, `symposium.cli`, `symposium.state`, `agent.start`, `agent.query`, `fixture.mcp`, `assertion`, or `cleanup`. A Symposium crash, missing prompt, wrong state, or completed agent query without its required witness is `Failed`.

Exceeding a scenario-owned token or operation limit is `Failed` because the witness did not fit its contract. Harness-controlled context that already exceeds the declared limit or an operator ceiling stopping the run is `InfrastructureError` owned by `runner.budget`. Missing trustworthy provider accounting makes a paid query `Unavailable`.

## Retries and cleanup

Assertion and Symposium failures are never retried automatically. An agent-free scenario with a recognized transient infrastructure error may retry once from fresh state. A scenario that contacted a paid provider is never retried automatically; another attempt requires a new explicit invocation. Individual steps are never replayed inside an existing container or query context.

Both attempts are preserved. A recovered run remains `Passed` with the `stability-warning(recovered-infrastructure-error)` modifier and attempt metadata, so infrastructure reliability still counts the transient.

On a deadline, the runner captures current evidence, attempts graceful termination, kills the complete process tree after a bounded cleanup deadline, and verifies that no process or container remains.

## Artifact safety

Artifacts live under `target/agent-tests/<run-id>/`. Every run keeps a compact summary. Failures keep sanitized journals, terminal output, selected logs, workspace diffs, agent events, explicitly allowed Symposium state, and a redaction report. Successful runs keep rich artifacts only with `--keep-artifacts`.

Capture is allowlist-based. The runner never archives a complete container, home, authentication directory, or process environment. Credentials are secret handles supplied only to the process that needs them. Known values, provider headers, credential-bearing URLs, command arguments, and environment fields are redacted in memory before persistence.

Every run injects harmless secret canaries and verifies that none survive. If sanitization cannot complete, rich artifacts are withheld and the summary reports the redaction failure. Upload-time filtering is not considered sufficient.
