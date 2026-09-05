# Telemetry configuration

Telemetry consent belongs to the current user. Symposium reads it from the user configuration file, normally `~/.symposium/config.toml`. Project configuration cannot enable, disable, or grant consent for telemetry.

## Consent states

```toml
[telemetry]
enabled = true
consent-version = 1
```

| Key               | Type             | Default | Meaning                                             |
| ----------------- | ---------------- | ------- | --------------------------------------------------- |
| `enabled`         | boolean          | `false` | The user's stored choice to permit local recording. |
| `consent-version` | positive integer | absent  | Disclosure version the user acknowledged.           |

Symposium records telemetry only when `enabled` is true and `consent-version` matches the disclosure used by the current binary. For consent version 1:

| Configuration                                | Effective state                  |
| -------------------------------------------- | -------------------------------- |
| absent section or `enabled = false`          | Disabled                         |
| `enabled = true` with absent/non-`1` version | Consent required; record nothing |
| `enabled = true` and `consent-version = 1`   | Enabled                          |

The binary chooses the current consent version. The configuration value records which disclosure the user accepted; it cannot select an arbitrary version. The [version 1 disclosure requirements](./telemetry-command.md#disclosure-requirements) define what that disclosure covers.

The complete prompt on that page is an example, not fixed implementation text. Interactive `init` and `telemetry enable` present the same final text approved by the team.

## When consent must be renewed

A future release may require a higher consent version after changing the recorded categories, linkability, timestamp precision, public-name eligibility, retention, or a normative exclusion. Until the user accepts that version, the effective state is `Consent required` and Symposium records nothing.

Adding an agent enum value creates a new schema version for each affected event kind so typed readers do not reinterpret old schemas. It does not by itself require renewed consent when the recorded fields, categories, timestamp precision, and correlation boundaries remain unchanged. New fields, hook surfaces, or linkage for that agent do require a higher consent version.

Accepting a newer consent version rotates the telemetry identity key and starts a new D0-D30 return cohort. Existing event and aggregate-metric files are neither rewritten nor deleted, but their scoped identifiers cannot link to later rows.

## Changing telemetry state

`cargo agents telemetry enable` presents the team-approved disclosure for the current version before writing these values. Interactive `cargo agents init` presents the same text for new users and existing unversioned opt-ins, defaulting to no. Non-interactive `init` does not grant or upgrade consent.

`cargo agents telemetry disable` sets `enabled = false` without deleting existing telemetry data files.

## Related private and installation state

Consent configuration is separate from the random identity key and current identifier-window and cohort anchors in private `<config-dir>/telemetry-state.toml` (default `~/.symposium/telemetry-state.toml`). This state sits outside the inspectable `<config-dir>/telemetry/` data directory and uses owner-only permissions where supported.

It persists across `disable` and `clear`, keeping identifiers consistent inside the active window. It is not configuration, and Symposium never reads it from project configuration.

Symposium-managed project skills may also have a generated `<agent-skills-parent>/.symposium/index-v1.json` installation index. It associates the agent-facing skill identifier with the resolved installation that sync produced. This file is gitignored installation state, not project configuration or telemetry; changing it cannot grant consent, and telemetry commands do not inspect or delete it.
