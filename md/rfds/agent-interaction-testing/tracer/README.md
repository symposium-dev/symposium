# The first tracer

## Purpose

The tracer proves one complete interaction in both directions:

- a user accepts a dependency's offered skill and a real agent advertises and loads it; or
- a user declines the offer and the real agent does not receive it.

It also proves that Claude invoked Symposium's `SessionStart` hook. It does not generalize that result to every hook event, agent, or registry behavior.

## Registered scenarios

Execution profile is part of scenario identity:

| Scenario | Profile | Purpose |
|---|---|---|
| `dependency-consent-accept` | real process, PTY | prove the visible accept interaction and persisted state |
| `dependency-consent-decline` | real process, PTY | prove decline persistence and prompt suppression |
| `dependency-consent-accept-agent-fixture` | fixture agent | prove accepted skill delivery through a real agent |
| `dependency-consent-decline-agent-fixture` | fixture agent | prove declined skill exclusion through a real agent |

Implementations may share Rust helpers. Selecting an agent never upgrades a process-only scenario into an agent scenario.

## Fixture state

Each scenario begins with fresh project, Symposium, agent, cache, service, event, and artifact directories. The fixture project has a path-only dependency that offers one plugin awaiting consent; resolving or building the fixture requires no registry access.

The runner assigns controlled roots before the first child starts:

```text
SYMPOSIUM_HOME=<run>/symposium
SYMPOSIUM_USER_HOME=<run>/user
HOME=<run>/user
USERPROFILE=<run>/user
<adapter relocation variables>=<run>/user/...
CARGO_HOME=<run>/cargo
RUSTUP_HOME=<effective host rustup home>
```

`SYMPOSIUM_USER_HOME` is an internal, unstable testing seam. Every Symposium user-profile lookup goes through the resolver that honors it. `HOME` and `USERPROFILE` isolate child tools, including Claude on Windows. Each adapter supplies its own relocation variables; the first Claude adapter sets `CLAUDE_CONFIG_DIR=<run>/user/.claude`. The checkout artifact directory precedes other entries on `PATH`; the fresh Cargo home contains no competing `cargo-agents` binary.

On host runs, `RUSTUP_HOME` is preserved explicitly as a declared read-only toolchain dependency so the controlled home does not break the rustup Cargo proxy. The Linux container supplies its own pinned toolchain instead.

The Symposium home contains only harness policy before the first command:

```toml
auto-update = "off"

[telemetry]
enabled = false

[defaults]
symposium-recommendations = false
user-plugins = false
```

Any registry used by the fixture is a declared local path or local service. This policy prevents crates.io update checks, public recommendation refreshes, telemetry, and ambient user plugins from changing the journey.

The tracer therefore does not claim that `init` creates configuration from an entirely empty home. It tests agent registration, hook setup, consent, and delivery while preserving harness policy.

Fixture-agent variants ask the selected adapter to place a control skill in the same project scope where Symposium installs skills, without a `.symposium` marker. For the first Claude adapter this is `.claude/skills/`. If Symposium supports hooks for the selected agent, the adapter also installs an independent control hook in the corresponding controlled settings. Controls exist before Symposium writes agent configuration, so the journey also proves that sync preserves entries it does not own.

## Scripted user interaction

Initialization is noninteractive:

```text
cargo agents init --add-agent <adapter config name>
```

It runs over pipes. Supplying `--add-agent` makes current initialization CLI-driven and suppresses four prompts: agent selection, hook scope, auto-update, and telemetry. The first implementation substitutes `claude`; the scenario body obtains that value from the selected adapter.

`cargo agents sync` runs under a native PTY. The stable prompt anchor is:

```text
Dependency `<name>` provides <what>. Enable it?
```

The options and inputs are:

| Choice | Keys | Persisted effect |
|---|---|---|
| Ask me later | Enter | no decision |
| Enable | Down, Enter | add to `auto-enable` |
| No, don't ask again | Down, Down, Enter | add to `disable` |

This RFD commits only to Enable and No. Ask me later and Escape remain follow-up scenarios.

Every wait targets rendered terminal state and has a monotonic deadline. The runner never uses a fixed sleep to decide when to send input.

## Accept journey

The process-only accept scenario:

1. starts from the controlled fixture state;
2. runs `init --add-agent <adapter config name>` over pipes;
3. starts `sync` under a PTY;
4. waits for the exact dependency prompt;
5. sends Down, Enter;
6. waits for process exit;
7. requires successful exit;
8. requires `ConsentRequested` and `ConsentAnswered { decision: enable }`;
9. requires the persisted enablement entry; and
10. asks the adapter to require the fixture skill in the selected agent's project inventory.

The fixture-agent accept scenario starts with the control skill and, when supported, the control hook. It performs the same steps and verifies the installed files and preserved foreign controls before the agent starts. That ordering is required because a `SessionStart` hook can run auto-sync and could otherwise repair a failed explicit sync.

It then:

