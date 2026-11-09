//! Agent evaluations: run a real agent client on a small Rust task, with or
//! without Symposium, then grade the code it leaves behind and check what
//! Symposium did along the way.
//!
//! This crate is the engine behind `cargo xtask agent-eval`. The approved design
//! is `md/rfds/skill-evaluation/`; `md/design/agent-eval.md` maps it onto this
//! crate's modules.

mod client;
mod command;
mod digest;
mod evals;
mod expect;
mod grade;
mod jsonl;
mod outcome;
mod result;
mod run;
mod skill_md;
mod task;
mod tree;

use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Parser, ValueEnum};

use crate::command::{Checkout, Request};
use crate::evals::{Evals, TaskEntry};
use crate::task::{Task, TaskError, TaskErrors, TaskName};

#[derive(Debug, Parser)]
#[command(
    name = "agent-eval",
    bin_name = "cargo xtask agent-eval",
    about = "Evaluate agent tasks with and without Symposium"
)]
struct Cli {
    /// Validate every task under evals/ without running an agent
    #[arg(long, conflicts_with = "task")]
    check_tasks: bool,

    /// The task to run, a directory under evals/tasks/
    #[arg(required_unless_present = "check_tasks", requires_all = ["client", "model"])]
    task: Option<String>,

    /// The client to run the task on
    #[arg(long, value_enum)]
    client: Option<ClientArg>,

    /// The model to ask the client for, as the client names it
    #[arg(long)]
    model: Option<String>,

    /// Evaluate this cargo-agents instead of building one from the checkout
    #[arg(long, value_name = "PATH")]
    symposium_bin: Option<PathBuf>,

    /// Where batches run, outside the checkout [default: the system's temporary directory]
    #[arg(long, value_name = "DIR")]
    eval_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ClientArg {
    Claude,
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
    let request = request(cli)?;
    command::run(&request, &Checkout::in_repository(), |name| {
        std::env::var_os(name)
    })
}

/// What `cli` asks to run. Clap has already required a task, client, and model.
fn request(cli: Cli) -> anyhow::Result<Request> {
    let (Some(task), Some(ClientArg::Claude), Some(model)) = (cli.task, cli.client, cli.model)
    else {
        bail!("a task needs --client and --model");
    };
    Ok(Request {
        task: TaskName::new(task)?,
        model,
        symposium_bin: cli.symposium_bin,
        eval_root: cli.eval_root,
    })
}

fn check_tasks(evals: &Evals) -> anyhow::Result<()> {
    let entries = evals
        .task_entries()
        .with_context(|| format!("cannot list `{}`", evals.tasks_dir().display()))?;
    if entries.is_empty() {
        bail!("`{}` holds no tasks", evals.tasks_dir().display());
    }
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
    use std::fs;

    use clap::CommandFactory;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    fn parses(args: &[&str]) -> bool {
        Cli::try_parse_from(std::iter::once("agent-eval").chain(args.iter().copied())).is_ok()
    }

    #[test]
    fn a_task_needs_a_client_and_a_model() {
        assert!(!parses(&[]));
        assert!(!parses(&["formatting"]));
        assert!(!parses(&["formatting", "--client", "claude"]));
        assert!(!parses(&["formatting", "--model", "haiku"]));
        assert!(parses(&[
            "formatting",
            "--client",
            "claude",
            "--model",
            "haiku"
        ]));
    }

    #[test]
    fn only_a_client_with_an_adapter_is_accepted() {
        assert!(!parses(&[
            "formatting",
            "--client",
            "codex",
            "--model",
            "m"
        ]));
    }

    #[test]
    fn checking_tasks_runs_none() {
        assert!(parses(&["--check-tasks"]));
        assert!(!parses(&[
            "--check-tasks",
            "formatting",
            "--client",
            "claude",
            "--model",
            "m"
        ]));
    }

    #[test]
    fn checking_no_tasks_is_an_error() {
        let root = TempDir::new().unwrap();
        let evals = Evals::new(root.path());
        assert!(check_tasks(&evals).is_err(), "without `tasks/`");

        fs::create_dir(evals.tasks_dir()).unwrap();
        assert!(check_tasks(&evals).is_err(), "with an empty `tasks/`");
    }
}
