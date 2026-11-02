//! Trials: the directories one agent session runs in, and the `srt` policy that confines
//! it to them (RFD reference, "Preparing a trial" and "Isolation").
//!
//! Every batch lives under one evaluation root, outside the checkout, since `srt` denies
//! the whole checkout to the agent:
//!
//! ```text
//! <eval-root>/<batch>/
//!   tools/common/        staged clients, readable by every trial
//!   tools/symposium/     staged cargo-agents, readable by Symposium trials only
//!   trial-<n>-<with|without>-symposium/
//! ```
//!
//! Invariant: a trial starts fresh. A batch or trial directory that already exists is an
//! error, never reused, so no state from an earlier attempt reaches the agent. A trial
//! whose setup failed keeps its directory as evidence, so its number is never retried;
//! `srt.json` is written last, and a trial without one never ran.
//!
//! Invariant: the policy denies the evaluation root the trial actually lives in. A
//! [`Batch`] keeps its root both as given and canonical, and a [`Trial`] carries its
//! batch, so the two cannot be paired wrongly.

#[cfg(all(test, target_os = "linux"))]
mod adversarial;
pub(crate) mod client;
mod env;
mod error;
mod exploratory;
mod policy;
mod prelaunch;
pub(crate) mod setup;
#[cfg(all(test, target_os = "linux"))]
mod smoke;
mod srt;
pub(crate) mod stage;
#[cfg(test)]
pub(crate) mod test_support;

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) use env::ApiKey;
pub(crate) use error::{RunError, Unready};
pub(crate) use policy::{Machine, Policy, Reach};
pub(crate) use srt::SRT_VERSION;
use stage::StagedTool;

use crate::evals::Evals;
use crate::grade::ProcessError;
use crate::grade::copy_tree;
use crate::grade::process::Finished;
use crate::task::{BatchName, Task};

/// Whether a trial can reach Symposium. Nothing is installed or uninstalled between the
/// two: a baseline trial simply cannot read the staged `cargo-agents`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Configuration {
    Baseline,
    Symposium,
}

/// One batch directory: the staged tools and every trial of one run of the runner.
#[derive(Debug, Clone)]
pub(crate) struct Batch {
    /// Canonical, so nesting checks compare real directories.
    eval_root: PathBuf,
    /// The evaluation root as given, which `srt` matches separately when it is a link.
    eval_root_as_given: PathBuf,
    root: PathBuf,
    /// The tools staged so far, which trials created from now on link.
    staged: Vec<StagedTool>,
}

/// The directories of one trial. The agent sees only these, the staged tools, and the
/// toolchain.
#[derive(Debug, Clone)]
pub(crate) struct Trial {
    batch: Batch,
    root: PathBuf,
    configuration: Configuration,
}

impl Configuration {
    fn label(self) -> &'static str {
        match self {
            Self::Baseline => "without-symposium",
            Self::Symposium => "with-symposium",
        }
    }
}

impl fmt::Display for Configuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

impl Batch {
    /// Creates `<eval_root>/<name>` and its tool directories. `eval_root` is created if
    /// needed; the batch directory must not exist yet.
    ///
    /// # Errors
    ///
    /// When `eval_root` is relative, the batch already exists, or a directory cannot be
    /// created.
    pub(crate) fn create(eval_root: &Path, name: &BatchName) -> Result<Self, RunError> {
        let eval_root = absolute(eval_root)?;
        let eval_root_as_given = eval_root.to_owned();
        fs::create_dir_all(eval_root).map_err(|source| RunError::Create {
            path: eval_root.to_owned(),
            source,
        })?;
        let eval_root = fs::canonicalize(eval_root).map_err(|source| RunError::Canonicalize {
            path: eval_root.to_owned(),
            source,
        })?;
        let batch = Self {
            root: eval_root.join(name.as_str()),
            eval_root,
            eval_root_as_given,
            staged: Vec::new(),
        };
        create_private(&batch.root)?;
        create_dirs([batch.common_tools(), batch.symposium_tools()])?;
        Ok(batch)
    }

    /// The canonical evaluation root, which holds other batches and their trials.
    pub(crate) fn eval_root(&self) -> &Path {
        &self.eval_root
    }

    /// Every name of the evaluation root: as given, then resolved when that differs.
    pub(crate) fn eval_root_names(&self) -> Vec<PathBuf> {
        let mut names = vec![self.eval_root_as_given.clone()];
        if self.eval_root != self.eval_root_as_given {
            names.push(self.eval_root.clone());
        }
        names
    }

    /// Clients every trial may run.
    pub(crate) fn common_tools(&self) -> PathBuf {
        self.root.join("tools").join("common")
    }

