# Agent evaluations

`cargo xtask agent-eval` runs a real Claude Code or Codex CLI session on a small Rust task inside `srt`, with or without Symposium, then grades the code the agent left behind and checks what Symposium did along the way. The approved design and its precise rules are in the [skill evaluation RFD](../rfds/skill-evaluation/README.md) and its [reference](../rfds/skill-evaluation/reference.md). This chapter records how that design maps onto the repository.

The runner calls a model, so it never runs in CI. Its model-free parts do: task validation (`--check-tasks`) and the crate's tests.

## Where things live

| Path | Holds |
| --- | --- |
| `symposium-eval/` | The engine, as a library crate (`publish = false`). |
| `xtask/` | The `cargo xtask agent-eval` entry point. It forwards its arguments to `symposium_eval::run` and holds no evaluation logic. |
| `evals/` | Tasks, plugins, and fixture crates. Data only, excluded from the Cargo workspace. |
| `results/` | Retained batches, one directory per run. Machine-local and ignored by git. |

Trial run directories are created outside the checkout. `srt` denies the whole checkout to the agent, since it holds `evals/` and the hidden tests, so a trial cannot live inside it. Only finished results are copied back to `results/`.

The RFD places the runner in `xtask`. The command is unchanged, but the code lives in its own crate so that its tests run with `cargo test -p symposium-eval` and later evaluations can build on a library rather than on a grab-bag binary.

## Modules

Each module arrives with the RFD step that needs it:

| Module | Responsibility | RFD step |
| --- | --- | --- |
| `task` | `task.toml`, the one-test-per-`checks/`-file rule, `--check-tasks` | 1 |
| `run` | `RunDir`, `RunEnv`, the generated `srt` policy, process-tree cleanup | 2 |
| `client` | `ClientRunner` and the Claude and Codex adapters | 4, 7 |
| `grade` | The grading copy and its checks | 1 |
| `expect` | Skill, hook, and sync expectations | 4 |
| `report` | `result.json`, the batch manifest, `benchmark.json` | 4, 5 |

## Task parsing

`cargo xtask agent-eval --check-tasks` validates every entry of `evals/tasks/` without running an agent. It is the RFD's task-authoring gate and runs in CI, needing no model or API key.

`task.toml` is parsed in two layers, following Cargo's `Cargo.toml` handling. `task::schema` mirrors the file, with `Option` where "absent" and "empty" mean different things (`used`). `Task` is the validated form: names and patterns are checked while deserializing, and the cross-field rules run afterwards, collected into one `TaskErrors` so every problem is reported at once. Unlike Cargo, unknown keys are errors: Cargo warns because it promises compatibility across versions, while a misspelled key here would silently make no claim.

- **Names.** Task, plugin, fixture, skill, and check names become directory or file names, so `DirName<marker::…>` accepts only one portable path component: no separators or Windows-forbidden characters, no control characters, no trailing dot or space, and none of the device names Windows reserves (the list Cargo rejects for package names). Hook names never touch the filesystem, so `HookName` only requires text.
- **Editable patterns** are compiled with `globset` when the task loads, with `*` never crossing `/`, so an unusable pattern is an authoring error rather than a grading failure. Each pattern starts with a literal directory or file name, so `**` and `*.toml` are rejected.
- **Protected files.** `Cargo.toml` and `Cargo.lock` at any depth, anything under a `.cargo` directory, `rust-toolchain` files, and every `build.rs` change how Cargo builds the crate or run code while it builds, so grading always takes them from the starter (`task::protected`). An `editable` pattern that matches one in the starter is rejected, and the starter must ship a `Cargo.lock`, since grading runs Cargo with `--locked`.
- **Hidden checks.** `checks/` must hold at least one `.rs` file, and each file exactly one top-level, plain `#[test]` function that is synchronous, takes no parameters, and has no generics. The rule is proved on the source with `syn`, since a check cannot compile against a starter that lacks the API it calls. The accepted syntax is closed: anything that could add, remove, or hide a test is rejected rather than interpreted, including `cfg` and `cfg_attr` anywhere, `#[ignore]`, attributes other than a short list of inert built-ins, derives outside the standard library, item macros at any depth, out-of-line modules, and tests inside modules.
- **Directories.** Task, `starter/`, plugin, fixture, and `checks/` directories must be real directories; a path that cannot be inspected is reported as an I/O error with its cause, separately from a structural problem. Unlike skill installation, which follows symlinks, a symlink is rejected here: these contents cross into trials and grading, which must see exactly what the checkout holds. Every entry of `tasks/` is reported, including stray files and names that are not UTF-8.

The crate exports only `run`; everything else is `pub(crate)` until another crate needs it. `Evals` holds the `evals/` layout so no other code joins those paths itself.

## Grading

Grading never runs a submission in place. `grade` (in `grade/mod.rs`) builds a **grading copy** in a private temporary directory, laid out as in a trial (`project/` beside `fixtures/<name>/`, so path dependencies resolve): a fresh copy of the starter and fixtures, with only the submission's editable files taken from it.