1. asks the adapter to verify that the exact project custom-skill inventory is control plus Symposium;
2. verifies that Symposium preserved every foreign control;
3. starts the pinned selected agent against its local model fixture;
4. when Symposium supports hooks for that agent, requires successful control-hook and Symposium-hook evidence; otherwise records the hook assertion as `Skipped(not-supported-by-agent)`;
5. scripts loading of the control skill;
6. requires the control advertisement and body witnesses;
7. scripts loading of the Symposium skill;
8. requires the Symposium advertisement and body witnesses;
9. waits for clean agent exit; and
10. rechecks the exact project custom-skill inventory.

## Decline journey

The process-only decline scenario follows the same initialization and prompt steps, then sends Down, Down, Enter.

After the first sync exits, it requires:

- `ConsentAnswered { decision: decline }`;
- the persisted disable entry; and
- no installed Symposium skill.

It starts `sync` again, waits for exit, and searches the completed rendered screen. The prompt anchor must be absent. The runner does not infer absence from a period of silence.

The fixture-agent decline scenario first performs the complete process-only decline journey, including initialization, the Down, Down, Enter selection, persisted state, and the prompt-free second sync. It begins with the same project-scoped control skill and, when supported, controlled-profile control hook, then:

1. asks the adapter to verify that the pre-agent project inventory contains only the control;
2. verifies that Symposium preserved every foreign control;
3. starts the pinned selected agent against the fixture model;
4. when Symposium supports hooks for that agent, requires successful control-hook and Symposium-hook evidence; otherwise records the hook assertion as `Skipped(not-supported-by-agent)`;
5. advertises and loads the control skill;
6. requires that no request contains the Symposium advertisement or body token;
7. waits for clean agent exit; and
8. verifies again that the Symposium skill is absent from disk.

The control and hook are positive controls. Without them, a disabled skill system or missing hook configuration could make the negative assertions pass vacuously.

## Skill witnesses

This section defines the Claude adapter's concrete proof for the shared `advertised` and `loaded` evidence. It is not a protocol requirement for later agents.

Every fixture-agent attempt generates four independent tokens:

```text
C_ADV_<run-nonce>   control frontmatter advertisement
C_BODY_<run-nonce>  control instruction body
S_ADV_<run-nonce>   Symposium frontmatter advertisement
S_BODY_<run-nonce>  Symposium instruction body
```

Tokens occur only inside the corresponding `SKILL.md`. They do not appear in crate names, dependency names, plugin names, directory names, prompts, Git status, or hook output.

Advertisement means that the frontmatter token appears in a main-loop request that exposes Claude's Skill tool. Loaded means that the body token appears in a later captured request after the fixture returns the corresponding Skill tool call.

The fixture selects requests by content, not position. It does not assume that the first request is the main loop or that a loaded body appears inside a particular JSON block.

## Model fixture conversation

The local endpoint implements the minimum Anthropic-compatible exchange discovered by the feasibility spike. Conceptually, the accepted script is:

```rust,ignore
fixture.when(request_with_skill_tool_and(control_advertisement))
    .respond(load_skill(control_name));

fixture.when(request_containing(control_body_nonce))
    .respond(load_skill(symposium_name));

fixture.when(request_containing(symposium_body_nonce))
    .respond(final_response());
```

The decline script loads only the control skill. Assertions inspect every captured request for forbidden Symposium tokens.

Full model prose is diagnostic. The final fixture response signals protocol completion; it is not a quality judgment.

## SessionStart witnesses

The committed hook contract is `delivery.hook.session-start`, not general hook delivery.

This tracer uses Claude's `SessionStart` and `hook_response` records to implement the shared hook evidence. Another adapter may name and expose its equivalent lifecycle differently.

The harness-installed control hook is a cross-platform helper invoked by absolute path. It has no dependency on Symposium and therefore cannot emit a Symposium `ProcessStarted` event. Instead it writes an independent nonce-bearing receipt containing its process ID and control-hook identity. The hook is observed only when Claude's stream contains a successful `hook_response` for its configured command and the receipt matches the current run. If it is missing or fails, Claude's hook path is not healthy enough to judge Symposium.

The Symposium hook is observed only when Claude's stream contains its successful `hook_response` and the hook subprocess emits a matching Symposium `ProcessStarted` event. A process that starts but fails or has no successful lifecycle response is a Symposium contract failure once the control hook has succeeded. Protocol purity is proved separately by `hook.stdout-protocol`; package 1 determines whether Claude's lifecycle stream exposes an additional parse outcome but the journey does not assume it does.

For Claude, both fixture-agent scenarios require both hook witnesses. The feasibility spike determines whether Claude passes `SYMPOSIUM_EVENT_DIR` into hook subprocesses. If it does not, the adapter writes the variable into the controlled Claude settings `env` block. For an agent where Symposium registers no hooks, the same skill scenario skips hook evidence explicitly instead of becoming unavailable.

Other hook events remain future work.

## Failure classification

Preflight runs first. Provenance is checked before any product attribution, preservation is checked before adapter health, and adapter controls are interpreted before product delivery evidence. The table below describes the initial Claude execution; an agent without Symposium hook support records the hook assertion as skipped and does not enter the hook-specific rows.