    /// The `cargo-agents` under evaluation, readable by Symposium trials only.
    pub(crate) fn symposium_tools(&self) -> PathBuf {
        self.root.join("tools").join("symposium")
    }

    /// Creates trial `number` of `task`: empty working directories, the starter in
    /// `project/`, the fixtures beside it, and, for Symposium, the task's plugins as the
    /// trial's registry.
    ///
    /// # Errors
    ///
    /// When the trial already exists, a directory cannot be created, or the task cannot
    /// be copied.
    pub(crate) fn create_trial(
        &self,
        number: usize,
        configuration: Configuration,
        evals: &Evals,
        task: &Task,
    ) -> Result<Trial, RunError> {
        let trial = Trial {
            batch: self.clone(),
            root: self.root.join(format!("trial-{number}-{configuration}")),
            configuration,
        };
        create_fresh(&trial.root)?;
        create_dirs([
            trial.home(),
            trial.bin(),
            trial.cargo_home(),
            trial.tmp(),
            trial.evidence(),
            trial.session(),
        ])?;
        self.link_tools(&trial)?;
        copy_tree(&evals.starter_dir(task.name()), &trial.project(), "starter")
            .map_err(RunError::Copy)?;
        // `srt` write-protects `.claude/commands` in the project. Where `.claude/` is
        // missing, it protects all of `.claude/` instead, and no skill could be installed.
        create_dirs([trial.project().join(".claude")])?;
        // Fixtures are not in the policy's writable paths, so `srt` keeps them read-only.
        for fixture in task.fixtures() {
            let destination = trial.fixtures().join(fixture.as_str());
            copy_tree(&evals.fixture_dir(fixture), &destination, "fixture")
                .map_err(RunError::Copy)?;
        }
        if configuration == Configuration::Symposium {
            create_dirs([trial.registry()])?;
            for plugin in task.plugins() {
                let destination = trial.registry().join(plugin.as_str());
                copy_tree(&evals.plugin_dir(plugin), &destination, "plugin")
                    .map_err(RunError::Copy)?;
            }
        }
        Ok(trial)
    }
}

impl Trial {
    pub(crate) fn batch(&self) -> &Batch {
        &self.batch
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn configuration(&self) -> Configuration {
        self.configuration
    }

    /// `HOME` for every process in the trial, so each client starts with fresh state.
    pub(crate) fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    /// The copy of the starter the agent works in.
    pub(crate) fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    /// The task's fixture crates, beside the project so path dependencies resolve.
    pub(crate) fn fixtures(&self) -> PathBuf {
        self.root.join("fixtures")
    }

    /// Links to the staged tools: the first directory on `PATH`.
    pub(crate) fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// `CARGO_HOME`, empty at the start.
    pub(crate) fn cargo_home(&self) -> PathBuf {
        self.root.join("cargo-home")
    }

    /// `TMPDIR`.
    pub(crate) fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// Hook records and other evidence, kept outside the project.
    pub(crate) fn evidence(&self) -> PathBuf {
        self.root.join("evidence")
    }

    /// The client's transcript and logs. The runner writes them from outside the
    /// sandbox, and the policy gives the trial no access, so the agent cannot alter them.
    pub(crate) fn session(&self) -> PathBuf {
        self.root.join("session")
    }

    /// The task's plugins, offered to Symposium as its only registry.
    pub(crate) fn registry(&self) -> PathBuf {
        self.root.join("registry")
    }

    /// Where the trial's `srt` policy is written, and kept with its results.
    pub(crate) fn policy_file(&self) -> PathBuf {
        self.root.join("srt.json")
    }

    /// Writes `policy` to [`Self::policy_file`].
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub(crate) fn write_policy(&self, policy: &Policy) -> Result<(), RunError> {
        policy.write(&self.policy_file())
    }
}

/// `path`, or an error when it is relative: `srt` would resolve a relative path against
/// whatever directory it starts in.
fn absolute(path: &Path) -> Result<&Path, RunError> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(RunError::NotAbsolute(path.to_owned()))
    }
}

/// `finished` when the command `tool` names ran to a successful exit.
pub(crate) fn succeeded(
    tool: &'static str,
    finished: Result<Finished, ProcessError>,
) -> Result<Finished, RunError> {
    let finished = finished.map_err(|source| RunError::Process { tool, source })?;
    let Some(status) = finished.status else {
        return Err(RunError::ToolTimeout { tool });
    };
    if !status.success() {
        return Err(RunError::ToolFailed {
            tool,
            status,
            stderr: finished.stderr.tail().trim().to_owned(),
        });
    }
    Ok(finished)
}

