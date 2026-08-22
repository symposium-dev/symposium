# Coverage and CI

## Contract table

The tracer begins with a reviewed Markdown table of the Symposium promises it exercises. Each row has a stable rule identifier, a behavioral statement, its required test layers, and one state:

- `Committed(step)`: this RFD commits to implementing the row in the named tracer step. It becomes `Covered` after the required scenarios pass.
- `Covered`: every required tracer scenario exists and passes.
- `Gap(issue)`: the implementation is known to violate the specification, a linked issue owns the discrepancy, and an executable reproducer returns `Failed` when run directly.
- `Direction(follow-up)`: the rule is outside this RFD's tracer commitment and must be carried into a closing follow-up issue or RFD. It is not counted as tracer coverage.

Accepted RFDs and current reference documentation remain authoritative; this table is a coverage index rather than a replacement specification. It must not copy an implementation bug into the expected result.

Typed Rust scenarios name the rules they prove. After enough journeys exist to expose stable catalog requirements, a follow-up may make the table machine-readable, validate layer and operating-system obligations, and generate a coverage report. This RFD does not build that meta-tool before the first journey.

## Coverage layers

The matrix separates the boundary being exercised from the command used to run the test. In-process tests call Symposium's Rust entry points. Real-process tests spawn the compiled binary, using a PTY only for interactive commands and pipes for hooks and other noninteractive subprocesses. Real-agent tests are reserved for delivery that can fail only inside the agent. A deterministic black-box process regression may still run under `cargo test`; it does not become an agent test merely because it spawns a binary.

The matrix records intended obligations, including follow-on direction; it does not claim those rows are implemented by the tracer. Linux containers rehearse representative production paths rather than duplicating every in-process test.

| Rule ID | Contract | State | In-process | Real process | Real agent |
|---|---|---|---:|---:|---:|
| `consent.accept` | Undecided candidate is accepted | `Committed(steps 2, 3)` | required | required, PTY | not required |
| `consent.decline` | Undecided candidate is declined | `Committed(steps 2, 3)` | required | required, PTY | not required |
| `consent.defer` | Ask later records nothing | `Direction(follow-up)` | required | required, PTY | not required |
| `cli.noninteractive` | Noninteractive execution never prompts | `Direction(follow-up)` | required | required, pipes | not required |
| `enablement.disable-precedence` | Disable overrides other enablement | `Direction(follow-up)` | required | representative, pipes | not required |
| `cache.expiration` | Cache expiration reevaluates its input | `Direction(follow-up)` | required | required, pipes | not required |
| `hook.stdout-protocol` | Hook stdout contains only protocol output | `Committed(step 1)` | not sufficient | required, pipes | not required |
| `isolation.skill-inventory` | Isolated custom-skill inventory exactly matches the fixture | `Committed(steps 2, 3, 4)` | not required | required, pipes | required |
| `delivery.skill` | An enabled fixture skill reaches the selected agent | `Committed(step 4)` | not sufficient | required, pipes | one delivery smoke |
| `use.search-endpoint` | Non-workspace `use` search uses only its declared fixture endpoint | `Direction(follow-up)` | required | required, pipes | not required |
| `delivery.hook` | Hook delivery reaches an agent | `Direction(follow-up)` | required | representative, pipes | required |
| `delivery.mcp` | MCP registration reaches an agent | `Direction(follow-up)` | required | representative, pipes | required |

Operating-system applicability is recorded separately. A Linux-container pass cannot satisfy a Windows-native or macOS-native requirement.

The first audit must include conflicting enablement entries. The accepted registry contract says `disable` wins over `use` and `auto-enable`, even though one current implementation path checks `use` first.

## Tracer journeys

This RFD commits to two fresh-state consent journeys. The accepted branch runs real `init` and `sync`, answers the prompt, and verifies installation. Step 4 extends that branch with one bounded Claude query that proves the nonce-bearing fixture skill reached the agent. The declined branch verifies that nothing is installed, the decision persists, and a later sync does not ask again.

The tracer also fixes the hook stdout contamination bug and adds a deterministic black-box process regression proving that stdout contains only the hook protocol. It plants an isolation canary and verifies the exact custom-skill inventory owned by the fixture.

Ask-later and Escape, custom predicates, cache reuse and expiration, malformed registries, enablement precedence, non-workspace `use`, hook delivery, MCP delivery, and registry resynchronization remain stated follow-on families. They use deterministic or real-process coverage by default. A real agent is added only where delivery into the agent could fail.