| Observation | Result |
|---|---|
| Claude executable or required capability is missing | `Unavailable` |
| Claude version differs from the pin | `Unavailable` |
| Direct CLI or agent-spawned hook provenance differs from the execution plan | `InfrastructureError(environment.provenance)` |
| Symposium removed or changed a harness control skill or hook entry | `Failed(settings.preserve-foreign)` |
| The independent control hook is absent, fails, or lacks either required witness | `InfrastructureError(adapter.claude.hooks)` |
| Matching pinned Claude does not advertise the control | `InfrastructureError(adapter.claude.fixture)` |
| Control is advertised but its body is not loaded | `InfrastructureError(adapter.claude.fixture)` |
| The control hook succeeds but Symposium's hook is absent | `Failed(delivery.hook.session-start)` |
| The control hook succeeds but Symposium's hook starts and fails | `Failed(delivery.hook.session-start)` |
| Control works but the accepted Symposium advertisement is missing | `Failed(delivery.skill.accept)` |
| Control works but the accepted Symposium body is not loaded | `Failed(delivery.skill.accept)` |
| Control works but a declined Symposium token appears | `Failed(delivery.skill.decline)` |
| Declined skill appears on disk after SessionStart | `Failed(delivery.skill.decline)` |

The runner never substitutes a live provider when fixture controls fail.

## Contract table

`Committed(package)` refers to the numbered work packages in the [implementation plan](../implementation/README.md), which is intentionally presented after this concrete tracer.

The table uses two states in this RFD:

- `Committed(package)`: the named implementation package must make the contract pass before the tracer is complete.
- `Direction(follow-up)`: the contract is relevant to the harness but outside this RFD's completion criteria.

| Rule ID | Contract | State | Required evidence |
|---|---|---|---|
| `hook.stdout-protocol` | Hook stdout contains only the selected agent protocol | `Committed(package 2)` | compiled process with pipes |
| `environment.user-home-isolation` | Host journeys resolve every Symposium user-profile path through the controlled root and leave Symposium-writable real-profile state unchanged | `Committed(packages 2, 3)` | resolver tests, process environment, decoy checks, scoped real-profile projection |
| `settings.preserve-foreign` | Init and sync preserve agent skill and hook entries they do not own | `Committed(packages 3, 4, 5)` | pre-agent inventories, settings checks, post-agent inventories |
| `adapter.claude.config-dir` | Claude global hook registration and removal follow `CLAUDE_CONFIG_DIR` and clean Symposium-owned legacy entries | `Committed(package 2)` | migration-focused process tests |
| `process.checkout-provenance` | Direct CLI and agent-spawned hook invocations use the planned checkout binary | `Committed(packages 2, 3, 4)` | separate startup events, paths, versions, digests |
| `consent.accept` | Down, Enter enables the displayed offer | `Committed(package 3)` | PTY, events, config, files |
| `consent.decline` | Down, Down, Enter persists decline and suppresses the later prompt | `Committed(package 3)` | PTY, events, completed rerun, config, files |
| `adapter.claude.control-skill` | Pinned Claude maps the project-scoped control skill to advertised and loaded evidence | `Committed(packages 1, 4, 5)` | captured requests and normalized evidence |
| `adapter.claude.control-hook` | Pinned Claude maps the preserved control hook to started and completed evidence | `Committed(packages 1, 4, 5)` | Claude lifecycle, independent helper receipt, and normalized evidence |
| `delivery.hook.session-start` | Claude successfully invokes Symposium's SessionStart hook | `Committed(packages 4, 5)` | Claude lifecycle and Symposium process evidence |
| `delivery.skill.accept` | Accepted skill is installed, advertised, and loaded | `Committed(packages 4, 5)` | files and captured requests |
| `delivery.skill.decline` | Declined skill remains absent before and after SessionStart | `Committed(packages 4, 5)` | exact inventories and captured requests |
| `consent.defer` | Enter records nothing | `Direction(follow-up)` | PTY and state |
| `consent.escape` | Escape leaves unshown candidates undecided | `Direction(follow-up)` | PTY, events, state |
| `enablement.disable-precedence` | Disable overrides use and auto-enable | `Direction(follow-up)` | logic and process evidence |
| `cache.expiration` | Expired cache input is reevaluated | `Direction(follow-up)` | logic and process evidence |
| `delivery.hook` | Other supported hook events reach an agent | `Direction(follow-up)` | event-specific witnesses |
| `delivery.mcp` | MCP registration reaches an agent | `Direction(follow-up)` | fixture-server protocol |

A later catalog can add `Covered` and `Gap(issue)` states when executable coverage is maintained. This RFD does not build catalog automation before the tracer exists.

## Completion criteria

The tracer is complete only when all committed rows pass at their required layers, including Windows and Linux for the agent-free PTY journeys and the Linux container for fixture-agent conformance.

A passing tracer proves only these named contracts. It does not prove general response quality, every registry rule, every Claude version, or behavioral consistency across agents.