/// Creates `path`, failing when it already exists.
fn create_fresh(path: &Path) -> Result<(), RunError> {
    fs::create_dir(path).map_err(|source| RunError::Create {
        path: path.to_owned(),
        source,
    })
}

/// Like [`create_fresh`], but readable only by its owner on Unix, so other users cannot
/// read the trials beneath it (as Nix creates its build directories).
fn create_private(path: &Path) -> Result<(), RunError> {
    #[cfg_attr(not(unix), expect(unused_mut, reason = "only Unix sets a mode"))]
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path).map_err(|source| RunError::Create {
        path: path.to_owned(),
        source,
    })
}

fn create_dirs(paths: impl IntoIterator<Item = PathBuf>) -> Result<(), RunError> {
    paths.into_iter().try_for_each(|path| {
        fs::create_dir_all(&path).map_err(|source| RunError::Create { path, source })
    })
}

#[cfg(test)]
mod tests {
    use std::io;

    use tempfile::TempDir;

    use super::*;
    use crate::task::TaskName;

    fn store_durability() -> (Evals, Task) {
        let evals = Evals::in_repository();
        let task = Task::load(&evals, &TaskName::new("store-durability").unwrap()).unwrap();
        (evals, task)
    }

    fn batch_name() -> BatchName {
        BatchName::new("batch").unwrap()
    }

    fn already_exists(result: Result<impl fmt::Debug, RunError>) -> bool {
        matches!(
            result,
            Err(RunError::Create { source, .. }) if source.kind() == io::ErrorKind::AlreadyExists
        )
    }

    #[test]
    fn a_symposium_trial_holds_the_task_and_its_registry() {
        let (evals, task) = store_durability();
        let root = TempDir::new().unwrap();
        let batch = Batch::create(root.path(), &batch_name()).unwrap();

        let trial = batch
            .create_trial(1, Configuration::Symposium, &evals, &task)
            .unwrap();

        assert_eq!(
            trial.root(),
            batch.eval_root().join("batch/trial-1-with-symposium")
        );
        assert!(trial.project().join("Cargo.lock").is_file());
        assert!(trial.fixtures().join("fixture-store/src/lib.rs").is_file());
        assert!(
            trial
                .registry()
                .join("fixture-store/Symposium.toml")
                .is_file()
        );
        for dir in [trial.home(), trial.bin(), trial.cargo_home(), trial.tmp()] {
            assert!(dir.is_dir(), "{}", dir.display());
        }
        assert!(batch.symposium_tools().is_dir());
    }

    #[test]
    fn a_baseline_trial_has_no_registry() {
        let (evals, task) = store_durability();
        let root = TempDir::new().unwrap();
        let batch = Batch::create(root.path(), &batch_name()).unwrap();

        let trial = batch
            .create_trial(2, Configuration::Baseline, &evals, &task)
            .unwrap();

        assert_eq!(
            trial.root(),
            batch.eval_root().join("batch/trial-2-without-symposium")
        );
        assert!(trial.project().join("src/lib.rs").is_file());
        assert!(!trial.registry().exists());
    }

    #[test]
    fn an_existing_trial_is_never_reused() {
        let (evals, task) = store_durability();
        let root = TempDir::new().unwrap();
        let batch = Batch::create(root.path(), &batch_name()).unwrap();
        let trial = batch
            .create_trial(1, Configuration::Baseline, &evals, &task)
            .unwrap();
        let sentinel = trial.home().join("sentinel");
        fs::write(&sentinel, "earlier attempt").unwrap();

        let again = batch.create_trial(1, Configuration::Baseline, &evals, &task);

        assert!(already_exists(again));
        assert!(sentinel.is_file());
    }

    #[test]
    fn an_existing_batch_is_never_reused() {
        let root = TempDir::new().unwrap();
        Batch::create(root.path(), &batch_name()).unwrap();

        assert!(already_exists(Batch::create(root.path(), &batch_name())));
    }

    #[cfg(unix)]
    #[test]
    fn only_the_owner_can_enter_a_batch() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().unwrap();
        let batch = Batch::create(root.path(), &batch_name()).unwrap();

        let mode = fs::metadata(&batch.root).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn a_relative_eval_root_is_rejected() {
        let result = Batch::create(Path::new("eval-root"), &batch_name());

        assert!(
            matches!(result, Err(RunError::NotAbsolute(_))),
            "{result:?}"
        );
    }

    #[test]
    fn a_batch_name_cannot_leave_the_eval_root() {
        for name in ["..", "../escape", "/absolute", r"C:\absolute"] {
            assert!(BatchName::new(name).is_err(), "{name}");
        }
    }
}
