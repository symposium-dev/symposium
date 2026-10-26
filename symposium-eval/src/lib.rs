//! Agent evaluations: run a real agent client on a small Rust task, with or
//! without Symposium, then grade the code it leaves behind and check what
//! Symposium did along the way.
//!
//! This crate is the engine behind `cargo xtask agent-eval`. The approved design
//! is `md/rfds/skill-evaluation/`; `md/design/agent-eval.md` maps it onto this
//! crate's modules.

mod evals;
mod grade;
mod task;
mod tree;

use std::ffi::OsString;

use anyhow::{Context, bail};
use clap::Parser;

use crate::evals::{Evals, TaskEntry};
use crate::task::{Task, TaskError, TaskErrors};

#[derive(Debug, Parser)]
#[command(
    name = "agent-eval",
    bin_name = "cargo xtask agent-eval",
    about = "Evaluate agent tasks with and without Symposium"
)]
struct Cli {
    /// Validate every task under evals/ without running an agent
    #[arg(long)]
    check_tasks: bool,
}

/// `args` is everything after `agent-eval`. Clap handles `--help` and invalid
/// arguments by printing and exiting the process.
///
/// # Errors
///
/// When the runner itself fails, or `--check-tasks` finds an invalid task.
/// Whatever happens inside a trial is an outcome, not an error.
pub fn run<I, T>(args: I) -> anyhow::Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let program = OsString::from("agent-eval");
    let cli = Cli::parse_from(std::iter::once(program).chain(args.into_iter().map(Into::into)));
    if cli.check_tasks {
        return check_tasks(&Evals::in_repository());
    }
    bail!("running a task is not implemented yet; use --check-tasks")
}

fn check_tasks(evals: &Evals) -> anyhow::Result<()> {
    let entries = evals
        .task_entries()
        .with_context(|| format!("cannot list `{}`", evals.tasks_dir().display()))?;
    let total = entries.len();
    let failed = entries
        .into_iter()
        .map(|entry| check_entry(evals, entry))
        .filter(|passed| !passed)
        .count();
    println!("checked {}", plural(total, "task"));
    if failed > 0 {
        bail!("{failed} of {} failed validation", plural(total, "task"));
    }
    Ok(())
}

/// Validates one entry of `tasks/` and prints its result; `true` when it passed.
fn check_entry(evals: &Evals, entry: TaskEntry) -> bool {
    let outcome = match entry.task {
        Ok(name) => Task::load(evals, &name).and_then(|task| grade::check_solution(evals, &task)),
        Err(error) => Err(TaskErrors::from(TaskError::from(error))),
    };
    match outcome {
        Ok(()) => {
            println!("ok    {}", entry.entry);
            true
        }
        Err(errors) => {
            println!("FAIL  {}", entry.entry);
            for line in errors.to_string().lines() {
                println!("      {line}");
            }
            false
        }
    }
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }
}
