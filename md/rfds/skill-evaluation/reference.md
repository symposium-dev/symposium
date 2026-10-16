# Reference: evaluating agent tasks

This page holds the precise rules behind [the skill evaluation RFD](./README.md). Read the RFD's walkthroughs first; this page assumes them.

## Task format

```text
evals/
  plugins/<name>/        # a Symposium plugin, offered through the trial's registry
  fixtures/<name>/       # a crate a task depends on, copied in read-only
  tasks/<name>/
    task.toml
    starter/             # project copied into each trial
    checks/              # hidden tests, one file per test
    solution/            # optional; files copied over the starter
```

`task.toml` fields:

| Field | Default | Meaning |
|---|---|---|
| `kind` | required | `regression` or `capability`. |
| `plugins` | `[]` | Plugins from `evals/plugins/` placed in the trial's registry. |
| `fixtures` | `[]` | Crates from `evals/fixtures/` copied to `<run>/fixtures/`, read-only. |
| `editable` | `["src/**"]` | The only paths taken from the submission for grading. |
| `fmt_check` | `false` | Run `cargo fmt --check` on the grading copy. |
| `prompt` | required | Sent to the client unchanged. |
| `expected_output` | optional | A human-readable description of success; not graded. |
| `[expected] hooks` | `[]` | Plugin hooks that must leave a successful record. |
| `[expected.skills] installed` | `[]` | Skills that must be installed during the session. |
| `[expected.skills] not_installed` | `[]` | Skills that must not be installed. |
| `[expected.skills] used` | `installed` minus `not_used` | Skills the agent must successfully load. `[]` makes no claim. |
| `[expected.skills] not_used` | `[]` | Skills the agent must not load. |

`--check-tasks` rejects a task when:

- a skill appears in both `installed` and `not_installed`, or both `used` and `not_used`;
- a skill in `used` also appears in `not_installed`;
- a `checks/` file does not hold exactly one test, or a named plugin or fixture does not exist.

Fixture crates are not workspace members and have no `skills/` directory, so the only way their rule reaches the agent through Symposium is a registry plugin.

## Preparing a trial

All batches live under one evaluation root:

```text
<eval-root>/<batch>/
  tools/
    common/                      # staged clients, readable by every trial
    symposium/                   # staged cargo-agents, readable by Symposium trials only
  trial-1-with-symposium/        # <run>
    home/         # HOME for every process in the trial
    project/      # copy of the starter
    fixtures/     # read-only copies of the task's fixture crates
    bin/          # links to staged tools: the client, sh, and (Symposium only) cargo-agents
    cargo-home/   # CARGO_HOME, empty at start
    tmp/          # TMPDIR
    evidence/     # EVAL_EVIDENCE_DIR and SYMPOSIUM_HOOK_TRACE, outside the project
    registry/     # the task's plugins (Symposium configuration only)
```

The executables a trial runs are **staged** once per batch: the client into `tools/common/` (a single binary for native installs; for npm installs, the package directory and the Node.js runtime), and the `cargo-agents` under evaluation into `tools/symposium/`. They are copied, not linked: on Linux, `srt` matches read rules by the name a path is opened under, so a link to a client under the developer's home would not launch. Staging also fixes the binaries for the whole batch, and their hashes go into each result.

Claude Code (`~/.claude`), Codex (`~/.codex`), and Symposium (`~/.symposium`) keep their configuration and state under the home directory, so pointing `HOME` (and `USERPROFILE` on Windows) at `<run>/home` gives each trial fresh state.

The client starts with a cleared environment holding only `HOME`, `PATH`, `TMPDIR`, `CARGO_HOME`, `EVAL_EVIDENCE_DIR`, `SYMPOSIUM_HOOK_TRACE`, and the model API key. `PATH` is, in order:

1. `<run>/bin`;
2. the active toolchain's own `bin/` directory (the directory of `rustup which cargo`), which holds only Rust tools;
3. the system directories (`/usr/bin`, `/bin`), which hold the linker and the tools the client needs.

The developer's `~/.cargo/bin` is never on `PATH`, since it can hold their own `cargo-agents`.

In the Symposium configuration, the runner copies the task's plugins into `<run>/registry`, writes this config, and runs `cargo agents init --add-agent <client>`:

```toml
auto-sync = true
hook-scope = "global"
auto-update = "off"

[defaults]
symposium-recommendations = false
user-plugins = false

[[registry]]
name = "eval"
path = "<run>/registry"
```