- Within `editable`, the starter's files are removed before the submission's are copied in, and so is each editable directory that leaves empty, so a file the agent deleted stays deleted and a directory can become a file or the reverse. A submitted file whose place is still held by trusted content (a protected file inside a directory it replaces, or the same name in another case on a case-insensitive filesystem) is not imported and is reported instead.
- The submission is listed recursively without following links (`tree::walk`). Files and directories are both imported, so an empty directory survives grading. A link or special file at, inside, or above an editable root (a pattern's literal prefix, such as `src`) invalidates the submission; anywhere else it is only reported.
- Protected files are never imported, even when `editable` matches them. Their names compare without regard to ASCII case on every platform.
- Every other change is reported as an outside edit (added, modified, deleted, or type changed), sorted by path; a directory with contents is reported through them, and an empty one by itself; files are compared in fixed-size chunks, so a large one costs time, not memory. The top-level `target/` is ignored, since the agent runs Cargo in its project.
- The starter and fixtures are trusted, but a link or special file in them is still a task error rather than something to copy.

Commands run through a `grade::process::Backend`: grading decides which command runs and what its result means, and the backend decides how the process starts and stops, so the `srt` backend can replace `Local` without changing grading policy. `Local` contains the whole process tree: on Unix a process group, killed at the deadline and again once the command exits; on Windows a kill-on-close job object the command is assigned to as it starts, closed on every path. A guard owns each command from the moment it starts, so every return path stops and reaps it, and no wait is unbounded. Full output streams to log files while a bounded tail stays in memory, and a failure to start, contain, stop, or collect a command is an infrastructure error. `GradeLimits` caps each command and all of grading's commands together: a command gets the smaller of its cap and the budget left. File work (copying, listing, comparing) is not counted; it gets entry-count and size quotas when grading accepts untrusted submissions.

Cargo runs in the grading copy with `--locked --offline`, a private target directory and `CARGO_HOME`, and incremental compilation off. Its environment is built, not inherited: only what the toolchain (including the `RUSTUP_*` pair), the linker, and temporary files need is passed through, so no unapproved Cargo or Rust override reaches a grade. `$CARGO`, with `rustc` and `rustdoc` from beside it, keeps grading on the toolchain running `cargo xtask`. A known gap remains: Cargo also reads `.cargo/config.toml` in every directory above the grading copy, which can include the developer's home; the `srt` backend's controlled run directories close it. Before anything is imported, `cargo metadata` runs on the trusted starter: it checks the lockfile, and the build scripts it reports at custom paths (`package.build`) become protected for the import. Cargo exits with the same code for a broken lockfile as for a compile error, so this preflight is what tells the task's failures from the submission's.

Invariant: only repository-owned code runs outside the `srt` backend. Until grading runs through `srt`, `--check-tasks` is the only caller: it lays each task's `solution/` over a copy of the starter (an entry of the other type is replaced, so a solution can turn a file into a directory or the reverse) and imports it like a submission, so a solution cannot pass by changing a file an agent may not contribute. A solution with a link, or with any outside edit, is a task error.

## Code comments

Comments answer why, an invariant, or a surprising behavior of a client or tool. They never restate what the code says.

- **Module docs (`//!`).** One summary sentence, then only what the code cannot show: the mechanism, the decision behind it, and links to the RFD rule it implements. Cargo's `fingerprint` module is the model, kept short.
- **Invariants.** Write a rule the code must keep as `Invariant:` in the module doc, and back each one with a `#[test]`, as rust-analyzer does with its architecture invariants. Example: the baseline can never reach `cargo-agents`.
- **Item docs (`///`).** One or two lines of purpose, plus an `# Errors` section on fallible public functions. Skip the doc when it would only repeat the item's name.
- **Inline comments (`//`).** Proper sentences, starting with a capital and ending with a period (rust-analyzer). Detail is welcome for a non-obvious client quirk, such as Codex appending a non-empty stdin to the prompt (rustc dev guide).
- **Error and context messages.** Lowercase, with no trailing period (rust-analyzer, Rust API guidelines).
- **Markers.** `FIXME:` for a known gap that may be merged. `TODO` only inside an unfinished PR, never merged (rustc's tidy rejects it). `SAFETY:` above any `unsafe` block (salsa).
- **Tests.** The test name states the behavior. Comment only a fixture whose setup is not obvious.

## Precedents

- **rustc's compiletest** treats tests as data: each test is a directory or file the runner discovers, so adding a test needs no code change. `evals/tasks/` follows the same idea, and `--check-tasks` plays the role of compiletest's directive validation.
- **The `cargo xtask` convention** keeps repository automation as a workspace binary invoked through a Cargo alias, with no extra tooling to install. This repository already uses it for `md-orphan-check`.
- **Cargo's own CLI test** calls clap's `Command::debug_assert` in a unit test, so a malformed argument definition fails `cargo test` instead of the first person to run the command. `symposium-eval` does the same.
