# Agent evaluation tasks

Data for `cargo xtask agent-eval`. Nothing here is Rust workspace code: the root `Cargo.toml` excludes this directory, because starters and fixture crates are copied into trials and built there.

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

Adding an evaluation means adding a directory here. The format and its rules are in [the RFD reference](../md/rfds/skill-evaluation/reference.md#task-format).
Validate every task with `cargo xtask agent-eval --check-tasks`. When a task has a `solution/`, it is graded too.