`init` registers hooks without syncing. Global scope matters: project-scoped hooks only exist after a sync, which would install skills before the behavior under test. The prompt never mentions Symposium, and telemetry stays at its default (off).

Before launch, the runner checks that the skill directory is empty and that the client's machine-wide managed-settings locations are empty. For a baseline, it also checks that `cargo-agents` resolves nowhere on `PATH` and that there is no `~/.symposium` and no Symposium entry in the client's hook or MCP configuration. A failed check is a setup failure.

The starter has no git history, so one trial cannot read another's work through it. Each trial starts a new conversation. The API key is passed to the client process only and never retained. The runner refuses clients older than a pinned minimum version, since hook and transcript formats change between versions.

## Isolation

### What "without Symposium" means

`cargo-agents` stays installed on the developer's machine throughout. "Without Symposium" means no process in the trial can reach it. Nothing is installed or uninstalled between configurations.

|                                     | Without Symposium | With Symposium                    |
| ----------------------------------- | ----------------- | --------------------------------- |
| `<run>/bin/cargo-agents`            | absent            | link to the staged binary         |
| Staged binary in `tools/symposium/` | unreadable        | readable                          |
| Hooks in `<run>/home`               | none              | registered by `cargo agents init` |
| Developer's installed copies        | unreadable        | unreadable                        |

Symposium registers its hooks as `cargo-agents hook <agent> <event>`, a bare name the client resolves on `PATH`. The trial's `PATH` holds no developer directories, and `srt` makes the developer's home unreadable, so an installed copy cannot be reached by any path.

### Enforced: `srt`

