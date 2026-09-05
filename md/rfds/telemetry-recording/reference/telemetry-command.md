# `cargo agents telemetry`

Use this command to manage per-user local telemetry: check whether it is enabled, opt in, inspect what is stored, stop recording, delete recorded data, or rotate future identifiers. See [What Symposium records](../contract/recorded-data.md) for the exhaustive field list and [Telemetry configuration](./configuration.md) for consent semantics.

Telemetry is off by default. Nothing is uploaded.

## Usage

```text
cargo agents telemetry [status]
cargo agents telemetry enable [--acknowledge]
cargo agents telemetry disable
cargo agents telemetry show [--count N]
cargo agents telemetry clear
cargo agents telemetry reset-identifiers
```

Start with `status`. If you choose to participate, run `enable` and review the disclosure. Use `show` whenever you want to inspect the stored records. `disable` stops future recording, while `clear` deletes recorded data.

Telemetry management commands are never recorded as command telemetry.

## `status`

Use `status` to see the effective consent state and a summary of the local event and aggregate-metric files:

```console
$ cargo agents telemetry status
Telemetry: enabled (consent version 1)
  data directory:    ~/.symposium/telemetry/
  stored:            3 file(s), 28.4 KiB
  physical lines:    71
  supported rows:    68
  unknown schemas:   2
  malformed lines:   1
  range:             2026-08-01 through 2026-08-03
```

Possible states are `disabled`, `consent required`, and `enabled`. `Consent required` means the config contains an earlier or unversioned opt-in; Symposium records nothing until you accept the current disclosure.

Stored files/bytes and physical lines cover daily event and aggregate-metric files; they exclude `.lock`, temporary files, and sibling private `telemetry-state.toml`. Supported rows are lines the current binary recognizes by kind and schema version. Unknown and malformed lines stay on disk and remain visible through `show`. `status` never prints the secret identity key or pending keyed session sets.

## `enable`

`enable` presents the current disclosure and asks whether to begin local recording. It does not enable telemetry unless the user explicitly accepts.

### Disclosure requirements

Before asking for consent, the team-approved version 1 disclosure must make these points clear:

- Telemetry is off by default, requires an explicit opt-in, and defaults to no in both interactive entry points.
- The recorded categories are:
  - observed sessions and configured agents, including Symposium version, agent labels, and platform classes;
  - public package names and exact versions, public resolution relationships, aggregate sync results, and reasons packages remain unnamed;
  - exact daily hook, plugin-hook, and agent skill-activation metrics, with structured skill activation available only for Claude in version 1; and
  - completed eligible commands without arguments, plus storage-limit markers.
- Exact hook-surface counts can approximate daily prompt/tool activity. The disclosure distinguishes second-precision session/command timestamps from day-only aggregate and resolution rows.
- Purpose-scoped pseudonymous identifiers permit the stated links for up to 30 days, including one D0-D30 observed-session cohort across agents; they are not anonymous or a global project/workspace identity.
- The main exclusions are stated plainly, including prompt/tool content, paths, project/workspace identity, environment and machine identity, raw errors and agent/package-manager payloads, private package/plugin/skill names, and individual hook or skill-invocation rows.
- Recorded data and private identifier/counting state remain local in their stated locations, nothing is uploaded, data remains through D30 and is eligible for deletion on D31, and the disclosure names the inspection and deletion commands.

