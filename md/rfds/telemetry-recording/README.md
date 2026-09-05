# Telemetry: recording events

## TL;DR

- Replace the experimental event list with a question-driven, closed schema for local telemetry.
- Keep recording off by default, per-user, local only, and gated by versioned consent. Existing unversioned opt-ins must consent again.
- Record observed sessions, configured agents, public package/extension resolution, aggregate agent skill invocation, aggregate hook reliability, command use, and known storage gaps.
- Never record individual prompt/tool activity rows, prompt or tool details, paths, private names, dependency snapshots, or a global installation/workspace id.
- Use purpose-scoped pseudonyms and non-waiting, best-effort recording. Telemetry failure must not disrupt hooks, sync, or commands.
- Defer upload, server-side handling, and subjective feedback to separate follow-up efforts tracked under [#246](https://github.com/symposium-dev/symposium/issues/246).

Supporting pages: [data contract and exclusions](./contract/recorded-data.md), [telemetry command reference and consent disclosure](./reference/telemetry-command.md), and [configuration and consent states](./reference/configuration.md).

## Motivation

Symposium has no production evidence about which integrations people reach, which public packages resolve to plugins and skills, whether agents activate those skills, or whether hooks are slow or failing.

The existing experimental telemetry was never wired into production. Its proposed prompt and tool rows would be high-volume without answering those questions.

The team instead needs a low-volume, inspectable record of reach, resolution, skill activation, and reliability. That evidence can guide agent support, recommendation work, and investigation of hook cost. The schema must be agreed before collection begins so every field has a stated use and privacy boundary.

This telemetry can show that a public skill resolved and that an agent activated it. It cannot show that the agent followed the skill or that the skill improved the task outcome. Version 1 observes structured skill activation only for Claude, but the event model and attribution boundary are agent-neutral.

Controlled evaluation and explicit feedback remain separate follow-up efforts tracked under [#246](https://github.com/symposium-dev/symposium/issues/246).

## Change in a nutshell

From the user's perspective, telemetry follows one inspectable lifecycle:

1. Telemetry begins disabled. `cargo agents init` and `cargo agents telemetry enable` present the same team-approved disclosure and default to no.
2. After versioned consent, recording-capable Symposium operations write only the disclosed local events and aggregates.
3. `cargo agents telemetry status` explains the effective state, while `show` exposes the stored bytes.
4. `disable` stops future recording without deleting existing files; `clear` removes recorded data, and `reset-identifiers` rotates future identifiers.
5. Nothing is uploaded under this RFD.

Recording uses a JSONL schema built around those questions. A full sync emits one summary plus safe package and extension relationships. Hook and agent skill observations update bounded daily snapshots:

```text
observed session -> session_start
full sync        -> resolution_summary
                 -> package_resolution*
                 -> extension_resolution*
completed hook   -> hook_metrics snapshot
                 -> plugin_hook_metrics snapshot
agent skill use  -> extension_invocation_metrics snapshot
command          -> command
```

The [exhaustive data contract](./contract/recorded-data.md) defines the fields, enums, exclusions, and complete JSONL examples. The main design has these invariants:

- Only `Enabled` recording can create telemetry state or files.
- The producer accepts closed typed data, never arbitrary metadata or raw errors.
- Public names require allowlisted provenance and validation; everything else is unnamed or opaque.
- Pseudonyms are scoped to one measurement and normally rotate every 30 days.
- Routine hook and skill invocations update aggregates; they never become activity rows.
- A non-waiting process lock may drop a whole batch or observation but never delays another recorder.
- Files remain through D30 and first become eligible for lazy deletion on D31.
- Nothing is uploaded by this RFD.

## Detailed plans

### Measurement questions

All measures describe opted-in installations, not the whole user population. Reports must state that selection bias. This RFD proposes the questions below to make the activity goals in [#246](https://github.com/symposium-dev/symposium/issues/246) and [#243](https://github.com/symposium-dev/symposium/issues/243) measurable.

| #   | Question                                                            | Operational definition                                                                                                             |
| --- | ------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| Q1  | Is another session observed after an installation's first session?  | Deduplicate `session_start` by `retention_subject` and measure observed cohort days D1, D7, and D30.                               |
| Q2  | Which plugins and skills resolve?                                   | Count `extension_resolution` occurrences and scoped subjects by public extension and safe witnessed path.                          |
| Q3  | Which public packages and versions occur, and what do they resolve? | Count `package_resolution` by public coordinate and `extension_match`; use paths carried by `extension_resolution`.                |
| Q4  | Are Symposium and plugin hooks failing or slow?                     | Use daily invocation/attempt counters, outcomes, fixed latency histograms, and complete identified-session impact counts.          |
| Q5  | Which agents, versions, and platforms have reach?                   | Keep configured-agent observations separate from observed hook sessions.                                                           |
| Q6  | Which command surfaces are used?                                    | Count completed built-ins and eligible public plugin commands without arguments.                                                   |
| Q7  | Which resolved public skills do agents actually activate?           | Count completed `extension_invocation_metrics` and complete identified-session counts by public skill and safe resolution subject. |

These questions have specific limits:

- For Q1, the first observed `session_start` for a `retention_subject` establishes D0. D1, D7, or D30 is present when at least one later session is observed on that cohort day, from the same or a different agent. Multiple sessions on one day count once. This measures a later observed session, not one long session or continued value; session start runs automatically once Symposium is installed.
- Q2 proves resolution, not activation.
- Q3 records relationship edges, not a complete dependency set.
- Q4 counts completed observations, so host termination can be invisible.
- Q7 proves activation, not that the agent followed the skill or completed the task better. Version 1 can answer Q7 only for Claude.

### Scope and boundaries

This RFD defines the complete scope of local recording: consent, provenance, resolution evidence, event families, identifiers, agent capability gaps, storage, controls, rollout, documentation, and tests.

It covers [#242](https://github.com/symposium-dev/symposium/issues/242) and the activity-metric portion of [#243](https://github.com/symposium-dev/symposium/issues/243).

It does not cover #243's experimental outcome signals, [#244 uploading](https://github.com/symposium-dev/symposium/issues/244), or [#245 feedback collection](https://github.com/symposium-dev/symposium/issues/245). It also leaves out new host-hook timeouts, public-identity inference from arbitrary git URLs, private-source names, and concurrent full-sync mutation of installed skills. Telemetry locking protects telemetry only.

### Vocabulary

**Discovery** finds candidate packages, plugins, and skills before consent is known. It never records telemetry.

**Resolution** selects the packages and extensions that actually apply. Resolution events describe the final set and the evidence that selected it.

An **observed session** is one whose registered `SessionStart` hook ran. It is narrower than a configured agent.

A **skill activation** is a completed, structured agent invocation of a particular installed skill. It does not mean the agent followed the skill or that the skill improved the result.

### Consent and activation

Telemetry remains a per-user setting:

```toml
[telemetry]
enabled = true
consent-version = 1
```

| Effective state   | Condition                                | Recording                                     |
| ----------------- | ---------------------------------------- | --------------------------------------------- |
| `Disabled`        | `enabled` absent or false                | None; create no telemetry directory or state. |
| `ConsentRequired` | enabled but consent version absent/stale | None until current disclosure is accepted.    |
| `Enabled`         | enabled with current consent version     | Events defined by this contract.              |

Existing users with an unversioned `enabled = true` must consent again. Interactive `init` and `telemetry enable` present the same team-approved disclosure and default to no. Non-interactive calls require an explicit acknowledgement; editing the boolean alone cannot upgrade consent.

The [version 1 disclosure requirements](./reference/telemetry-command.md#disclosure-requirements) define what the disclosure must cover. The command reference includes a complete example for review, but its wording is not fixed implementation text. The team approves the final wording before recording is activated. `init` and `telemetry enable` then use the same snapshot-tested string.

Editorial changes that preserve coverage and meaning do not require renewed consent. Increase the consent version when collection expands categories, user-derived fields, linkability, timestamp precision, public-name eligibility, or retention, or weakens a normative exclusion. Narrowing collection does not require renewed consent. The [telemetry configuration reference](./reference/configuration.md) defines the effective-state semantics.

Adding an agent enum value creates a new schema version for each affected event kind because existing typed readers cannot parse the new value. It does not by itself require renewed consent when the fields, categories, timestamp precision, and correlation boundaries remain inside the accepted disclosure.

A new field, hook surface, or linkage for that agent does require a consent-version increase.

No production event is wired until the typed catalogue, privacy contract, controls, documentation, and current consent flow all land. Earlier implementation steps remain inert behind `ConsentRequired`.

### Event contract

The schema is closed and typed. It accepts fixed variants, enums, counters, and bounded structures. It rejects arbitrary metadata maps, raw PM or agent payloads, errors, debug strings, command arguments, and sanitization fallbacks.

Counters use checked unsigned arithmetic. Overflow drops the batch or observation.

Every row has a per-kind schema version, fixed kind, random row id, UTC day, and Symposium version. Only completed `session_start` and `command` rows carry a wall-clock timestamp, truncated to one second. Millisecond durations remain because Q4 needs mergeable distributions.

| Kind                           | Purpose and emission                                                                                                 |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------------- |
| `session_start`                | A registered session-start hook completed; optional agent-scoped session id and D0-D30 return subject.               |
| `agent_configuration`          | Daily configured/not-configured observation for each supported agent.                                                |
| `resolution_summary`           | One completed full-sync result with public/unnamed inputs, reason counts, resolved artifacts, changes, and duration. |
| `package_resolution`           | One eligible public input coordinate with `public`, `unnamed_only`, or `none` extension match.                       |
| `extension_resolution`         | One public plugin/skill and one bounded safe path that selected it.                                                  |
| `hook_metrics`                 | Cumulative daily counters and latency histogram per agent, hook, and identifier epoch.                               |
| `plugin_hook_metrics`          | Cumulative daily counters/histograms per agent, hook, epoch, and bounded plugin bucket.                              |
| `extension_invocation_metrics` | Cumulative daily agent skill-attempt, completion, and failure counters per bounded public or unnamed bucket.         |
| `command`                      | One completed eligible top-level command without arguments.                                                          |
| `storage_limit`                | At most one daily marker naming the low-volume operation whose whole batch did not fit.                              |

The `session_start` event is authoritative for observed-session and return measurements (Q1 and Q5). A `hook_metrics` row whose hook is `session_start` measures only that hook surface's reliability and latency for Q4; its invocation count is not a session count.

Ordinary hook lookup emits no resolution summary. A structured agent skill-use observation may update `extension_invocation_metrics` without emitting a resolution event. Version 1 obtains that observation from Claude's `Skill` signal.

Internal hook dispatch, telemetry management commands, and ineligible external commands emit no command event. See [What Symposium records](./contract/recorded-data.md) for exact fields and invariants.

### Event producers

Producers return typed observations or reports. They never serialize telemetry or write files. The `Recorder` checks consent, applies public-identity and correlation rules, derives identifiers, and sends accepted event batches or aggregate updates to storage.

| Rows                                                               | Producer boundary                                                                                                                          |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `session_start`                                                    | The outer hook wrapper, after a registered `SessionStart` completes successfully.                                                          |
| `agent_configuration`                                              | The first non-telemetry recording-capable invocation each UTC day, as one all-agent snapshot derived from Symposium's per-user agent list. |
| `resolution_summary`, `package_resolution`, `extension_resolution` | One structured full-sync report assembled while resolving and installing; telemetry does not rerun predicates or rediscover relationships. |
| `hook_metrics`                                                     | The outer hook wrapper, after the hook's final outcome and duration are known.                                                             |
| `plugin_hook_metrics`                                              | The plugin dispatcher, around each applicable plugin's preparation and execution.                                                          |
| `extension_invocation_metrics`                                     | The Claude `Skill` adapter normalizes targeted pre/success/failure hooks, then the generated installation index supplies safe attribution. |
| `command`                                                          | The top-level CLI dispatcher, after an eligible built-in or public plugin command reaches an outcome.                                      |
| `storage_limit`                                                    | The JSONL sink itself, when a complete low-volume batch cannot fit.                                                                        |

Agent and package-manager payloads provide input to existing Symposium operations; they do not emit arbitrary telemetry. If a producer cannot construct a complete typed observation, it records nothing for that observation.

### Identifiers and correlation boundaries

When enabled recording first needs identity state, Symposium atomically creates a random 32-byte key in private `<config-dir>/telemetry-state.toml` (default `~/.symposium/telemetry-state.toml`). This file is separate from the inspectable `<config-dir>/telemetry/` data directory. Symposium creates and replaces it with owner-only permissions where the platform supports them.

Identifiers use the first 128 bits of HMAC-SHA-256 over a domain, locally anchored 30-day window, and exact dimension:

```text
HMAC(key, "telemetry:<domain>:v1\0" || window || "\0" || dimension)
```

The dimension limits what an identifier can link. It represents one installation for one package, agent, or command dimension, never the installation globally.

Private state keeps the identity key and the current identifier-window and return-cohort anchors. Every recorder reads that state under the telemetry lock, so the same domain, window, and dimension produce the same subject across processes and restarts.

Normal 30-day rollover changes the window input rather than replacing the key. `disable` and `clear` preserve the key and anchors. Renewed consent and `reset-identifiers` replace the key and start a new cohort.

The key is private state, not anonymized telemetry. Someone who has it can recompute candidate identifiers. Telemetry commands therefore never print it, and it remains outside the inspectable telemetry data directory.

| Identifier          | Scope                                                                                            |
| ------------------- | ------------------------------------------------------------------------------------------------ |
| `event_id`          | One row; random rather than derived.                                                             |
| `session_id`        | Agent + vendor session id + 30-day window; optional.                                             |
| `retention_subject` | One observed-session D0-D30 cohort; `session_start` only.                                        |
| `agent_subject`     | One agent + 30-day window.                                                                       |
| `package_subject`   | One safe public package coordinate + 30-day window.                                              |
| `extension_subject` | One safe target/path + 30-day window; shared by its resolution and public invocation aggregates. |
| `hook_subject`      | One agent/hook surface + 30-day window.                                                          |
| `plugin_subject`    | One safe public plugin + 30-day window.                                                          |
| `command_subject`   | One safe command coordinate + 30-day window.                                                     |

The return subject is the sole cross-agent exception: it deduplicates Q1 but cannot link to other event kinds. A cohort remains stable through D30; the next observed session starts a new cohort. Accepting new consent or resetting identifiers rotates the key and starts another cohort.

`session_id` is absent when the agent supplies none, including Copilot. Raw vendor ids never enter events or an unkeyed hash. There is no global installation/workspace id, and future analysis or upload must not reconstruct one. Missing identity state is created only when enabled; malformed existing state stops recording until explicit identifier reset.

### Public package and extension identity

Each package manager must return typed provenance with the resolved coordinate: registry URL, git, path, workspace, or unknown. Core, rather than the package manager, maps allowlisted public registries to stable ecosystem labels.

Raw URLs and provenance never enter an event. Sources outside the allowlist are private and unnamed by default.

A named package requires allowlisted provenance, a valid public name, and an exact resolved version. Other inputs increment `unnamed_packages` and exactly one reason: `private_registry`, `git`, `path`, `workspace`, `unknown_source`, or `invalid_coordinate`.

Source reasons take precedence. `invalid_coordinate` applies only to an otherwise public source with an invalid coordinate. Exact versions let the team isolate version-specific recommendation gaps.

`package_resolution.extension_match` distinguishes:

- `public`: at least one eligible public extension matched;
- `unnamed_only`: extension content matched, but none was safe to name;
- `none`: no resolved extension matched.

Plugin, skill, and command names follow the same public-by-allowlist rule. This requires extending the current PM representation, which loses source provenance after loading metadata.

### Complete safe resolution paths

`extension_resolution` records the actual successful package-to-plugin-to-skill path. Predicate evaluation produces evidence in its original pass; telemetry never re-evaluates a predicate.

Safe nodes are public `package` and `extension` coordinates, `all` contributors, the successful `any` branch, an opaque `not` marker, or `opaque` with a fixed reason: `private_source`, `non_package_predicate`, or `limit`.

Shell commands, paths, environment values, custom predicate details, workspace members, wildcards, private names, and a negated child never enter the path.

Witness depth counts nested evidence nodes from the root, which is level 1, to a terminal package, extension, `not`, or opaque node. A subtree that would exceed level 8 becomes `opaque: limit`. The complete path is also limited to 16 evidence leaves and 4 KiB. These limits do not count filesystem path components; filesystem paths are never recorded.

Full sync builds safe evidence for successful installations because the generated attribution index needs it even when telemetry is disabled. Only an enabled recorder serializes that evidence as telemetry.

Cached booleans for non-package predicates may synthesize `opaque: non_package_predicate`. Caching an entire `PredicateSet` would discard the successful branches and witnesses. Supporting such a cache would require a different witness design.

This preserves actionable resolution relationships without recording a complete dependency snapshot. It proves that an extension resolved. A matching `extension_invocation_metrics` row separately proves that an agent activated the installed skill; version 1 can produce that row only for Claude.

### Hook aggregation and version 1 agent coverage

Each completed hook merges into one daily `hook_metrics` row per agent, surface, and active identifier epoch. Per-plugin preparation and execution merge into `plugin_hook_metrics`. An identifier reset can create another epoch row on the same day.

Histogram bounds are fixed at `[5, 10, 25, 50, 100, 250, 500, 1000]` milliseconds so rows remain mergeable.

Outcome counters and histograms have the exact sum invariants defined in the data contract. Identified-session counts are all-or-nothing: every observation must supply an id, and each keyed set is limited to 256.

On a missing id, overflow, or state/snapshot mismatch, Symposium discards the sets. The row remains incomplete for that day rather than publishing a plausible partial count.

Private plugins merge into unnamed buckets. At most 128 public-plugin rows are named per UTC day across agents, hooks, and identifier epochs; later names merge into overflow buckets. Extension invocation metrics have a separate daily limit of 128 public skills.

Each named subset is first-observed. Earlier-in-day plugins or skills are therefore overrepresented when the limit is reached. Analysis must report overflow and must not treat named rows as a random sample. The combined aggregate snapshot may occupy at most 512 KiB of the shared daily allowance.

Hook rows disclose exact daily counts. `pre_tool_use`, `post_tool_use`, and `user_prompt_submit` therefore approximate daily tool/prompt activity even without individual records. Extension invocation rows disclose exact daily skill-attempt, completion, and failure counts. The disclosure states both explicitly.

This matrix defines which agent signals version 1 records. It is part of the producer contract, not implementation status or telemetry priority. Unsupported absence means unknown, not zero.

| Agent          | Configuration | `SessionStart` | Session id | Fresh/resume | `Stop` | Skill invocation           |
| -------------- | ------------- | -------------- | ---------- | ------------ | ------ | -------------------------- |
| Claude Code    | yes           | yes            | yes        | yes          | yes    | attempted/completed/failed |
| Codex CLI      | yes           | yes            | yes        | yes          | no     | unsupported                |
| GitHub Copilot | yes           | yes            | no         | no           | no     | unsupported                |
| Gemini CLI     | yes           | yes            | yes        | no           | no     | unsupported                |
| Kiro           | yes           | yes            | yes        | no           | no     | unsupported                |
| OpenCode       | yes           | no             | n/a        | n/a          | no     | unsupported                |
| Goose          | yes           | no             | n/a        | n/a          | no     | unsupported                |

Configured reach covers all seven agents; observed-session measures cover only registered hook integrations. `Stop` and structured skill invocation remain Claude-only in version 1. An unsupported agent produces no invocation row, which means unknown rather than zero. `Stop` is not required by Q1-Q7.

### Skill invocation attribution

Agent-specific parsing ends at a normalized extension-use observation. The Claude adapter accepts only a fixture-tested `Skill` tool shape and produces a skill identifier plus one phase: `attempted`, `completed`, or `failed`. Raw Claude input and ephemeral invocation ids do not enter the recorder.

Full sync writes a versioned installation index under the agent skills parent at `.symposium/index-v1.json`. This generated, gitignored file is installation state, not configuration or telemetry.

The index maps the actual agent-facing identifier to the installed directory, marker fingerprint, eligibility, and safe public coordinate or path when one exists. Private identifiers may remain in the local index because they already exist in installed skill content. They are never serialized as telemetry.

The index is atomically replaced after sync determines which installations succeeded. Before naming a public invocation, lookup verifies the Symposium marker and fingerprint. A hook sees an old or new complete index; a missing, corrupt, or stale mapping never falls back to a path or content guess.

Public matches reuse the `extension_subject` derived for the selected safe resolution path. Other observations merge into `unnamed` rows with one fixed reason: `ineligible`, `not_indexed`, `attribution_unavailable`, `ambiguous`, or `invalid_signal`.

`not_indexed` means a valid attribution index has no matching agent-facing identifier. `attribution_unavailable` means the index is missing, corrupt, or stale. After 128 named public-skill rows in one UTC day, further public matches merge into `overflow`.

Claude's existing `PreToolUse` and `PostToolUse` hooks increment attempts and completions. A `PostToolUseFailure` registration matched only to `Skill` increments failures; it does not create generic failure-surface metrics.

Each phase update is an independent lower bound. If one update is lost, completed plus failed need not equal attempted and may exceed it.

The same all-or-nothing 256-id rule applies to attempted and completed distinct-session sets. A missing id, overflow, or state/snapshot mismatch makes both counts incomplete for that row and day. Completion proves activation only; it does not show whether the agent followed the instructions or improved the result.

### Recording architecture

| Unit                      | Responsibility                                                                    |
| ------------------------- | --------------------------------------------------------------------------------- |
| `Recorder`                | Enforce consent, buffer typed batches, and isolate recording failures.            |
| `IdentityDeriver`         | Own identity state and derive scoped pseudonyms.                                  |
| `PublicIdentityPolicy`    | Convert PM provenance into safe coordinates or unnamed counts.                    |
| `ResolutionWitness`       | Carry evidence through one-pass resolution.                                       |
| `InstalledExtensionIndex` | Persist actual agent-facing skill attribution after successful installation.      |
| `ObservationRouter`       | Normalize agent-native extension-use signals and send them only to active sinks.  |
| `MetricAggregator`        | Merge hook, plugin-hook, and extension-invocation observations into bounded rows. |
| `JsonlSink`               | Lock, cap, retain, append/replace, inspect, and clear files.                      |

Only an enabled `Recorder` owns the telemetry sink and identity components. Call sites cannot write directly. The generated installation index is functional sync state and exists independently of telemetry consent.

A sync returns a structured report with provenance and witnesses. That report drives skill installation, atomic index replacement, and, when recording is enabled, one sanitized relationship batch.

A hook prepares its agent response before converting timings and outcomes into aggregate updates. When telemetry is disabled and no other sink is active, the observation router does not load the index or construct an extension-use observation.

Commands are measured once at top-level dispatch. Raw errors never enter telemetry.

### Storage, concurrency, and retention

#### Files and state

Low-volume rows append to `events-YYYY-MM-DD.jsonl`. Current daily hook, plugin-hook, and extension-invocation aggregates live in a bounded, atomically replaced `metrics-YYYY-MM-DD.jsonl` snapshot under the inspectable telemetry data directory.

The sibling private `telemetry-state.toml` holds the identity key, cohort and cleanup metadata, marker state, and temporary keyed session-count sets. These sets are never emitted and expire at day rollover. The telemetry lock remains in the data directory and guards data and private state mutations.

#### Concurrent writes and failure

Recorders make one non-waiting exclusive-lock attempt. Contention drops the entire buffered batch or aggregate observation. Event batches serialize before one append so concurrent lines cannot interleave. Snapshot updates use same-directory temporary replacement.

Private-state replacement uses a temporary file beside `telemetry-state.toml` in the config directory. Abandoned state and snapshot temporaries are ignored and cleaned lazily under the telemetry lock. No `fsync` is promised, so a crash can still lose the latest update. Contribution counts detect state and snapshot divergence and permanently mark affected daily session counts incomplete.

Aggregate counters are lower bounds. No durable counter can quantify observations lost to lock contention, process termination, or I/O failure because those conditions can also prevent writing the counter. A cap-only counter would not measure total loss.

#### Size and retention

The event file, aggregate snapshot, and reserved maximum-size `storage_limit` row share 8 MiB per day. This is a safety ceiling, not expected volume or preallocation. It bounds damage from a producer bug or unexpectedly large resolution batch; normal recording should remain well below it.

Together with D31 expiry, the daily allowance bounds ordinary retained telemetry near 248 MiB, excluding temporary files and private state. Aggregate metrics receive at most 512 KiB. An oversized metric update is dropped without stopping low-volume events. An ordinary batch that cannot fit is replaced by the daily marker, and ordinary recording stops for that day. Relationship batches are never split.

Files survive D30 and become eligible for lazy deletion when `current_day - file_day > 30`, first on D31. `clear` deletes event and metric files plus pending count sets, but preserves consent and identity/cohort state. `reset-identifiers` rotates future identifiers without rewriting old files.

`disable` stops recording but keeps files by default. Uninstalling Symposium also leaves them. The [telemetry CLI reference](./reference/telemetry-command.md) defines the exact command behavior.

#### Multiple agents

Concurrent agents produce separate session and agent/hook rows. The same public package/path derives the same dimension subject for unique-install deduplication, but no project id links the agents. A simultaneous flush may be dropped; concurrent mutation of installed skills remains a separate problem.

### Schema evolution

Schema versions are per event kind. Semantic or correlation changes create a new version. Privacy expansions also require a new consent version.

Readers retain malformed and unknown lines, count them separately, and exclude them from typed analysis. Raw `show` preserves their bytes.

The ["What is never recorded" section of the data contract](./contract/recorded-data.md#what-is-never-recorded) is a producer rule. In summary, telemetry excludes prompt and tool content, per-invocation rows, raw errors and payloads, paths and workspace identity, environment/machine/account values, private-source names, arbitrary URLs, global identifiers, and timestamps finer than one second.

Here, the per-invocation exclusion includes individual skill activations and raw agent-facing skill identifiers.

### Drawbacks and limitations

This design accepts the following costs and limits:

- Opt-in measurements describe participating installations, not the complete user population. Reports must state this selection bias.
- A completed skill activation does not prove that the skill was followed or improved the task. Version 1 also obtains structured skill-use observations only from Claude.
- Scoped pseudonyms do not make a local directory anonymous. File/day/order and one buffered sync can expose co-occurrence; unusual public versions, public skill-use counts, agent/platform combinations, and exact counts can fingerprint an installation.
- Best-effort recording undercounts activity. Busy multi-agent sessions contend more, terminated hooks lose final observations, and unsupported agents have configuration but not session or skill-invocation observations. A failure may also prevent writing a durable dropped-update counter, so the missing data cannot be measured completely.
- Recording performs bounded in-process work and one non-waiting lock attempt. It does not promise zero latency, although failure and contention never change the user operation's result.
- The daily safety cap permits ordinary retained data near 248 MiB through D30. The implementation also adds cross-cutting provenance, witness, attribution, aggregation, consent, and schema-maintenance work.

### Rationale and alternatives

#### Record no production telemetry

This avoids the privacy, storage, and implementation costs above, but leaves Q1-Q7 unanswered. Controlled evaluation can determine whether a skill helps in a test scenario; it cannot show production reach, resolution gaps, or hook reliability.

#### Record every hook and skill invocation

Hooks are high-volume; Q4 needs rates and distributions, not traces. Resolution edges are low-volume and Q2/Q3 need the actual safe package-to-plugin-to-skill relationship. Aggregating hook and skill activity while retaining safe resolution events preserves that product signal without creating per-invocation rows.

#### Defer scoped identifiers, provenance, and witnesses until upload

These mechanisms already serve local measurement and privacy. Q1-Q7 need narrow deduplication and relationship evidence. Provenance prevents private coordinates from reaching an inspectable or shareable telemetry directory.

Recording plain coordinates now would create weaker local files and permanently lose evidence needed by the measurement questions. `event_id` also gives cumulative metric rows stable identity across snapshot replacement and reset epochs.

#### Omit exact package versions

A package name alone cannot distinguish a missing recommendation from a version-specific resolution gap or regression. Exact versions provide that diagnostic only after the PM proves an allowlisted public origin and validates the coordinate; private and local versions remain unnamed.

#### Record a complete dependency snapshot

Per-package frequency and safe extension paths answer Q3 without an explicit project fingerprint. Same-file/day/batch order can still reveal co-occurrence locally, but a dependency-set or resolution id would make that linkage direct and persistent.

#### Use SQLite instead of JSONL

Low-volume events append, while hook metrics form a small bounded daily snapshot. JSONL keeps both byte-inspectable, preserves unknown versions and malformed lines, and needs no database. The process lock plus atomic snapshot replacement supplies the required concurrency behavior.

### Unresolved questions

No telemetry-contract question is intentionally left open for acceptance. Two implementation inputs remain: Step 7 requires team approval of the exact disclosure wording, and Step 5 requires before-and-after hook measurements.

If either requires changing collection, privacy, or failure semantics, the RFD must be amended before recording is activated.

### Future possibilities

The event catalogue is closed for consent version 1, but new measurements can be added as typed, versioned event families. Every family goes through `Recorder` and remains subject to consent, public-identity rules, storage caps, retention, failure behavior, and the never-record exclusions.

#### Additional agent adapters

Agent-native extension-use payloads remain behind the normalized observation boundary. Version 1 implements only the Claude `Skill` adapter. Supporting another agent requires a thin adapter, native-signal conformance fixtures, capability documentation, and an event schema update; it does not duplicate attribution, aggregation, or storage.

#### Plugin telemetry

The same model can later support eligible public plugins. A plugin would declare bounded feature names in its manifest and report only core-defined aggregates. Symposium would own and validate the schema, record and use the data, and optionally provide aggregate reports to plugin authors.

Plugins could not add arbitrary fields, bypass consent, or write directly to storage. This RFD implements only the event families listed above. Plugin reporting requires a separate implementation and disclosure before collection begins.

#### Controlled evaluation

Production telemetry and a future evaluation harness may consume the same normalized extension-use observation, but they must use separate sinks and contracts. The telemetry sink writes only the daily aggregates defined here.

An explicitly started harness may retain detailed per-run evidence in its isolated evaluation workspace. It does not read telemetry JSONL or rely on rotating telemetry identifiers.

This RFD preserves that internal seam but does not define a harness, scenario format, trace format, outcome grader, or token accounting. Controlled with/without comparisons answer whether a skill improved a task; production telemetry does not.

#### Upload

Accepting this RFD is not consent to upload. A future RFD must define transport/authentication, renewed consent and scheduling, retry/idempotency, server retention/access/deletion, reporting thresholds, and incident handling.

Upload may use only accepted local fields and must preserve scoped-correlation boundaries. It cannot create a global subject from identifiers, file order, batch membership, request grouping, or transport metadata. `event_id` and per-kind versions enable retry and mixed-version handling; they pre-approve no transport.

### Proposed documentation

- [What Symposium records](./contract/recorded-data.md): normative fields, enums, examples, and exclusions.
- [`cargo agents telemetry`](./reference/telemetry-command.md): controls, files, inspection, and concurrency.
- [Telemetry configuration](./reference/configuration.md): consent and effective-state semantics.

These remain proposed pages until implementation lands; shipped design/reference chapters continue to describe the current binary.

## Frequently asked questions

### What does pseudonymous mean here?

A scoped identifier comes from a random local secret rather than machine identity. It still links observations inside one stated purpose and window. Rotation and domain separation limit that linkage; they do not make the local files anonymous.

The secret key is private state, not anonymized telemetry. It is not included in telemetry because someone who has it can recompute candidate identifiers.

### Does a completed skill activation mean the skill helped?

No. It means an agent successfully activated the installed skill; version 1 can observe this only for Claude. It does not show whether the agent followed the instructions or whether the task result improved. That causal question requires a controlled evaluation comparing equivalent runs with and without the skill.

## Implementation plan and status

The seven steps below are PR-sized. Steps 1-6 keep production collection inert. Step 7 activates the current consent version only after every producer and control is present.

Tests and benchmarks may construct an enabled recorder only through a test-only API bound to a temporary telemetry home. There is no runtime environment-variable or configuration bypass.

```text
1. Contract and identity
          |
2. Storage and controls
     /                     \
3. PM provenance       6. Reach/commands
     |
4. Resolution witnesses/index/events
     |
5. Hook and extension-use metrics
     \                     /
       7. Consent and activation
```

After Step 2, Steps 3 and 6 may proceed in parallel. Step 4 follows Step 3, Step 5 follows Step 4, and Step 7 waits for Steps 5 and 6.

Step 3 isolates the risky core PM type seam from telemetry instrumentation. Steps 4 and 5 remain end-to-end measurement slices. The plan remains seven PRs.

### Step 1: Telemetry contract and identity

Replace the dormant event types with the closed producer schema: common fields, bounded witnesses, fixed outcomes and histograms, extension-invocation counters and buckets, and per-kind version dispatch.

Add private telemetry-owned `<config-dir>/telemetry-state.toml`, atomic owner-only key creation where supported, scoped HMAC derivation, local 30-day windows, D0-D30 cohorts, and reset primitives. Keep this state outside the inspectable telemetry data directory. Do not add storage or emission.

Verify:

- All contract examples round-trip, and bounds and unknown versions behave as specified.
- Arbitrary or private data cannot serialize.
- Identities separate domains, dimensions, agents, and windows. The same active-window inputs remain stable across recorders and restarts, while normal rollover changes subjects without replacing the key.
- `disable` and `clear` preserve identity state; renewed consent and reset rotate it.
- Disabled paths create nothing.

- [ ] PR: telemetry contract and scoped identity

### Step 2: Storage and local controls

Add whole-batch event appends, non-waiting process locking, atomically replaced aggregate snapshots, daily caps and reservations, `storage_limit`, and lazy D31 cleanup. Add typed `status`, byte-preserving `show`, `clear`, and `reset-identifiers` commands.

Expose a test-only enabled recorder bound to a caller-supplied temporary telemetry home for integration tests and benchmarks. No production caller writes through the sink yet, and no runtime bypass is added.

Verify:

- Concurrent complete lines, old-or-new snapshots, whole-operation drops, and cap/marker accounting.
- D30/D31 cleanup, malformed and unknown inspection, and abandoned state/snapshot temporary cleanup.
- Private-state permissions and separation, clear/reset semantics, and test-only recorder isolation.
- Management commands never record themselves.

- [ ] PR: telemetry storage and local controls

### Step 3: PM provenance and public-identity policy

Extend every package-manager result with typed provenance and exact coordinates. Add core-owned public allowlists and coordinate validation. If an out-of-process package-manager protocol lands first, carry provenance through it as part of this step.

All supported package managers must provide provenance before events are wired. This step emits no telemetry.

Verify every package manager and source class, malformed and wildcard coordinates, public allowlist behavior, unnamed-reason precedence, and that raw URLs and private or local coordinates cannot become public identities.

- [ ] PR: package provenance and public identity policy

### Step 4: Resolution witnesses, attribution index, and recording

Return safe evidence from the original predicate evaluation while preserving short-circuit and accepted cache behavior. Whole-`PredicateSet` caching remains disallowed.

Carry evidence through plugin and skill selection, return one structured full-sync report, and construct coherent `resolution_summary`, `package_resolution`, and `extension_resolution` batches. After installation, atomically replace the generated agent-facing attribution index with successful managed skills and their marker fingerprints. Ordinary read-only plugin lookup remains silent.

Verify:

- `all`, `any`, `not`, and opaque/limit paths, plus cache hits without reevaluation.
- Every `extension_match` case and complete package-to-plugin-to-skill paths.
- Whole-batch failure, successful and failed installation indexing, and atomic old-or-new index reads.
- Stale, corrupt, fingerprint-mismatch, and collision handling.
- Raw paths, private names, and dependency snapshots stay out of telemetry.

- [ ] PR: resolution witnesses, installed attribution, and recording

### Step 5: Hook and extension-invocation telemetry

Measure completed Symposium and plugin-hook handling. Merge top-level observations into `hook_metrics` and plugin preparation/execution into `plugin_hook_metrics`, with fixed outcomes/histograms, scoped subjects, public/unnamed/overflow buckets, the 128-named-row limit, and all-or-nothing identified-session counts.

Add the sink-neutral extension-use observation and Claude `Skill` adapter. Existing pre/post-tool hooks and a targeted failure hook update `extension_invocation_metrics` through the installed attribution index, with independent phase counters, fixed unnamed reasons, a separate 128-public-skill limit, and all-or-nothing attempted/completed session counts. Never emit a per-invocation row.

Measure the hook path before and after. Verify:

- Sanitized automatic and manual activation, lifecycle fixtures, and unknown schemas.
- Public, private, missing, ambiguous, and stale attribution; marker validation; and raw-input exclusion.
- Counter, histogram, and cross-row invariants; independently dropped phases; boundary buckets; process merging; and rollover/reset.
- Bounded behavior across 500 lifecycles, private and overflow buckets, and missing, mixed, or over-256 session ids.
- Crash recovery, a worst-case snapshot within 512 KiB, and integer-overflow drops.
- A fake second sink, disabled-path short-circuiting, and unchanged agent output on recording failure.

- [ ] PR: bounded hook and extension-invocation telemetry

### Step 6: Session, configuration, and command telemetry

Add capability-aware `session_start`, daily `agent_configuration`, and eligible top-level `command` rows over the shared recorder.

The first non-telemetry recording-capable invocation of each UTC day attempts one all-agent configuration batch from the per-user agent list. It does not inspect agent-owned files. Hook internals, telemetry controls, arguments, and unsafe plugin command names remain excluded.

Verify:

- The capability matrix, Copilot without a session id, and Claude-only `Stop`.
- Configuration-list semantics, same-day configuration deduplication, and retry after a dropped configuration batch.
- Fixed command vocabulary and public eligibility, plus failures before or after command dispatch.

- [ ] PR: session reach and command telemetry

### Step 7: Consent, activation, and documentation

Add `consent-version`, one shared team-approved disclosure for `init` and `telemetry enable`, re-consent, explicit non-interactive acknowledgement, `enable` and `disable`, and final production wiring for Steps 4-6.

Review the final wording against the version 1 coverage requirements. The proposed prompt is a complete example, not the implementation string. There is no event-file migration because the dormant recorder was never called. Publish the proposed pages and update the current design and flow chapters plus `md/SUMMARY.md`.

Verify:

- New and existing configuration states, plus documented review of the final disclosure against every required coverage point.
- A snapshot proving that `init` and `telemetry enable` use identical approved text.
- Interactive and non-interactive flows, no partial or disabled collection, and no runtime consent bypass.
- Raw inspection and expiry, the full CLI and integration suite, and the hook-path benchmark.
- Formatting, clippy, workspace tests, mdBook, and orphan checks.

- [ ] PR: telemetry consent and recording activation
