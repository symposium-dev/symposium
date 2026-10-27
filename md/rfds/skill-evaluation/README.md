# Evaluating agent tasks with and without Symposium

## TL;DR

- Add an internal `cargo xtask agent-eval` runner. It starts a real Claude Code or Codex CLI session on a small Rust task, with or without Symposium, then checks the code the agent left behind and what Symposium did along the way.
- Start with two tasks. `formatting` checks that a skill and a hook reach a brand-new session. `store-durability` compares the agent with and without a skill for a crate it cannot already know.
- Each trial runs inside [`srt`](https://github.com/anthropics/sandbox-runtime), so the agent cannot read your files, other trials, or the tests used to grade it.
- Designing this surfaced a suspected bug: Claude Code may not see skills that Symposium installs during the first session, because Symposium never asks it to reload them. We reproduce it first, then fix it.

## Motivation

We want to measure whether Symposium changes how an agent performs a task. Three gaps prevent that today.

First, our agent-mode fixtures always initialize *and* synchronize Symposium before the agent starts (`with_fixture` in `symposium-testlib/src/lib.rs` runs `init` then `sync`). Skills are already installed when the session begins, so no test exercises the path a new user hits: the first `SessionStart` hook installing skills on its own. That path has a suspected gap: according to the [Claude Code hooks documentation](https://code.claude.com/docs/en/hooks), Claude Code discovers skills before `SessionStart` hooks finish, so a skill a hook installs would be missing from the first prompt unless the hook asks for a rescan, and Symposium does not ask today.

Second, there is no baseline. Every agent-mode run has Symposium present, so we cannot compare an ordinary agent run against one with Symposium on the same task.

Third, delivering a skill is not the same as the agent using it. In [Vercel's evaluation](https://vercel.com/blog/agents-md-outperforms-skills-in-our-agent-evals), an agent never invoked an available skill in 56% of cases, and with the skill available but unprompted, results matched having no documentation at all. So we need to check whether a skill was used, not only whether it was installed.

This RFD builds the smallest runner that closes these gaps.

## Change in a nutshell

| Task | Clients | Configurations |
|---|---|---|
| `formatting` | Claude Code and Codex CLI | With Symposium |
| `store-durability` | Claude Code and Codex CLI | With and without Symposium |

- **What can I run?** The two tasks above, with `cargo xtask agent-eval <task>`. Developers run them by hand when changing how Symposium delivers skills and hooks; they call a model, so they never run in CI.
- **What happens?** Each trial gets a fresh environment and its own agent session, then the runner checks what Symposium did and tests the agent's code, and keeps everything.
- **What must I write?** A starter project, a prompt, the plugins to offer, what Symposium should do, and hidden tests.
- **What are we building?** An internal runner in `xtask`, two client adapters, isolation with `srt`, the checks, and the report.
- **What changes in Symposium?** Reproduce and fix first-session skill loading on Claude Code, and add `Stop` hook support for Codex.

Out of scope for now: model-graded or human checks, more clients, parallel trials, and importing `evals.json` files.

The rest of this RFD follows the runner from the user's side: [what happens when you run an eval](#what-happens-when-you-run-an-eval), using `formatting`, then [comparing with and without Symposium](#comparing-with-and-without-symposium), using `store-durability`, and [what you do when you write an eval](#what-you-do-when-you-write-an-eval). The precise rules are on [a reference page](./reference.md); you should not need them to follow the proposal.

A few terms, from [Anthropic's guide to agent evals](https://anthropic.com/engineering/demystifying-evals-for-ai-agents):

- A **task** is one test: a starter project, a prompt, and what counts as success.
- A **trial** is one attempt at a task in one configuration, with or without Symposium.
- The **transcript** is the client's full event log for a trial.
- A **hidden test** is a test stored with the task in `evals/` and never copied into the agent's project. The agent cannot see or change it.

## Detailed plans

### What happens when you run an eval

This proposal adds a command for running agent evaluations from the Symposium checkout. The command does not exist yet. You choose a task, a client (`claude` or `codex`), and a model; the runner prepares the projects, starts the agent, checks the results, and saves the evidence. The `formatting` task checks Symposium's integration, while `store-durability` compares outcomes with and without Symposium.

Take the `formatting` task on Claude Code:

```sh
cargo xtask agent-eval formatting --client claude --model <model>
```

The task asks the agent to add a small function to a crate. Its plugin carries a `rust-formatting` skill and a `Stop` hook that runs `cargo fmt` when the agent finishes. Here is what the runner does.

**1. Prepare a fresh trial.** The runner builds `cargo-agents` from your checkout (or uses `--symposium-bin <path>`). It creates a trial directory with an empty `home/`, a copy of the starter in `project/`, and a `bin/` holding `claude` and `cargo-agents`. Every process in the trial uses that `home/` as `HOME`, so your own Claude Code, Codex, and Symposium configuration never leaks in.

It then runs `cargo agents init` with global hooks and a registry holding only the task's plugins. It does **not** run `cargo agents sync`, so the skill directory starts empty. Installing the skill is Symposium's job during the session, and that is part of what the trial checks.

**2. Start the agent.** The runner launches `claude` inside `srt` with the task's prompt. The agent can write only inside its trial directory and can reach only the model API. It cannot see your home directory or `evals/`, where the hidden tests live; it can still read system and toolchain files. The [reference](./reference.md#isolation) gives the exact boundary.

**3. Symposium syncs.** When the session starts, Claude Code runs Symposium's `SessionStart` hook. Symposium sees the project, installs the `rust-formatting` skill, and asks Claude Code to reload its skills so the first prompt can use it. That last part is the suspected bug [described below](#reload-skills-after-the-first-sync): Symposium does not ask today.

**4. The agent works.** The prompt names the skill, so Claude should load it. When the agent finishes, the plugin's `Stop` hook records whether the code was already formatted and then runs `cargo fmt`.

**5. Check what Symposium did.** The runner checks that:

- the `SessionStart` hook ran and succeeded, and Symposium recorded a successful sync;
- the skill was installed, with Symposium's `.symposium` marker and content matching the plugin;
- the agent successfully loaded the skill, according to the transcript;
- the `Stop` hook left a successful record.

A successful load shows the skill reached the agent in that same session. It does not show the agent followed it, and it does not show whether the client listed the skill on its own.

**6. Check the code.** Grading never runs in the agent's project. The runner builds a fresh copy of the starter and takes only the agent's `src/` from the submission, so the agent cannot weaken a test by editing `Cargo.toml` or a build script. If that copy does not build, the trial fails, the remaining code checks are skipped, and its evidence is still kept. Otherwise the runner runs the agent's own `cargo test --lib`, each hidden test, and `cargo fmt --check`.

The agent's own tests alone prove little, since they could pass with nothing meaningful in them. The hidden tests are what check the behavior the task asked for.

**7. Report the result.** A `formatting` trial **passes** when every check in steps 5 and 6 passes, and **fails** when any of them fails. It is **inconclusive** when the evidence cannot decide a check and nothing failed: for example, the runner could not interpret the transcript, or the hook evidence is missing or contradicts the transcript. A timeout or client error is a failure. The [reference](./reference.md#expectations) has the exact rules.

The trial's result directory keeps the transcript, the submission as the agent left it, the grading output, the hook traces, and Symposium's log, so any result can be checked by hand. A rerun creates a new batch; earlier results are never overwritten.

### Comparing with and without Symposium

`formatting` only runs with Symposium, because it asks whether Symposium delivered. The interesting question for a capability task is whether Symposium helps at all, which needs a baseline:

```sh
cargo xtask agent-eval store-durability --client claude --model <model> --trials 5
```

Each round now runs two trials. The Symposium trial is the one above. The **baseline** trial is prepared the same way, except that `bin/` has no `cargo-agents`, `init` never runs, and `srt` makes the staged binary unreadable. Your own installed `cargo-agents` stays on your machine the whole time; no trial can reach it. The order alternates between rounds, so neither configuration always goes second.

For each configuration, the report shows:

- the **success rate**: trials that succeeded, out of trials that say something about the agent. Timeouts and client errors count as failures, so a configuration cannot look better by failing early. Setup failures and grading infrastructure errors say nothing about the agent; they are counted separately, and any of them is a reason to inspect the batch;
- how often the agent **loaded** each skill;
- the **cost**: mean tokens and agent time, over all those trials and over the successful ones only, so a configuration that is cheaper only because it gives up early does not look like a saving;
- the **difference** between the two configurations.

Five trials per configuration are limited evidence. The report gives numbers and claims nothing more.

Without `srt`, `--exploratory` runs trials inside each client's own, narrower sandbox. Those results are labeled exploratory, are useful for developing tasks, and are never compared.

### What you do when you write an eval

Here is the whole `formatting` task. A plugin can be shared between tasks, so it lives on its own:

```text
evals/
  plugins/
    format-on-stop/
      Symposium.toml        # depends-on = ["*"], one skill, one Stop hook
      skills/rust-formatting/SKILL.md
      scripts/format-on-stop.sh
  tasks/
    formatting/
      task.toml
      starter/              # a small crate with no dependencies
      checks/
        double.rs           # hidden test
```

```toml
kind = "regression"
plugins = ["format-on-stop"]
fmt_check = true
prompt = """
Use the rust-formatting skill. Add `pub fn double(x: i32) -> i32` to src/lib.rs.
"""
expected_output = "double returns twice its argument, and the crate is formatted."

[expected]
hooks = ["format-on-stop"]

[expected.skills]
installed = ["rust-formatting"]
```

Writing it takes these steps.

**1. Pick the kind.** A regression task asks whether Symposium still delivers; its expectations decide whether a trial succeeds. A capability task asks whether Symposium improves the outcome; it runs both configurations and compares their code checks, while its expectations are graded and reported but do not change the comparison. For example, in a capability trial:

- the code passes and the skill was not loaded: the trial succeeds, and its `used` expectation fails;
- the code fails and the skill was loaded: the trial fails, and its `used` expectation passes.

**2. Write the starter and the prompt.** Ask for a public function, so a hidden test can call it. This prompt names the skill. That is fine here, because the task checks delivery, not whether the agent picks a skill on its own. Our capability comparisons are unprompted, so they never name the skill; a prompted comparison would be valid, but it answers a different question.

**3. Choose the plugins** from `evals/plugins/`.

**4. Declare what Symposium should do.** `hooks` lists plugin hooks that must leave a successful record. `[expected.skills]` holds four lists:

- `installed`: skills that must be installed during the session;
- `not_installed`: skills that must not be;
- `used`: skills the agent must successfully load, according to the transcript. A load is evidence the skill reached the agent, not proof it followed the skill;
- `not_used`: skills the agent must not load.

When `used` is omitted, it defaults to `installed` minus `not_used`, which is why the example above needs only `installed`. An explicit `used = []` makes no claim about use. A skill can be installed but irrelevant to the task:

```toml
[expected.skills]
installed = ["rust-formatting", "release-notes"]
not_used = ["release-notes"]    # installed, but nothing in this task calls for it
```

Lists that contradict each other, such as a skill in both `used` and `not_used`, are rejected.

**5. Write the hidden tests** in `checks/`. Each file holds exactly one test, and the runner checks that exactly one test ran for it, which catches a typo in a test name or a test that was never discovered. `checks/double.rs`:

```rust
#[test]
fn doubles() {
    assert_eq!(formatting::double(21), 42);
}
```

**6. Optionally add a `solution/`.** These are files copied over the starter that should pass every code check. `cargo xtask agent-eval --check-tasks` always validates every `task.toml`, and runs in CI without a model or API key. When a task has a `solution/`, it also grades the solution, which catches a broken hidden test before it shows up as a 0% success rate. Passing `--check-tasks` without a solution does not show that the hidden tests work.

**7. Run it.** Run `--check-tasks`, then the task itself.

### Writing a capability task

A capability task needs knowledge the model cannot already have, or both configurations succeed and the comparison shows nothing. `store-durability` uses `fixture-store`, a crate that exists only in this repository. Its `Store::put` buffers writes in memory, and they reach disk only on `Store::flush`. A `Store` dropped without flushing loses its writes silently. The code compiles and runs either way.

The prompt, which does not mention the skill:

> Add a function `pub fn save_settings(dir: &Path, settings: &[(&str, &str)]) -> std::io::Result<()>` to src/lib.rs that saves each key/value pair with fixture-store, using a `Store` opened at `dir`.

A registry plugin, gated on the dependency, carries a skill that states the rule:

```toml
name = "fixture-store"
depends-on = ["fixture-store"]

[[skills]]
source.path = "skills"
```

```markdown
---
name: fixture-store
description: Use when reading or writing data with the fixture-store crate.
---

`Store::put` buffers writes in memory. They reach disk only when `Store::flush`
is called, and a `Store` dropped without flushing loses them without an error.
Call `flush()` before returning from any function that writes.
```

The hidden test calls `save_settings`, opens a *new* `Store` at the same directory, and checks that every value is there. Without the flush, it fails. Its `task.toml` differs from `formatting` in three ways:

```toml
kind = "capability"
plugins = ["fixture-store"]
fixtures = ["fixture-store"]    # copied into the trial read-only, outside the project
editable = ["src/**"]           # the only files taken from the submission; the default

[expected.skills]
installed = ["fixture-store"]   # graded and reported, but not part of the comparison
```

The made-up crate reduces the chance that the model already knows the rule. A baseline may still infer it from familiar buffering conventions or read it in the crate's source, so pilot runs will tell us whether the task separates the two configurations. If the baseline finds the rule, that is a fair result: the success rates converge, and the cost difference shows what the skill saved.

An illustrative result:

|                       | Without Symposium | With Symposium      |
| --------------------- | ----------------- | ------------------- |
| Skill installed       | (none)            | at `SessionStart`   |
| Skill loaded          | (none)            | 4 of 5 trials       |
| Task succeeded        | 2 of 5            | 4 of 5              |
| Mean tokens and time  | baseline          | difference reported |

### Changes to Symposium

#### Reload skills after the first sync

Installing a skill during `SessionStart` is not enough for the first prompt to see it. Claude Code runs skill discovery before `SessionStart` hooks finish, and rescans only when a hook returns `"reloadSkills": true` in its `SessionStart` output. Symposium's `SessionStart` handler (`handle_session_start` in `src/hook.rs`) never sets it, so we suspect a skill installed in the first session is invisible until the next one.

This is a suspected bug in its own right, so it is tracked separately from the runner. We first write a test that shows it (a skill installed by `SessionStart` that the first prompt cannot load), then fix it: after a successful auto-sync on `SessionStart`, the Claude Code response always includes `reloadSkills: true`. Reloading only when skills changed would need `sync::sync` to report what it changed, which it does not today; that can come later if rescans prove costly.

For Codex, third-party sources indicate skills are scanned only at startup, and its documentation describes no reload mechanism. Step 7 determines what Codex actually does. If it cannot load a skill installed during its own session, that is a measured limitation on Codex, reported as such, not hidden by syncing in advance.

#### Stop hooks on Codex

Codex supports `Stop` hooks, but Symposium does not register or map the event for Codex today: `register_codex_hooks` in `src/agents/mod.rs` registers four events, and `src/hook_schema/codex.rs` has no `Stop` arm. This proposal adds it, following the Claude mapping. Until it lands, `formatting` on Codex fails its hook expectation.

## Frequently asked questions

### Will tasks support grader prompts?

Not in the first version. Every check in these two tasks has an objective answer: a test passes, a file exists, a transcript shows a load. A check that needs judgment, such as whether code follows a DSL's idioms, needs a model-graded check calibrated against human review. The [formality evaluation](https://github.com/rust-lang/a-mir-formality/blob/main/crates/formality-core/skills/a-mir-formality-idiomatic-judgment-fn/evals/evals.json) will likely be the first task that needs one.

### Why don't skill expectations decide a capability task?

A capability task compares the two configurations on the same code checks. The baseline has no skills, so if "the skill was used" decided success, the Symposium configuration would face a check the baseline never does. Expectations are still graded and reported next to the result, so a trial that passed without loading the skill is visible.

### Why check discoverability only through use?

No client is known to list its available skills reliably in the transcript, so "the client could see the skill" cannot be observed directly. A successful load is the evidence we can get, and `formatting` names its skill so a working delivery produces one. Reading a client's skill listing is follow-up work.

### Why is `solution/` optional?

It is most useful when the hidden test is subtle, as in `store-durability`, where a broken test would look like the agent failing in both configurations. For `formatting`, it adds little. Without it, `--check-tasks` still validates the task, but cannot show the hidden tests work.

### Why no negative tasks yet?

Whether a skill is installed in a project it does not apply to is decided by Symposium's sync, without the agent, and the existing in-process tests already cover it. Whether an agent leaves an installed but irrelevant skill alone is an agent question, and any task can ask it with `not_used`.

### What did we take from existing evaluation practice?

Isolated trials, checks on the outcome rather than the steps, a baseline over several trials, tasks the model cannot already solve, a check on whether the skill was used, and checks the agent cannot reach. These recur across [Anthropic's guide](https://anthropic.com/engineering/demystifying-evals-for-ai-agents), the [Agent Skills guide](https://agentskills.io/skill-creation/evaluating-skills), [Vercel's evaluation](https://vercel.com/blog/agents-md-outperforms-skills-in-our-agent-evals), and [Terminal-Bench](https://arxiv.org/html/2601.11868v1). `task.toml` uses the Agent Skills `evals.json` field names where they overlap, so that format can be imported later.

### Why `srt`?

It restricts reads, writes, and network for the client and every process it starts, with no container image to maintain, and puts the same boundary around both clients. The clients' own sandboxes differ: Codex documents no way to restrict reads, and Claude Code's sandbox does not cover its file tools. See [Isolation](./reference.md#isolation).

### Why grade only the editable files?

So the agent cannot make a hidden test pass by editing `Cargo.toml`, a build script, or the fixture crate. See [Grading the code](./reference.md#grading-the-code).

### Why count timeouts as failures?

Excluding them would let a configuration look better by failing early. See [Outcomes and counting](./reference.md#outcomes-and-counting).

### Why several trials?

Agent results vary between runs, by more than many differences worth detecting. Five trials per configuration is a starting point; regression tasks, which should succeed every time, run once by default.

### Why is formatting a regression task and not a comparison?

Models already format Rust well, so a baseline would likely succeed every time. The task is still the simplest end-to-end check that `SessionStart` installs a skill the agent can load and that a plugin's `Stop` hook fires. A `Stop` hook rather than `PostToolUse` keeps the agent from always reading already-formatted files, so the hook's record shows what the agent wrote itself.

### Why not only use simulation tests?

Simulation tests invoke hooks by hand with prepared input. They confirm Symposium handles an event correctly, not that a real client sends that event, at that time, or sees the skills a hook installed. The suspected first-session reload bug is exactly the kind of problem simulation cannot find.

### Why not use Inspect?

[Inspect](https://inspect.aisi.org.uk/) provides scoring, logs, and sandboxing, and its [Agent Bridge](https://inspect.aisi.org.uk/agent-bridge.html) supports Claude and Codex. It is a viable alternative. For two tasks with code checks, a Rust runner fits the existing tests and calls the clients directly. If model-graded scoring becomes most of the work, we should revisit this.

### What are the costs and limitations?

Real trials cost model usage and take minutes each, so a five-trial comparison on two clients is twenty sessions. Results from different machines are comparable only when the recorded client versions match. Isolation needs macOS or Linux today. A load shows the agent saw a skill, not that it followed it.

### What happens if we do nothing?

Comparisons stay manual and hard to reproduce, no test covers first-session synchronization with a real client, and we have no measure of whether delivered skills are used at all.

## Implementation plan and status

### Step 1: Task format and grading

Add the `evals/` layout, `task.toml` parsing and validation, the `fixture-store` crate, and the two tasks with their starters, hidden tests, and the `store-durability` solution. Add the trusted grading copy, the one-test-per-hidden-test check, and `--check-tasks`, and run it in CI. Test that the solution passes, that extra agent-written unit tests do not change a grade, that an edit to `Cargo.toml` or the fixture crate does not either, and that contradictory expectations are rejected.

- [x] Task format, grading, and `--check-tasks`.

### Step 2: Isolated trial setup

Pin an `srt` version. Build the trial directories, tool staging, the cleared environment, both setup modes with their pre-launch checks, the generated `srt` policy, and the exploratory fallback. Results are written on every path, and each phase has a deadline.

Add a no-model smoke test under the exact generated policy. It launches the real clients and `cargo-agents` with `--version`, builds and tests a project, and fails to read the developer's home (including `cargo-agents` by absolute path), another trial, and `evals/`; in a baseline trial, it also fails to run the staged `cargo-agents`. This is the first thing to build: it decides whether the runner can launch at all.

- [ ] Trial setup, `srt` boundary, and smoke test.

### Step 3: Reproduce and fix the reload bug

In its own issue and pull request: first a test showing that a skill installed by `SessionStart` is invisible to the first prompt on Claude Code, then return `reloadSkills: true` after a successful auto-sync on `SessionStart`, with hook schema tests.

- [ ] Bug reproduced, then fixed.

### Step 4: Claude Code adapter and `formatting`

Add `ClientRunner`, the Claude adapter, the expectation checks, and the evidence cross-checks. Test transcript parsing, client errors, timeouts, and inconclusive results against a stand-in client and recorded transcripts. Then run `formatting` and confirm the skill is installed and loaded in the same session and the `Stop` hook fires. Record the minimum supported Claude Code version.

- [ ] Claude adapter and `formatting`.

### Step 5: Comparisons on Claude Code

Add `--trials`, alternating order, and `benchmark.json`. Test the counting against recorded results, including timeouts, setup failures, exclusions, inconclusive trials, and a configuration with no eligible trials. Then run `store-durability` with five trials and record the results here.

- [ ] Trials, reporting, and `store-durability`.

### Step 6: Codex Stop hook support

Map and register `Stop` for Codex in `src/hook_schema/codex.rs` and `src/agents/mod.rs`, following Claude, with the existing hook schema and registration tests.

- [ ] Codex `Stop` support.

### Step 7: Codex CLI adapter

Add the Codex adapter and its skill-load detection, reusing the tasks, checks, and isolation unchanged. Test parsing and failures without model calls, then run both tasks. Determine whether Codex can load a skill installed during its own `SessionStart`, and record the answer here. Record the minimum supported Codex version. Update the running-tests and testing-guidelines chapters.

- [ ] Codex adapter.