[`srt`](https://github.com/anthropics/sandbox-runtime) is the open-source sandbox runtime Claude Code's own sandbox is built on, packaged as a command that wraps a program and every process it starts. It uses Seatbelt on macOS and bubblewrap on Linux. The runner pins an `srt` version, records it in each result, and launches each client as `srt --settings <run>/srt.json <client> ...`.

`srt` reads are deny-then-allow: everything is readable unless a `denyRead` region covers it, and `allowRead` reopens paths inside a denied region. Writes and network are denied unless allowed. The runner generates this policy per trial and keeps it with the results:

```json
{
  "filesystem": {
    "denyRead": ["<user data roots>", "<eval-root>", "<symposium checkout>"],
    "allowRead": ["<run>", "<batch>/tools/common", "<toolchain>"],
    "allowWrite": ["<run>/project", "<run>/home", "<run>/cargo-home", "<run>/tmp", "<run>/evidence"]
  },
  "network": {
    "allowedDomains": ["<the client's model API host>"]
  }
}
```

The denied regions are:

- **user data roots**: every user's home and the usual mount points (`/home`, `/root`, `/mnt`, `/media` on Linux; `/Users` and `/Volumes` on macOS);
- **the evaluation root**, so other trials and earlier batches are unreadable;
- **the Symposium checkout**, which holds `evals/`, so hidden tests, solutions, and other tasks' plugins are unreadable.

A Symposium trial's policy also lists `<batch>/tools/symposium` in `allowRead`; a baseline's never does. Everything else stays readable, notably the system directories and shared temporary directories outside the evaluation root. A machine with user data elsewhere needs those paths added to `denyRead`, which the runner accepts as configuration.

Because `srt` is the boundary, each client's own sandbox is off inside it and the client runs without permission prompts: Claude Code with `--permission-mode bypassPermissions`, Codex with `--sandbox danger-full-access`. Both clients document those modes for environments that are already isolated. Inside `srt`, prompts would only duplicate the boundary, and in non-interactive mode they would stop the agent from running `cargo test`.

Enforced mode is the default on macOS and Linux and requires `srt` and its dependencies (`ripgrep`, plus `bubblewrap` and `socat` on Linux). Only enforced trials are compared.

### Exploratory: without `srt`

When `srt` is unavailable, or on Windows until its support leaves alpha, `--exploratory` runs trials inside each client's own sandbox, as restrictive as it allows:

- **Claude Code**: through `--settings`, `sandbox.enabled`, `failIfUnavailable`, `allowUnsandboxedCommands: false`, `filesystem.denyRead` for the developer's home with `allowRead` for the trial and toolchain, `allowWrite` for `<run>/cargo-home`, and `permissions.blockReadsOutsideWorkingDirectories`, with `--permission-mode acceptEdits`.
- **Codex**: `--sandbox workspace-write`, with `<run>/cargo-home`, `<run>/home`, and `<run>/evidence` as `sandbox_workspace_write.writable_roots`. Codex documents no way to restrict reads.

An escape audit scans each transcript for paths outside the trial and for any `cargo-agents` call in a baseline. It parses command text, so it can be evaded and an empty audit proves nothing; that is why exploratory results are labeled and never compared. In enforced mode, the same audit runs as a secondary check.

## Running the client

Both adapters run, in enforced mode:

```sh
srt --settings "$RUN/srt.json" claude -p "$PROMPT" --output-format stream-json --verbose \
  --model "$MODEL" --permission-mode bypassPermissions
srt --settings "$RUN/srt.json" codex exec --json --model "$MODEL" \
  --sandbox danger-full-access --skip-git-repo-check --dangerously-bypass-hook-trust "$PROMPT"
```

Codex runs a hook only after it is trusted. The trial's fresh home contains no hooks except Symposium's, so `--dangerously-bypass-hook-trust` applies to exactly those, for that one invocation. Codex requires a git repository unless told otherwise, hence `--skip-git-repo-check`.

A missing completion event or a client-reported error is a client error, even when the process exits 0. When the agent's deadline expires, the runner kills the process tree, including any hooks, and keeps whatever evidence exists.

## Grading the code

The runner first keeps the submission as the agent left it, then builds a **grading copy**: a fresh copy of the starter and fixtures from `evals/`, with only the `editable` paths taken from the submission.

- Within `editable`, the submission replaces the starter exactly: the starter's editable files are removed first, so a file the agent deleted stays deleted.
- Only regular files and directories are accepted. A symlink or special file under an editable path invalidates the submission, which is a failed trial. Every destination is checked to resolve inside the grading copy before it is written.
- Edits outside `editable` are listed in the result and never graded.

Grading runs under `srt` with its own fresh writable paths, read-only fixtures, no network, and no credentials. Then, in order:

1. `cargo build`. If it fails, the trial fails and the remaining checks are skipped.
2. `cargo test --lib`, however many tests the agent wrote.
3. Each hidden test, as an explicit target (`cargo test --test <name>`). It must pass, and exactly one test must have run, with none ignored or filtered out.
4. `cargo fmt --check`, when `fmt_check` is set.

Every step has a deadline. A failure is either the submission's or the runner's:

- **Submission failure**: the code does not build, a test fails or crashes, or a test exceeds its deadline. The trial fails.
- **Grading infrastructure error**: the sandbox could not launch, a fixture was unavailable, or the runner could not collect results. The trial is excluded and reported, and its kept submission can be graded again.

## Expectations

An expectation is the task author's assertion, in `[expected]`. The runner decides each one from what it observed, as `pass`, `fail`, or `inconclusive`. Inconclusive means the evidence cannot decide, and it is never silently turned into a pass or a fail. Expectations apply to the Symposium configuration only.

### Skills installed

`installed` passes when the skill directory holds the skill with Symposium's `.symposium` marker and content matching the plugin in `evals/`. `not_installed` passes when it is absent.

### Skills used

From the transcript, the adapter observes each skill's load as one of:

- `loaded`: a successful `Skill` tool call naming it, or a successful read of its installed `SKILL.md`;
- `attempted`: such a call that failed or was denied;
- `not_observed`: neither appears in a transcript the adapter fully interpreted;
- `unknown`: the adapter could not interpret the transcript.

| Assertion  | pass                          | fail                        | inconclusive |
| ---------- | ----------------------------- | --------------------------- | ------------ |
| `used`     | `loaded`                      | `attempted`, `not_observed` | `unknown`    |
| `not_used` | `not_observed`, `attempted`   | `loaded`                    | `unknown`    |

A failed or denied attempt never delivered the skill's content, so it satisfies `not_used`; it is still reported. A client could supply a skill in a way its adapter does not see, so each adapter documents what it detects.

### Synchronization and hooks

Every Symposium trial also requires:

- the skill directory was empty before launch;
- a `SessionStart` hook trace with a successful exit;
- Symposium recorded a successful sync for the project (it records one only after a sync succeeds).

A missing `SessionStart` invocation fails. Evidence too incomplete to tell whether a sync completed is inconclusive. Auto-sync failures do not fail the agent's session, so a successful client exit is not evidence that a sync happened.

Each hook in `[expected] hooks` must leave at least one successful record. `Stop` can fire more than once, so its first record is the one checked.

### What the evidence can and cannot show

Hook records, hook traces, and installed skills are written by processes inside the trial: Symposium's hooks, which the client starts. The same boundary that lets them write lets the agent write there too, so the agent could, in principle, alter a record, edit an installed skill, or change the hook configuration. They are supporting evidence, cross-checked from outside the trial:

- installed skill contents are compared with the plugin in `evals/`, which the trial cannot read or write;
- the client's hook and Symposium configuration are snapshotted after `init` and compared after the trial, and any change is recorded;
- the transcript, captured from the client's output stream rather than from a file, is the primary record of what the agent did, including any write to `evidence/` or `home/`.

A hook record contradicted by the transcript, or a changed configuration, makes the affected expectations inconclusive. Independently verified hook execution would need a recording channel outside the agent's writable paths; that is follow-up work if tampering is ever observed.

## Outcomes and counting

A trial can succeed only if its client session completed. A timeout or client error is a failure even when the code it left would pass; that code is still graded and reported, and a grading infrastructure error during it does not move the trial out of the count. For a completed trial:

| Kind       | Succeeds when                                  | Inconclusive when                                               |
| ---------- | ---------------------------------------------- | --------------------------------------------------------------- |
| Regression | Every code check and every expectation passes. | An expectation is inconclusive and nothing failed.              |
| Capability | Every code check passes.                       | Never. Expectations are reported but do not decide the outcome. |

A missing hook or skill is a Symposium failure, not a point in the baseline's favor.

Each planned trial ends in exactly one place:

| Count                           | Meaning                                                                                |
| ------------------------------- | -------------------------------------------------------------------------------------- |
| setup failures                  | the trial never launched its client                                                    |
| cancelled                       | the batch stopped before the trial ran                                                 |
| isolation exclusions            | the client launched, but the trial violated isolation                                  |
| grading exclusions              | the trial completed, but grading needed to decide it hit an infrastructure error       |
| succeeded, failed, inconclusive | everything else, by outcome                                                            |

```text
planned  = setup failures + cancelled + started
eligible = started - isolation exclusions - grading exclusions
         = succeeded + failed + inconclusive
```

For each configuration, `benchmark.json` reports:

- **success rate**: succeeded divided by eligible, or `null` with no eligible trials, never 0 or 1. For regression tasks, the succeeded, failed, and inconclusive counts are reported alongside;
- **setup failures, cancellations, and exclusions**, by cause;
- **pass_all**: whether every eligible trial succeeded, or `null` with none;
- **observed loads**, per skill;
- **cost**: mean tokens (input, output, and cache) and mean agent time, over eligible trials and over succeeded trials, each with its own count of contributing trials, since a client may report time but not tokens. Agent time excludes setup and grading. Usage a client does not report is `null` and left out of the means;
- **delta**: the Symposium configuration minus the baseline, for each rate and mean.

The order of configurations alternates between rounds (baseline first, then Symposium first, and so on). Failed trials are never retried or discarded; a rerun is a new batch.

## Retained results

```text
results/store-durability/claude/<batch>/
  benchmark.json
  trial-1-without-symposium/
    result.json
    srt.json
    submission/          # the project as the agent left it
    grading/             # the grading copy's output
    transcript.jsonl
    response.txt
    stderr.log
  trial-1-with-symposium/
    ...                  # as above, plus:
    hook-trace.jsonl
    evidence/
    symposium.log
```

A trial's `result.json`:

```json
{
  "task": { "name": "store-durability", "kind": "capability", "content_hash": "<sha256 of task, fixture, and plugin dirs>" },
  "client": { "name": "claude", "version": "<version>" },
  "model": { "name": "<model>", "settings": {} },
  "mode": "with-symposium",
  "trial": 3,
  "order": 2,
  "symposium": { "version": "<cargo-agents --version>", "sha256": "<binary hash>", "revision": "<git sha or null>" },
  "os": "<os and arch>",
  "toolchain": "<rustc version>",
  "deadlines_secs": { "setup": 120, "agent": 600, "grading": 300 },
  "elapsed_secs": 94,
  "usage": { "input_tokens": 30100, "output_tokens": 4100, "cache_read_tokens": 7000, "cache_write_tokens": 0 },
  "execution": "completed",
  "isolation": { "mode": "enforced", "srt_version": "<version>", "audit_findings": [], "config_changes": [] },
  "code": {
    "build": "pass",
    "cargo_test_lib": "pass",
    "hidden": { "settings_persist": "pass" },
    "cargo_fmt_check": null,
    "edits_outside_editable": []
  },
  "expectations": {
    "skills": { "fixture-store": { "installed": "pass", "used": "pass", "observed_load": "loaded" } },
    "hooks": {},
    "sync": { "skill_dir_empty_before": "pass", "session_start": "pass", "sync_recorded": "pass" }
  },
  "success": "pass"
}
```

The batch's `benchmark.json`:

```json
{
  "task": "store-durability",
  "client": "claude",
  "isolation": "enforced",
  "configurations": {
    "without-symposium": {
      "planned": 5, "setup_failures": 0, "cancelled": 0, "started": 5,
      "excluded": { "isolation": 0, "grading": 0 },
      "eligible": 5, "succeeded": 2, "failed": 3, "inconclusive": 0,
      "success_rate": 0.4, "pass_all": false,
      "cost": {
        "eligible": { "tokens": { "mean": 38900, "trials": 5 }, "agent_secs": { "mean": 81, "trials": 5 } },
        "succeeded": { "tokens": { "mean": 44100, "trials": 2 }, "agent_secs": { "mean": 92, "trials": 2 } }
      }
    },
    "with-symposium": {
      "planned": 5, "setup_failures": 0, "cancelled": 0, "started": 5,
      "excluded": { "isolation": 0, "grading": 0 },
      "eligible": 5, "succeeded": 4, "failed": 1, "inconclusive": 0,
      "success_rate": 0.8, "pass_all": false,
      "observed_load": { "fixture-store": { "loaded": 4, "attempted": 0, "not_observed": 1, "unknown": 0 } },
      "cost": {
        "eligible": { "tokens": { "mean": 35100, "trials": 5 }, "agent_secs": { "mean": 70, "trials": 5 } },
        "succeeded": { "tokens": { "mean": 36000, "trials": 4 }, "agent_secs": { "mean": 72, "trials": 4 } }
      }
    }
  },
  "delta": { "success_rate": 0.4 }
}
```

## The runner in code

The runner lives in `xtask` and is synchronous: trials run one after another and only spawn subprocesses, so async would add a runtime without adding concurrency. It reuses `symposium-testlib`'s fixture preparation and hook trace types, but adds its own two setup modes instead of routing through `with_fixture`, whose unconditional `init` and `sync` are exactly what this evaluation must avoid. Each trial's environment is set on the client's `Command`, never on the runner's own process.

`Err` means the runner itself broke, such as a full disk. Everything that can go wrong in a trial is an outcome. A trial allocates its result directory first and finalizes a result on every path, so no failure loses evidence:

```rust
fn run_trial(
    task: &Task,
    client: &impl ClientRunner,
    plan: &TrialPlan,
) -> anyhow::Result<TrialResult> {
    let mut result = TrialResult::started(task, plan, ResultDir::create(plan)?);

    let run = match RunDir::prepare(task, plan, plan.setup_deadline) {
        Ok(run) => run,
        Err(err) => return result.finalize_setup_error(err),
    };
    result.execution = client.run(run.env(), task.prompt(), plan.agent_deadline);
    result.grading = grade(task, &run, plan.grading_deadline);

    // `run` is still alive here, so its evidence is copied before the directory is removed.
    result.finalize(run)
}
```

`grade` returns a value, not a `Result`: a submission that fails or hangs produces failed checks, and only an infrastructure problem becomes `Grading::InfrastructureError`.

A client adapter only builds its command line. Isolation lives in `RunEnv`, whose `command` clears the environment, sets the trial's `HOME`, `PATH`, and working directory, and wraps the program in `srt`, so adding a client means adding one small adapter:

```rust
pub trait ClientRunner {
    fn run(&self, env: &RunEnv, prompt: &str, deadline: Duration) -> Execution;
}
```

Everything except the model call is testable without a client or API key. For example, the guarantee behind "without Symposium":

```rust
#[test]
fn baseline_cannot_reach_cargo_agents() -> anyhow::Result<()> {
    let opts = RunOpts::for_test();
    let task = Task::load(&opts.evals_root, "store-durability")?;
    let run = RunDir::prepare(&task, &opts.plan(Mode::WithoutSymposium), opts.setup_deadline)?;

    let status = run
        .env()
        .command("sh")
        .args(["-c", "command -v cargo-agents || cat \"$DEVELOPER_CARGO_BIN/cargo-agents\""])
        .env("DEVELOPER_CARGO_BIN", opts.developer_cargo_bin())
        .status()?;
    assert!(!status.success());
    Ok(())
}
```

The second half of that command reads the developer's copy by absolute path, which `srt` must refuse.
