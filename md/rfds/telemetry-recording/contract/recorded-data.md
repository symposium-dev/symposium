# What Symposium records

Symposium records telemetry only after you opt in. The records stay on your machine; nothing described on this page is uploaded. `cargo agents telemetry show` displays the stored bytes, and `cargo agents telemetry clear` deletes event and aggregate-metric files. The [never-record list](#what-is-never-recorded) summarizes the exclusions.

This page is the complete producer contract for consent version 1. If a field is not listed here, Symposium does not write it as telemetry.

## Common fields

Every JSONL row has:

| Field       | Example         | Meaning                                 |
| ----------- | --------------- | --------------------------------------- |
| `v`         | `1`             | Schema version for this row kind.       |
| `kind`      | `session_start` | Row kind from the list below.           |
| `event_id`  | `9f2c41b6-...`  | Identifier for this row; random when minted. |
| `day`       | `2026-08-03`    | UTC calendar day.                       |
| `symposium` | `0.4.0`         | Symposium version that wrote the event. |

Completed operational events (`session_start` and `command`) also have `at`, an RFC3339 UTC timestamp truncated to one second. Resolution, configuration, and aggregate metric rows have only `day`.

Counters and durations are non-negative JSON integers that fit an unsigned 64-bit value. Symposium checks arithmetic and drops an overflowing batch or observation instead of wrapping the value.

`event_id` exists to deduplicate a future retry of the same event or identify one cumulative metric row. A metric row keeps the `event_id` minted when its dimension first appears that UTC day as the row is rewritten. It is not an installation, session, account, or project identifier.

## Example JSONL for every row kind

These are independent row-shape examples, not one coherent operation or batch. The identifiers and coordinates are illustrative. An actual day may contain repeated low-volume events and one cumulative row per aggregate-metric dimension.

```jsonl
{"v":1,"kind":"session_start","event_id":"9f2c41b6-495e-4c88-a22b-c597f8102aed","day":"2026-08-03","at":"2026-08-03T09:14:02Z","symposium":"0.4.0","agent":"claude","os":"linux","arch":"x86_64","start":"fresh","session_id":"sess_31d8b1916028f65a0c0521dc1f4c86fb","retention_subject":"ret_74ddf26f80ad8b58de7f03e6c632e654","cohort_day":0}
{"v":1,"kind":"agent_configuration","event_id":"1a77f7c8-2c45-4de2-8cc6-b507ac3605f8","day":"2026-08-03","symposium":"0.4.0","agent":"claude","configured":true,"os":"linux","arch":"x86_64","agent_subject":"agt_9255770e1679cb789796a9f9e86325c5"}
{"v":1,"kind":"resolution_summary","event_id":"6c3ef7a1-f782-4b47-9a25-829dd64e1ba2","day":"2026-08-03","symposium":"0.4.0","trigger":"session_start","outcome":"ok","duration_ms":142,"public_packages":2,"unnamed_packages":1,"unnamed_package_reasons":{"private_registry":1,"git":0,"path":0,"workspace":0,"unknown_source":0,"invalid_coordinate":0},"plugins":1,"skills":1,"installed":2,"updated":0,"reaped":0,"session_id":"sess_31d8b1916028f65a0c0521dc1f4c86fb"}
{"v":1,"kind":"package_resolution","event_id":"c03e7390-52b1-4e11-b7e7-9573c84e555a","day":"2026-08-03","symposium":"0.4.0","package":{"ecosystem":"cargo","name":"example-runtime","version":"1.2.3"},"extension_match":"public","package_subject":"pkg_f6db813c87209816ae4896f3e60dd774"}
{"v":1,"kind":"extension_resolution","event_id":"fa21a1d7-64a7-4e3e-9f6a-4572cf8f1939","day":"2026-08-03","symposium":"0.4.0","target":{"type":"skill","source":"symposium-recommendations","name":"example-debugging"},"path":[{"type":"package","ecosystem":"cargo","name":"example-runtime","version":"1.2.3"},{"type":"extension","extension_type":"plugin","source":"symposium-recommendations","name":"example-tools"},{"type":"extension","extension_type":"skill","source":"symposium-recommendations","name":"example-debugging"}],"extension_subject":"ext_6e68a75b9701bbad86cf32cd876e994a"}
{"v":1,"kind":"hook_metrics","event_id":"b563dd02-0301-4e2c-aac4-2e0d5dfaa977","day":"2026-08-03","symposium":"0.4.0","agent":"claude","hook":"pre_tool_use","invocations":500,"outcomes":{"ok":498,"blocked":1,"plugin_error":1,"internal_error":0},"plugins_attempted":500,"plugins_completed":500,"duration_ms":{"bounds":[5,10,25,50,100,250,500,1000],"counts":[40,80,180,130,55,12,2,1,0]},"identified_sessions":1,"identified_sessions_non_ok":1,"session_counts_complete":true,"hook_subject":"hok_51f4f4143ce32704e96086899dc66a27"}
{"v":1,"kind":"plugin_hook_metrics","event_id":"70dcad2a-fd19-4b5d-97cb-a7c7b52a81f1","day":"2026-08-03","symposium":"0.4.0","agent":"claude","hook":"pre_tool_use","plugin_scope":"public","plugin":{"source":"symposium-recommendations","name":"example-tools"},"attempts":500,"executions":500,"outcomes":{"ok":499,"blocked":0,"error":1},"prepare_ms":{"bounds":[5,10,25,50,100,250,500,1000],"counts":[400,90,10,0,0,0,0,0,0]},"execute_ms":{"bounds":[5,10,25,50,100,250,500,1000],"counts":[10,50,200,180,50,8,2,0,0]},"identified_sessions":1,"identified_sessions_non_ok":1,"session_counts_complete":true,"plugin_subject":"plg_d42e3f1fbac5bb001e1258a1e70b0d77"}
{"v":1,"kind":"command","event_id":"5d18caa8-84f7-4aa3-846c-99ea810ccd85","day":"2026-08-03","at":"2026-08-03T10:02:11Z","symposium":"0.4.0","command":{"type":"builtin","name":"use"},"duration_ms":820,"outcome":"ok","command_subject":"cmd_adf0c14ddc35b97762b5daae6f4119ce"}
{"v":1,"kind":"storage_limit","event_id":"8430f7f3-7ec5-4ca5-9c65-8f6e83eaa3de","day":"2026-08-03","symposium":"0.4.0","dropped_operation":"manual_sync"}
{"v":1,"kind":"extension_invocation_metrics","event_id":"7b3bda33-1fd7-4657-9637-4057268370dc","day":"2026-08-03","symposium":"0.4.0","agent":"claude","target_scope":"public","target":{"type":"skill","source":"symposium-recommendations","name":"example-debugging"},"attempted":14,"completed":12,"failed":2,"session_counts_complete":true,"identified_sessions":4,"identified_sessions_completed":3,"extension_subject":"ext_6e68a75b9701bbad86cf32cd876e994a"}
```

## Scoped identifiers

### Key and rotation

When an enabled recorder first needs identity state, Symposium stores a random secret key in private `<config-dir>/telemetry-state.toml` (default `~/.symposium/telemetry-state.toml`). The same state holds the current identifier-window and return-cohort anchors. Every recorder reads it under the telemetry lock, so identical domain, window, and dimension inputs produce the same subject across processes and restarts.

Normal 30-day rollover changes the window input without replacing the key. Renewed consent or `telemetry reset-identifiers` replaces it. `telemetry disable` and `telemetry clear` preserve the key and anchors.

This file is separate from the inspectable `<config-dir>/telemetry/` data directory and has owner-only permissions where the platform supports them. The key is private state, not anonymized telemetry. It is not written into events, printed by telemetry commands, or derived from your machine. Someone who has the key can recompute candidate identifiers.

### What identifiers can link

Symposium derives each identifier for one narrow purpose:

| Identifier          | What it can link                                               | Rotation                          |
| ------------------- | -------------------------------------------------------------- | --------------------------------- |
| `session_id`        | Events carrying the same vendor session id for one agent       | 30 days; omitted when unavailable |
| `retention_subject` | Observed session days in one D0-D30 cohort                     | After cohort day 30               |
| `agent_subject`     | Repeated configuration observations for one agent              | 30 days                           |
| `package_subject`   | Repeated observations of one public package/version            | 30 days                           |
| `extension_subject` | Resolution and public invocation aggregates for one safe path   | 30 days                           |
| `hook_subject`      | Daily hook aggregates for one agent and hook surface           | 30 days                           |
| `plugin_subject`    | Daily hook aggregates for one eligible public plugin           | 30 days                           |
| `command_subject`   | Repeated use of one eligible command                           | 30 days                           |

These values are pseudonymous, not anonymous: they deliberately permit limited linking inside the stated scope. `retention_subject` can link observed sessions across agents for D0-D30; this is the single exception needed for return measurement. No identifier links that cohort, a package subject, and a command subject, and there is no workspace id.

All lines in your local telemetry directory still come from your Symposium home. File order and same-day events can therefore suggest which observations happened together.

## Version 1 public identity allowlists

Only the following stable labels can make package, plugin, skill, or plugin-command names eligible for recording under consent version 1:

| Enum | Version 1 values | Used by |
| --- | --- | --- |
| Package ecosystem | `cargo` | `package.ecosystem` and package path nodes. |
| Public extension source | `symposium-recommendations`, `crates-io` | Resolution/invocation `target.source`, extension path nodes, `plugin.source`, and plugin-command `source`. |

`crates-io` applies only when core proves allowlisted crates.io provenance; `symposium-recommendations` identifies the built-in registry. `user-plugins`, configured registries, paths, workspaces, and arbitrary git sources remain unnamed. Raw registry names and URLs are never enum values. Adding an ecosystem or public-source label expands name eligibility and requires a new consent version.

## Event kinds

### `session_start`

This row records a completed registered Symposium session-start hook.

| Field               | Values                                         | Meaning                                                 |
| ------------------- | ---------------------------------------------- | ------------------------------------------------------- |
| `at`                | UTC second                                     | When Symposium completed the session-start handling.    |
| `agent`             | `claude`, `codex`, `copilot`, `gemini`, `kiro` | Agent that invoked the registered hook.                 |
| `os`                | `linux`, `macos`, `windows`, `other`           | OS class for the running Symposium build.               |
| `arch`              | `x86_64`, `aarch64`, `other`                   | Architecture class for the running build.               |
| `start`             | `fresh`, `resumed`, `unknown`                  | Agent-supplied lifecycle classification when available. |
| `session_id`        | scoped id, optional                            | Omitted when the agent supplies no session id.          |
| `retention_subject` | scoped id                                      | Deduplicates this D0-D30 observed-session cohort.       |
| `cohort_day`        | integer `0` through `30`                       | UTC days since the cohort's first observed session.     |

GitHub Copilot does not currently supply a session id. OpenCode and Goose do not currently call Symposium through a registered session-start hook, so they do not produce this event.

These rows, not `hook_metrics` rows whose `hook` is `session_start`, are authoritative for observed-session and return measurements. For each `retention_subject`, the first row establishes D0. D1, D7, or D30 is present when at least one later session-start row has that `cohort_day`, regardless of agent or vendor session id. Multiple rows on the same cohort day count once.

The aggregate hook rows measure only session-start hook reliability and latency.

### `agent_configuration`

This row records whether a supported agent is configured for Symposium that day.

| Field           | Values                                                              | Meaning                                                  |
| --------------- | ------------------------------------------------------------------- | -------------------------------------------------------- |
| `agent`         | `claude`, `codex`, `copilot`, `gemini`, `kiro`, `opencode`, `goose` | Agent being checked.                                     |
| `configured`    | boolean                                                             | Whether the agent is listed in per-user Symposium configuration. |
| `os`            | `linux`, `macos`, `windows`, `other`                                | OS class for the running Symposium build.                |
| `arch`          | `x86_64`, `aarch64`, `other`                                        | Architecture class for the running build.                |
| `agent_subject` | scoped id                                                           | Deduplicates one installation for this agent and window. |

On the first non-telemetry recording-capable invocation each UTC day, Symposium attempts one whole batch containing all seven agents. `configured` means the agent is present in Symposium's per-user configuration at that moment; Symposium does not inspect agent-owned configuration files.

A successful batch is the day's snapshot, so later configuration changes appear on the next UTC day's snapshot. If locking, storage, or I/O drops the batch, it remains outstanding and a later eligible invocation retries it.

`configured: true` does not claim that an agent session ran or that the agent-owned integration files remain intact.

### `resolution_summary`

This row records the result of a full sync after session start, manual sync, `use`, or removal. Package, plugin, and skill counts are distinct coordinates within the sync, not duplicate declarations.

| Field              | Values                                          | Meaning                                                           |
| ------------------ | ----------------------------------------------- | ----------------------------------------------------------------- |
| `trigger`          | `session_start`, `manual_sync`, `use`, `remove` | Why full sync ran.                                                |
| `outcome`          | `ok`, `partial`, `error`                        | Closed result classification.                                     |
| `duration_ms`      | integer                                         | Full resolution/sync duration.                                    |
| `public_packages`  | integer                                         | Eligible public resolution-input packages.                        |
| `unnamed_packages` | integer                                         | Private, local, unknown, invalid, or otherwise ineligible inputs. |
| `unnamed_package_reasons` | fixed counters                              | Mutually exclusive reasons that sum to `unnamed_packages`.       |
| `plugins`          | integer                                         | Plugins in the final resolved set.                                |
| `skills`           | integer                                         | Skills in the final resolved set.                                 |
| `installed`        | integer                                         | Artifacts newly installed by this sync.                           |
| `updated`          | integer                                         | Existing artifacts changed by this sync.                          |
| `reaped`           | integer                                         | Obsolete artifacts removed by this sync.                          |
| `session_id`       | scoped id, optional                             | Present only for a sync inside an identified agent session.       |

`unnamed_package_reasons` has exactly `private_registry`, `git`, `path`, `workspace`, `unknown_source`, and `invalid_coordinate` counters. Each unnamed package increments exactly one. Source provenance takes precedence; `invalid_coordinate` is used only for an allowlisted public source with a malformed name or a missing, wildcard, or invalid exact version. The counters sum to `unnamed_packages`.

Read-only extension lookup on ordinary hook calls does not produce this event.

### `package_resolution`

This row records one eligible public package used as resolution input during a full sync.

| Field               | Values                  | Meaning                                                              |
| ------------------- | ----------------------- | -------------------------------------------------------------------- |
| `package.ecosystem` | `cargo`                  | Stable public ecosystem label.                                       |
| `package.name`      | validated string        | Public package name.                                                 |
| `package.version`   | validated exact version | Exact resolved public version, never a range or `*`.                 |
| `extension_match`   | `public`, `unnamed_only`, `none` | What kind of resolved extension, if any, the package contributed to. |
| `package_subject`   | scoped id               | Deduplicates this exact coordinate for 30 days.                      |

A package is named only when its package manager reports provenance matching a reviewed public-registry allowlist. Registry URLs themselves are not recorded.

`extension_match` describes what the package contributed:

- `public`: at least one eligible public extension matched, including when unnamed content also matched.
- `unnamed_only`: extension content matched, but none was eligible to name.
- `none`: no resolved extension matched.

Events measure packages independently. Symposium does not record a workspace/resolution id, but the summary and contiguous relationship batch can still make the public package set for one sync recoverable, especially when only one sync occurred that day. Review the whole file before sharing it.

### `extension_resolution`

This row records one public plugin or skill and one safe path that selected it.

| Field               | Values                      | Meaning                                             |
| ------------------- | --------------------------- | --------------------------------------------------- |
| `target.type`       | `plugin`, `skill`           | Resolved extension type.                            |
| `target.source`     | `symposium-recommendations`, `crates-io` | Stable label, never a configured URL or local name. |
| `target.name`       | validated string            | Name defined by eligible public content.            |
| `path`              | bounded typed nodes         | Actual safe package/predicate/extension chain.      |
| `extension_subject` | scoped id                   | Deduplicates this safe target/path for 30 days.     |

Path nodes are limited to:

| Node `type` | Recorded content                                                     |
| ----------- | -------------------------------------------------------------------- |
| `package`   | Eligible public ecosystem, name, and exact version.                  |
| `extension` | Eligible public plugin/skill source and name.                        |
| `all`       | All safe children that contributed to success.                       |
| `any`       | The branch that actually made the expression succeed.                |
| `not`       | Marker only; the child is not recorded.                              |
| `opaque`    | Fixed reason: `private_source`, `non_package_predicate`, or `limit`. |

Shell commands, paths, environment variables, custom predicate names or arguments, and private package or extension names never enter a path. An opaque marker can represent their position.

Witness depth counts nested evidence nodes from the root, which is level 1, to a terminal package, extension, `not`, or opaque node. A subtree that would exceed level 8 becomes `opaque: limit`. The complete path is also limited to 16 evidence leaves and 4 KiB. Evidence depth does not count filesystem components; filesystem paths are never recorded.

This event says an extension resolved. It does not say that an agent read or used the extension; a matching `extension_invocation_metrics` aggregate separately reports observed agent activation. Version 1 can produce that aggregate only for Claude.

### `hook_metrics`

This cumulative row combines completed hook observations for one UTC day, agent, hook surface, and active identifier epoch. An epoch normally lasts 30 days but ends early after identifier reset or renewed consent. A reset can therefore leave two rows for the same agent, hook, and day with different `hook_subject` values. Symposium updates the current row instead of appending one telemetry row per invocation.

| Field                             | Values                                                                         | Meaning                                                                                     |
| --------------------------------- | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| `agent`                           | `claude`, `codex`, `copilot`, `gemini`, `kiro`                                | Invoking agent.                                                                             |
| `hook`                            | `pre_tool_use`, `post_tool_use`, `user_prompt_submit`, `session_start`, `stop` | Symposium hook surface.                                                                     |
| `invocations`                     | integer                                                                        | Completed hook observations merged into the row.                                            |
| `outcomes`                        | hook outcome counters                                                          | Exact counters named `ok`, `blocked`, `plugin_error`, and `internal_error`.                  |
| `plugins_attempted`               | integer                                                                        | Plugin hooks whose preparation began.                                                       |
| `plugins_completed`               | integer                                                                        | Plugin hooks with an observed terminal result.                                              |
| `duration_ms`                     | latency histogram                                                              | Parsed-input-to-response-ready latency; telemetry update time is excluded.                   |
| `session_counts_complete`         | boolean                                                                        | Whether the two distinct identified-session counts are complete for every observation.      |
| `identified_sessions`             | integer, optional                                                              | Distinct identified sessions represented; present only when `session_counts_complete=true`. |
| `identified_sessions_non_ok`      | integer, optional                                                              | Those sessions with at least one non-`ok` observation; present only when counts are complete. |
| `hook_subject`                    | scoped id                                                                      | Links this agent/hook dimension inside its 30-day identifier window.                         |

`outcomes` and the duration histogram each sum to `invocations`. Exactly one outcome counter advances for each completed observation. Precedence is `internal_error`, then `blocked`, then `plugin_error`, then `ok`.

Session counts remain complete only when every contributing observation supplies a session id and neither set exceeds 256 distinct ids for the row. On the first missing id or overflow, Symposium discards both sets, writes `session_counts_complete: false`, and omits both counts for the rest of that day. Raw and keyed session ids are never written into the aggregate file.

### `plugin_hook_metrics`

This cumulative row combines plugin-hook observations for one UTC day, agent, hook surface, active identifier epoch, and bounded plugin bucket. A reset can leave otherwise identical `unnamed` or `overflow` rows distinguished only by `event_id`; public rows also receive a new `plugin_subject`.

| Field                         | Values                                                                         | Meaning                                                                                     |
| ----------------------------- | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| `agent`                       | `claude`, `codex`, `copilot`, `gemini`, `kiro`                                | Invoking agent.                                                                             |
| `hook`                        | `pre_tool_use`, `post_tool_use`, `user_prompt_submit`, `session_start`, `stop` | Symposium hook surface.                                                                     |
| `plugin_scope`                | `public`, `unnamed`, `overflow`                                                | Whether the bucket names an eligible public plugin.                                         |
| `plugin.source`               | `symposium-recommendations`, `crates-io`, conditional                         | Present only when `plugin_scope=public`.                                                     |
| `plugin.name`                 | validated string, conditional                                                  | Present only when `plugin_scope=public`.                                                     |
| `attempts`                    | integer                                                                        | Plugin-hook attempts that reached an observed terminal result.                              |
| `executions`                  | integer                                                                        | Those completed attempts that reached child execution.                                      |
| `outcomes`                    | plugin outcome counters                                                        | Exact counters named `ok`, `blocked`, and `error`.                                           |
| `prepare_ms`                  | latency histogram                                                              | Preparation time for every attempt.                                                         |
| `execute_ms`                  | latency histogram                                                              | Child execution time for attempts counted by `executions`.                                  |
| `session_counts_complete`     | boolean                                                                        | Whether the two distinct identified-session counts are complete for every attempt.          |
| `identified_sessions`         | integer, optional                                                              | Distinct identified sessions represented; present only when `session_counts_complete=true`. |
| `identified_sessions_non_ok`  | integer, optional                                                              | Those sessions with at least one non-`ok` attempt; present only when counts are complete.    |
| `plugin_subject`              | scoped id, conditional                                                         | Present only with an eligible public plugin.                                                |

#### Counting rules

`attempts` counts terminal results. `executions` counts the subset that started child execution. A preparation failure is terminal `error` with no execution. `outcomes` and `prepare_ms` each sum to `attempts`; `execute_ms` sums to `executions`. Across every epoch and bucket for one agent, hook, and day, plugin `attempts` sum to the corresponding top-level `plugins_completed`.

`blocked` means the plugin requested a block. `error` covers a closed preparation or execution failure. `ok` is every other completed attempt. The same 256-id all-or-nothing session rule used by `hook_metrics` applies.

#### Plugin identity and row limits

Eligible public plugins use `{source, name}` coordinates from the reviewed allowlist. Private, local, invalid, or otherwise ineligible plugins merge into one `unnamed` row per agent, hook, epoch, and day and expose no identity.

At most 128 named public-plugin rows may appear across all agents, hooks, and identifier epochs in one UTC day. Attempts for later rows merge into `overflow` rows per agent, hook, and epoch and expose no identity. Identifier reset does not reset this daily limit.

Named rows are first-observed, not sampled, so earlier-in-day public plugins are overrepresented when the limit is reached. Analysis must report overflow and must not treat the named subset as random.

### Rules shared by hook aggregates

A latency histogram is exactly:

```json
{"bounds":[5,10,25,50,100,250,500,1000],"counts":[0,0,0,0,0,0,0,0,0]}
```

The nine non-overlapping millisecond buckets mean `<=5`, `(5,10]`, `(10,25]`, `(25,50]`, `(50,100]`, `(100,250]`, `(250,500]`, `(500,1000]`, and `>1000`. Bounds and count-array length cannot vary. Fixed histograms can be merged correctly across installations; locally calculated percentiles cannot.

Neither aggregate kind has `at`, an invocation id, a raw or scoped session id, or an individual invocation's time or outcome. No hook input, output, stdout, stderr, error text, or numeric exit detail is recorded. Symposium adds no timeout as part of telemetry.

A host termination, lock conflict, storage limit, or I/O failure can omit an observation, so aggregate counts are lower bounds. No durable dropped-update counter can cover every loss mode because lock contention, termination, and I/O failure can also prevent writing that counter. Counting only cap rejections would imply false completeness.

These rows do reveal exact daily counts for each hook surface. In particular, `pre_tool_use`, `post_tool_use`, and `user_prompt_submit` counts are proxies for daily tool and prompt activity even though no individual activity record exists.

### `extension_invocation_metrics`

This cumulative row combines skill-invocation observations for one UTC day, supported agent, active identifier epoch, and bounded skill bucket. Version 1 supports only Claude Code and accepts only its fixture-tested `Skill` signal. Symposium does not append one row per invocation.

| Field                           | Values                                                                                              | Meaning                                                                                                 |
| ------------------------------- | --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `agent`                         | `claude`                                                                                            | The only version 1 agent with a supported structured skill-invocation signal.                           |
| `target_scope`                  | `public`, `unnamed`, `overflow`                                                                    | Whether the bucket names an eligible public skill.                                                      |
| `target.type`                   | `skill`, conditional                                                                                 | Present only when `target_scope=public`.                                                               |
| `target.source`                 | `symposium-recommendations`, `crates-io`, conditional                                              | Reviewed public source; present only when `target_scope=public`.                                       |
| `target.name`                   | validated string, conditional                                                                        | Public skill name; present only when `target_scope=public`.                                            |
| `unnamed_reason`                | `ineligible`, `not_indexed`, `attribution_unavailable`, `ambiguous`, `invalid_signal`, conditional    | Present only when `target_scope=unnamed`.                                                              |
| `attempted`                     | integer                                                                                              | Valid Claude `PreToolUse:Skill` observations merged into the row.                                      |
| `completed`                     | integer                                                                                              | Successful Claude `PostToolUse:Skill` observations merged into the row.                                |
| `failed`                        | integer                                                                                              | Terminal Claude `PostToolUseFailure:Skill` observations merged into the row.                           |
| `session_counts_complete`       | boolean                                                                                              | Whether both distinct identified-session counts are complete for all contributing observations.        |
| `identified_sessions`           | integer, optional                                                                                    | Distinct sessions with an attempted observation; present only when session counts are complete.         |
| `identified_sessions_completed` | integer, optional                                                                                    | Distinct sessions with a completed observation; present only when session counts are complete.          |
| `extension_subject`             | scoped id, conditional                                                                               | Present only for a public bucket; matches the selected safe resolution path for 30 days.                 |

#### Count semantics

`attempted`, `completed`, and `failed` are independent lower bounds. Each hook phase updates the snapshot separately. If one update is lost, completed plus failed need not equal attempted and may exceed it. The two session counts have the same independence.

`failed` advances only when the supported agent emits its targeted failure signal. For version 1, that is Claude's `PostToolUseFailure:Skill` event. Symposium does not infer failure from denial, termination, or a missing terminal observation. Completion means that the agent activated the skill; it does not show whether the agent followed its instructions or improved the task result.

#### Attribution

Public attribution comes only from the generated installation index and a matching Symposium marker fingerprint. Unnamed reasons have these meanings:

- `ineligible`: the installed skill was private, local, invalid, or otherwise unsafe to name.
- `not_indexed`: the index was valid and readable but contained no matching agent-facing identifier.
- `attribution_unavailable`: the index was missing, corrupt, or stale.
- `ambiguous`: more than one entry matched.
- `invalid_signal`: the Claude payload did not match the validated schema.

No reason exposes the raw identifier.

#### Row and session limits

At most 128 public-skill rows may be named across identifier epochs in one UTC day. Later public skills merge into one `overflow` row per agent and epoch. Each fixed unnamed reason produces at most one row per agent and epoch. The public subset is first-observed, not sampled; analysis must report overflow and include unnamed counts when interpreting the public denominator.

The same 256-id all-or-nothing rule applies to the attempted and completed session sets. On the first missing id, overflow, or state/snapshot mismatch, Symposium discards both sets, writes `session_counts_complete: false`, and omits both counts for the rest of that row and day.

This aggregate has no `at`, duration, raw or scoped session id, invocation id, tool name, tool input/output, prompt, transcript, or individual outcome row. Unsupported agents emit no row, which means unknown rather than zero.

### `command`

This row records one completed eligible top-level user command.

| Field             | Values           | Meaning                                                       |
| ----------------- | ---------------- | ------------------------------------------------------------- |
| `at`              | UTC second       | Completion time.                                              |
| `command`         | typed coordinate | Fixed built-in, or eligible public plugin command coordinate. |
| `duration_ms`     | integer          | Top-level command duration.                                   |
| `outcome`         | `ok`, `error`    | Closed result.                                                |
| `command_subject` | scoped id        | Deduplicates this command for 30 days.                        |

Arguments are never recorded. Internal `hook`, all `telemetry` commands, and ineligible external/plugin commands do not produce command events.

Built-in names are `init`, `sync`, `search`, `use`, `status`, `plugin_sync`, `plugin_list`, `plugin_show`, `plugin_validate`, `self_update`, and `crate_info`:

```json
{"type":"builtin","name":"use"}
```

An eligible plugin command contains only its reviewed public-source label, public plugin name, and declared command name:

```json
{"type":"plugin","source":"symposium-recommendations","plugin":"example-tools","name":"example-check"}
```

### `storage_limit`

This row means that the next complete low-volume event batch did not fit in the shared daily 8 MiB allowance. In addition to the common fields, `dropped_operation` is `session_start`, `manual_sync`, `use`, `remove`, `init`, `configuration`, or `command`. It identifies the top-level operation whose batch was rejected.

The row appears at most once per UTC day, and the marker itself counts toward 8 MiB. An aggregate-metric update that would exceed its separate 512 KiB maximum or the remaining shared allowance is dropped without this marker and does not stop low-volume recording. The marker does not report lock-contention, aggregate-update, or crash losses.

## What is never recorded

- Individual prompt/tool activity records, per-invocation hook records, or individual skill-invocation records.
- Prompt text or any substring of it.
- Tool names, arguments, responses, or results.
- Shell commands, file contents, patches, or URLs from user/agent data.
- Error messages, debug strings, hook stdout/stderr, numeric exit details, or raw agent/package-manager payloads.
- Raw agent-facing or private skill identifiers and ephemeral invocation identifiers.
- File paths, workspace roots, project names, repository ids, or dependency-set snapshots.
- Environment names/values, hostname, username, home directory, IP address, model name, or account/vendor identifiers.
- Names or versions from private registries, git/path/workspace sources, or sources whose public identity is not allowlisted.
- Raw configured registry names/URLs or arbitrary git URLs.
- Raw vendor session ids, a global installation id, a workspace id, or one identifier shared across measurement purposes.
- Wall-clock timestamps more precise than one second.

## Storage and expiry

### Data files and recording

Low-volume events are appended as JSON lines in `events-YYYY-MM-DD.jsonl` under the inspectable `<config-dir>/telemetry/` data directory (default `~/.symposium/telemetry/`). Current cumulative hook, plugin-hook, and extension-invocation aggregates are JSON lines in `metrics-YYYY-MM-DD.jsonl`. Symposium rewrites this bounded snapshot atomically after a merge.

The lock in the telemetry directory also guards sibling private state. A recorder makes one non-waiting lock attempt. It may drop a complete buffered event batch or aggregate observation rather than delay your hook or command. Recording failures never change the user operation's result.

### Daily limits and retention

The event file, aggregate-metric snapshot, and reserved maximum-size `storage_limit` line share an 8 MiB daily allowance. This allowance is a safety ceiling, not expected volume or preallocation. It bounds damage from a producer bug or unexpectedly large resolution batch.

Aggregate metrics may use at most 512 KiB, so high-volume hook and skill activity cannot consume the allowance reserved for resolution and configuration events.

A file is eligible for deletion only when `current_utc_day - file_utc_day > 30`. A D0 file remains throughout D30 and is first eligible on D31. Together with D31 expiry, the daily limit bounds ordinary retained telemetry near 248 MiB, excluding temporary files and private state. Cleanup is lazy, so an old file remains until a recording-capable invocation or telemetry command runs.

### Private state

The sibling private `<config-dir>/telemetry-state.toml` holds the identity key, current identifier-window and return-cohort anchors, cleanup and marker metadata, bounded keyed session sets, and snapshot contribution counts used to calculate complete distinct-session counts.

Symposium creates and replaces it atomically with owner-only permissions where supported. Replacement uses a same-directory temporary file beside `config.toml`; abandoned state temporaries are ignored and cleaned lazily under the telemetry lock.

The session sets are not printed or copied into metric rows. Symposium discards them at UTC-day rollover and removes them when `telemetry clear` or `telemetry reset-identifiers` runs. State is replaced before the corresponding metric snapshot. If a later snapshot write fails, a contribution-count mismatch on the next update discards the sets and permanently marks the row's session counts incomplete for that day.

`telemetry clear` deletes event and aggregate-metric files and rewrites private state to remove pending sets while preserving the identity key and current anchors. `telemetry reset-identifiers` rotates future identifiers and starts a new retention cohort. `telemetry disable` stops recording; existing files remain unless the user accepts its interactive clear offer or runs `telemetry clear` later.

### Installation index

The generated `<agent-skills-parent>/.symposium/index-v1.json` file is installation state, not telemetry. It may contain local agent-facing identifiers, installed-directory fingerprints, eligibility, and safe public attribution needed to identify the skill Symposium installed. It is gitignored, atomically replaced after sync, excluded from `telemetry show` and `telemetry clear`, and not uploaded by this RFD.

### Limits of interpretation

Public names, versions, and exact counts can be identifying when they are unusual. Exact daily hook counts disclose approximate prompt/tool activity by surface. Extension-invocation rows disclose exact skill-attempt, completion, and failure counts.

Lock contention, process termination, full files, and I/O failures can make the log incomplete. The records are therefore pseudonymous, locally associated, and best-effort. They must not be described as anonymous or as proof that an unrecorded action did not happen.