## Command interface

The orchestration entry point is:

```console
cargo xtask agent-test [OPTIONS]
```

Initial options are:

```text
--list
--agent <agent>
--environment <host|container>
--scenario <name> ...
--symposium-bin <path>
--auth <api-key|local>
--max-agent-turns <count>
--max-provider-requests <count>
--max-input-tokens <count>
--max-output-tokens <count>
--max-tool-calls <count>
--confirm-paid-run
--keep-artifacts
```

`--scenario` is repeatable. No scenario means "print the execution plan," not "start an agent." A selection containing a real-agent journey requires an explicit agent and `--confirm-paid-run`. Missing runtime, credentials, or capability yields `Unavailable`; a requested container never silently falls back to the host.

The plan reports CLI-only and real-agent scenarios, scenario and operator token limits, maximum turns, provider requests, and tool calls, the provider-side spending cap, environment, binary provenance, and pinned runtime before execution.

## Cost and runtime controls

Each real-agent scenario declares maximum cumulative input, cache-read, cache-write, and output tokens; provider requests; user turns; tool calls; real-time deadline; and usage class. Paid execution requires trustworthy provider accounting. Cached tokens remain visible and count toward token limits even when their billable price is lower.

Scenario-owned limits define the product contract. Exceeding one is `Failed`. Operator flags and run-wide limits are protective ceilings. If a lower `--max-agent-turns` or run-wide cap stops an otherwise valid scenario, the result is `InfrastructureError` owned by `runner.budget`, never a Symposium failure. The execution plan shows both limits and their effective minimum before paid work begins.

Before any provider request, the runner checks the declared fixture and prompt inputs it controls. The adapter reports actual provider usage for agent-owned context. A scenario that cannot fit its declared limit is reduced before the limit is raised.

Authoritative paid tracer runs use a dedicated restricted provider key with a $5 monthly provider-side spending limit as the aggregate backstop. A host run using `--auth local` is non-authoritative, cannot claim that provider cap, and still obeys the scenario's hard token and operation limits. Real-agent concurrency is one. Credentials alone never enable paid tests: the user must select a real-agent scenario, name the agent, and pass `--confirm-paid-run`. Ordinary `cargo test` retains its explicit agent-testing gate.

Runtime reporting separates checkout build or image preparation, warm environment startup, agent execution, and assertion/evidence processing. This makes container overhead distinguishable from provider latency.

The tracer's provisional guard permits one user turn, at most four provider requests, three tool calls, 25,000 total input-side tokens across base input, cache reads, and cache writes, and 1,000 cumulative output tokens. Initial manual runs record actual usage so these limits can be reduced. If the pinned runtime cannot produce the nonce witness within the guard, the prompt, tools, fixture, and context are reduced before any limit is raised.

At Sonnet 5's standard price after its introductory period, $3 per million input tokens and $15 per million output tokens, the provisional base-token ceiling is approximately $0.09 per run. The runner reports a conservative allowance of $0.20 per run for cache-price differences while the provider key caps the month at $5. The estimate is recalculated when the model pin or [provider pricing](https://www.anthropic.com/research/claude-sonnet-5) changes; tokens remain the primary limit and dollars are derived reporting.

## Tracer CI boundary

- Fast deterministic tests block every pull request.
- Stable native agent-free process and PTY scenarios may become PR-blocking.
- A small agent-free Linux-container suite may graduate if its measured runtime is acceptable.
- Real-agent tracer journeys are explicitly selected, manually run, and non-gating while the runner is experimental.

Agent-free tests graduate after an observation period with no unexplained flakes, acceptable runtime, actionable failure artifacts, reliable cleanup, and consistently successful secret-canary validation. Provider credentials are never exposed to fork pull requests.

Scheduled ownership, quarantine, pass-rate, and release-gating policy for real-agent tests are outside the tracer contract. A follow-up may propose those mechanisms using measured reliability, runtime, and cost rather than assumptions.

## Post-tracer direction

After the tracer, tracked follow-ups can expand the contract table across registry, discovery, predicate, cache, and delivery behavior. They can add every consent branch, representative Linux-container and native Windows/macOS lanes, fake and fixture-ACP adapter contracts, and selected hook and MCP witnesses.

Catalog automation, full release reporting, a latest-agent canary, and broader CI graduation are separate commitments informed by tracer evidence. They are not acceptance criteria for this RFD.