The [exhaustive field list](../contract/recorded-data.md) and [never-record list](../contract/recorded-data.md#what-is-never-recorded) define the details behind these requirements. The team may revise structure, tone, and wording before implementation, provided the final disclosure preserves every required point and its meaning.

`cargo agents init` and `cargo agents telemetry enable` must use the same approved text and both default to no.

### Example version 1 disclosure

The following non-normative example demonstrates complete coverage. It is a review aid and starting point for the final product copy, not the string the implementation must reproduce.

```text
Symposium telemetry is off by default. If enabled, Symposium records:
- observed session starts and configured agents, with fresh/resumed status when
  available, Symposium version, operating-system class, and architecture;
- public package names and exact versions, public plugin/skill resolution paths,
  aggregate sync results, reasons packages remain unnamed, and whether only
  unnamed extensions matched;
- exact daily Claude skill-activation attempts, completions, and failures,
  public skill names, distinct-session counts when complete, fixed unnamed
  reasons, and overflow counts;
- exact daily hook and plugin-hook counts, outcomes, and latency histograms;
- completed built-in and eligible public plugin commands, with outcome and
  duration but without arguments;
- storage-limit markers naming only the affected operation; and
- purpose-scoped pseudonymous identifiers that link repeated observations for
  up to 30 days, plus one D0-D30 observed-session cohort across agents.

Counts for pre-tool-use, post-tool-use, and user-prompt-submit approximate daily
tool and prompt activity. Session starts and commands include UTC timestamps
truncated to one second; other rows include only the UTC day.
Pseudonymous does not mean anonymous: the scoped identifiers permit only the
links described above, and no identifier links all telemetry or identifies a
project or workspace.

Symposium never records prompt or tool content, tool names or arguments, file
paths, project or workspace identity, environment values, hostname, username,
model/account/vendor identifiers, raw errors or agent/package-manager payloads,
private package, plugin, or skill names, raw agent-facing skill identifiers,
individual hook-invocation rows, or individual skill-invocation rows.

Recorded data stays on this machine in ~/.symposium/telemetry/. A private
identifier key and bounded counting state stay in
~/.symposium/telemetry-state.toml; telemetry commands never print them.
Nothing is uploaded.
Files remain through day 30 and become eligible for deletion on day 31. Inspect
them with cargo agents telemetry show and delete them with
cargo agents telemetry clear.

Enable telemetry under consent version 1? [y/N]
```

In a non-interactive environment, `enable` does not change config unless `--acknowledge` is supplied explicitly. This prevents scripts or a manually retained unversioned boolean from upgrading consent silently.

Enabling telemetry does not rewrite or assign new identifiers to old stored lines. Accepting a new consent version rotates the secret identity key and starts a new retention cohort, severing old and new scoped identifiers.

## `disable`

`disable` stops future recording by setting `enabled = false`. Existing event and aggregate-metric files remain on disk. In an interactive terminal, `disable` offers to clear them and defaults to keeping them. In a non-interactive environment, it prints the `clear` command instead of deleting data.

## `show`

`show` prints stored event and current aggregate-snapshot JSONL lines. The ordering is deterministic:

1. UTC days appear in ascending order.
2. Within a day, append-only event lines appear first in physical order.
3. Aggregate rows follow in kind, agent, hook or target scope, public source/name or unnamed reason, and event-id order.

The event id breaks ties when an identifier reset creates two aggregate epochs in one day. `--count N` returns the last `N` lines in this ordering:

```console
$ cargo agents telemetry show --count 2
{"v":1,"kind":"command","event_id":"5d18caa8-84f7-4aa3-846c-99ea810ccd85","day":"2026-08-03","at":"2026-08-03T10:02:11Z","symposium":"0.4.0","command":{"type":"builtin","name":"use"},"duration_ms":820,"outcome":"ok","command_subject":"cmd_adf0c14ddc35b97762b5daae6f4119ce"}
{"v":1,"kind":"hook_metrics","event_id":"b563dd02-0301-4e2c-aac4-2e0d5dfaa977","day":"2026-08-03","symposium":"0.4.0","agent":"claude","hook":"pre_tool_use","invocations":500,"outcomes":{"ok":496,"blocked":1,"plugin_error":3,"internal_error":0},"plugins_attempted":500,"plugins_completed":500,"duration_ms":{"bounds":[5,10,25,50,100,250,500,1000],"counts":[8,17,76,144,181,68,6,0,0]},"session_counts_complete":true,"identified_sessions":4,"identified_sessions_non_ok":2,"hook_subject":"hok_b5b707841de7695912bec9b8bca382e8"}
```

The command copies stored line bytes; it does not parse, normalize, repair, or pretty-print them. Unknown-version and malformed lines are shown as stored. The current day's aggregate file is a cumulative snapshot, not a history of its replaced versions.

Aggregate rows have no `at`, so storage order is not chronology. All output comes from the same Symposium home, and line order or day can expose co-occurrence even though the rows have no global installation or workspace id. Review the complete output before sharing it.

Redirect the output to create a local copy:

```bash
cargo agents telemetry show --count 100000 > telemetry.jsonl
```

No separate export command is part of this RFD.

## `clear`

`clear` deletes every `events-YYYY-MM-DD.jsonl` and `metrics-YYYY-MM-DD.jsonl` file. It acquires the telemetry lock and also discards pending aggregate session-count sets:

```console
$ cargo agents telemetry clear
Deleted 12 telemetry data file(s) from ~/.symposium/telemetry/.
```

`clear` preserves the telemetry directory, lock, identity/cohort state, identity key, and consent setting. New rows in the same identifier window can therefore carry the same scoped subjects as cleared rows. Severing that future linkage also requires `reset-identifiers`.

## `reset-identifiers`

`reset-identifiers` severs identifier linkage between future and existing rows. It acquires the telemetry lock, replaces the secret identity key, discards pending aggregate session-count sets, and starts a new retention cohort:

```console
$ cargo agents telemetry reset-identifiers
Telemetry identifiers reset. Existing telemetry data files were not changed.
```

The command neither deletes nor rewrites old event or aggregate-metric rows. Identifiers before and after the reset cannot be derived into each other from the data files.

If no identity state exists, the command reports that there is nothing to reset instead of creating a key. If existing state is unreadable or malformed, recording remains stopped until this command explicitly replaces it.

## Files and private state

```text
~/.symposium/
|-- telemetry-state.toml
`-- telemetry/
    |-- .lock
    |-- metrics-2026-08-03.jsonl
    `-- events-2026-08-03.jsonl
```

Each project skills parent may also contain a generated `.symposium/index-v1.json` installation index. It maps agent-facing skill identifiers to Symposium-managed installations so a later hook can attribute a skill activation. The index is gitignored installation state, not telemetry: `show`, `clear`, retention, and identifier reset do not read or delete it, and this RFD does not upload it.

`telemetry-state.toml` is private Symposium state outside the inspectable telemetry data directory. It contains the secret identity key, current identifier-window and return-cohort anchors, cleanup and marker metadata, and bounded keyed session sets plus contribution counts used for complete aggregate session counts. All recorders read this state under the telemetry lock.

Normal 30-day rollover changes the window anchor without replacing the key. Renewed consent or `reset-identifiers` replaces the key; `disable` and `clear` preserve it.

Symposium atomically creates and replaces the file with owner-only permissions where supported. Replacement uses a same-directory temporary file beside `config.toml`; abandoned state temporaries are ignored and cleaned lazily under the telemetry lock.

`show`, `status`, data retention, and `clear` do not expose or delete the state file. `clear` rewrites it only to remove pending sets, preserving the key and current anchors.

Session sets and contribution counts are never copied into metric rows. Symposium discards them at day rollover or when `clear` or `reset-identifiers` runs. `show` and `status` do not lock writers, so a summary spanning several files is not an atomic snapshot.

## Concurrent recording

Recorders make one non-waiting attempt on the lock in the telemetry data directory. The lock guards data and private state mutations. On contention, the recorder drops the entire event batch or aggregate observation rather than delaying the agent or command.

Event batches are appended. Hook, plugin-hook, and extension-invocation observations are merged into a bounded, canonically ordered snapshot using a same-directory temporary write and atomic replace. A crash leaves either the old or new complete snapshot; abandoned temporary files are ignored and cleaned lazily.

Session-count state is atomically replaced first and carries the snapshot contribution count. After a failed snapshot write, a mismatch discards the sets and makes the row's session counts incomplete for that day.

Management commands can wait for the lock. A crash can still lose the last batch or metric update, or leave a partial final event line; `status` reports that line as malformed and `show` preserves it.

Hook and extension-invocation counts are lower bounds. There is no durable all-cause dropped-update counter because contention, termination, and I/O failure can also prevent writing that counter.

## Size and expiry

Each UTC day's event file, aggregate-metric snapshot, and reserved maximum-size `storage_limit` line share an 8 MiB allowance. This is a safety ceiling, not expected volume or preallocation. It bounds damage from a producer bug or unexpectedly large resolution batch.

Aggregate metrics may use at most 512 KiB. An update that would exceed that maximum or the remaining shared allowance is dropped without stopping low-volume event recording.

Telemetry files remain through D30 and become eligible for deletion on D31, when `current_utc_day - file_utc_day > 30`. Together with D31 expiry, the daily allowance bounds ordinary retained telemetry near 248 MiB, excluding temporary files and private state.

Cleanup runs lazily, at most once per day, when a recording-capable or telemetry command next runs. Uninstalling Symposium does not delete these files.
