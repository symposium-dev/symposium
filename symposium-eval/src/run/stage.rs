//! Staging: the executables trials run, copied into the batch once (RFD reference,
//! "Preparing a trial").
//!
//! A tool is copied into `tools/common/`, which every trial may run, or
//! `tools/symposium/`, which only Symposium trials may. It is copied, not linked: on Linux
//! `srt` matches read rules by the name a file is opened under, so a link into the
//! developer's home would not run. Copying also fixes the binaries for the whole batch.
//! Each trial then gets a link in its `bin/`, the first directory on its `PATH`, for every
//! tool its configuration may run, so a client finds it by name.
//!
//! Tools are staged before any trial is created: a trial links only what is staged by then.
//!
//! FIXME: record each staged tool's hash in the results, once there are results.

use std::fs;
use std::path::{Component, Path, PathBuf};

use super::{Batch, Configuration, RunError, Trial};
use crate::grade::copy_exact;
use crate::task::ToolName;

/// Which trials may run a staged tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Audience {
    /// Every trial: the clients and what they need.
    Common,
    /// Symposium trials only: the `symposium` under evaluation.
    Symposium,
}

/// How a tool is installed on the developer's machine.
#[derive(Debug, Clone)]
pub(crate) enum Tool {
    /// One executable file, such as a native client or `node`.
    Executable(PathBuf),
    /// An npm package directory, copied exactly, and run through `entry`, a regular file
    /// inside it whose `env node` line finds the staged `node` on the trial's `PATH`.
    NodePackage { package: PathBuf, entry: PathBuf },
}

/// A tool in the batch, and the file a trial's `bin/` links to.
#[derive(Debug, Clone)]
pub(crate) struct StagedTool {
    name: ToolName,
    audience: Audience,
    command: PathBuf,
}

impl Audience {
    fn reaches(self, configuration: Configuration) -> bool {
        match self {
            Self::Common => true,
            Self::Symposium => configuration == Configuration::Symposium,
        }
    }
}

impl StagedTool {
    /// The staged file a trial runs.
    pub(crate) fn command(&self) -> &Path {
        &self.command
    }
}

impl Batch {
    /// Copies `tool` into the batch as `name`, for the trials `audience` names.
    ///
    /// # Errors
    ///
    /// When a tool of that name is already staged, a package's entry is not a regular file
    /// inside it, or the tool cannot be copied.
    pub(crate) fn stage(
        &mut self,
        name: ToolName,
        tool: &Tool,
        audience: Audience,
    ) -> Result<&StagedTool, RunError> {
        if self.staged.iter().any(|staged| staged.name == name) {
            return Err(RunError::StagedTwice(name.to_string()));
        }
        let directory = match audience {
            Audience::Common => self.common_tools(),
            Audience::Symposium => self.symposium_tools(),
        };
        let destination = directory.join(name.as_str());
        let command = match tool {
            Tool::Executable(source) => {
                // Copy what a link such as `~/.local/bin/claude` points at.
                let source =
                    fs::canonicalize(source).map_err(|source_error| RunError::Canonicalize {
                        path: source.clone(),
                        source: source_error,
                    })?;
                // `fs::copy` keeps the permission bits, so the copy stays executable.
                fs::copy(&source, &destination).map_err(|source| RunError::Create {
                    path: destination.clone(),
                    source,
                })?;
                destination
            }
            Tool::NodePackage { package, entry } => {
                // A missing or unusable entry would only show when a trial runs it.
                if !is_inside(entry) || !is_regular_file(&package.join(entry)) {
                    return Err(RunError::ToolEntry(entry.clone()));
                }
                copy_exact(package, &destination, "tool").map_err(RunError::Stage)?;
                destination.join(entry)
            }
        };
        self.staged.push(StagedTool {
            name,
            audience,
            command,
        });
        Ok(self.staged.last().expect("just pushed"))
    }

    /// Links every tool `trial` may run into its `bin/`.
    pub(super) fn link_tools(&self, trial: &Trial) -> Result<(), RunError> {
        let reachable = self
            .staged
            .iter()
            .filter(|staged| staged.audience.reaches(trial.configuration()));
        for staged in reachable {
            let link = trial.bin().join(staged.name.as_str());
            symlink(&staged.command, &link)
                .map_err(|source| RunError::Link { path: link, source })?;
        }
        Ok(())
    }
}

/// Whether `entry` names a file inside its package: relative, with no `..`.
fn is_inside(entry: &Path) -> bool {
    entry.components().count() > 0
        && entry
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// Whether `path` is a regular file itself, not a link to one.
fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Trials run under `srt` only on Linux, so a trial elsewhere cannot run staged tools yet.
/// Saying so here keeps a missing link from surfacing later as a missing command.
#[cfg(not(unix))]
fn symlink(_target: &Path, _link: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "staged tools are linked into trials only on Unix",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::test_support::Host;

    fn tool_name(name: &str) -> ToolName {
        ToolName::new(name).unwrap()
    }

    #[test]
    fn an_executable_is_copied_into_its_audiences_directory() {
        let host = Host::new();
        let mut batch = host.batch();
        let source = host.join("installed-tool");
        fs::write(&source, "tool").unwrap();

        let staged = batch
            .stage(
                tool_name("tool"),
                &Tool::Executable(source),
                Audience::Symposium,
            )
            .unwrap()
            .clone();

        assert_eq!(staged.command(), batch.symposium_tools().join("tool"));
        assert_eq!(fs::read_to_string(staged.command()).unwrap(), "tool");
    }

    #[test]
    fn a_package_is_run_through_its_entry() {
        let host = Host::new();
        let mut batch = host.batch();
        let package = host.join("package");
        fs::create_dir_all(package.join("bin")).unwrap();
        fs::write(package.join("bin/cli.js"), "console.log('hi')").unwrap();
        // Grading leaves out a `target/`; staging copies the package as it is.
        fs::create_dir_all(package.join("target")).unwrap();
        fs::write(package.join("target/native.node"), "addon").unwrap();
        let tool = Tool::NodePackage {
            package,
            entry: PathBuf::from("bin/cli.js"),
        };

        let staged = batch
            .stage(tool_name("client"), &tool, Audience::Common)
            .unwrap()
            .clone();

        let expected = batch
            .common_tools()
            .join("client")
            .join("bin")
            .join("cli.js");
        assert_eq!(staged.command(), expected);
        assert!(expected.is_file());
        let copied = batch.common_tools().join("client/target/native.node");
        assert!(copied.is_file());
    }

    #[test]
    fn a_package_entry_must_stay_inside_it() {
        let host = Host::new();
        let mut batch = host.batch();
        let package = host.join("package");
        fs::create_dir_all(&package).unwrap();

        fs::create_dir_all(package.join("lib")).unwrap();
        // Outside the package, missing, a directory, and nothing at all.
        for entry in ["../outside.js", "missing.js", "lib", "", "/abs/cli.js"] {
            let tool = Tool::NodePackage {
                package: package.clone(),
                entry: PathBuf::from(entry),
            };
            let result = batch.stage(tool_name("client"), &tool, Audience::Common);
            assert!(
                matches!(result, Err(RunError::ToolEntry(_))),
                "{entry:?}: {result:?}"
            );
        }
    }

    #[test]
    fn a_name_is_staged_once() {
        let host = Host::new();
        let mut batch = host.batch();
        let source = host.join("installed-tool");
        fs::write(&source, "tool").unwrap();
        let tool = Tool::Executable(source);
        batch
            .stage(tool_name("tool"), &tool, Audience::Common)
            .unwrap();

        let result = batch.stage(tool_name("tool"), &tool, Audience::Symposium);

        assert!(
            matches!(result, Err(RunError::StagedTwice(_))),
            "{result:?}"
        );
    }

    /// Where links are not made yet, a trial that would need one is refused rather than
    /// left without its tools.
    #[cfg(not(unix))]
    #[test]
    fn a_trial_that_cannot_link_its_tools_is_not_created() {
        use crate::run::test_support::store_durability;

        let host = Host::new();
        let mut batch = host.batch();
        let source = host.join("installed-tool");
        fs::write(&source, "tool").unwrap();
        let tool = Tool::Executable(source);
        batch
            .stage(tool_name("client"), &tool, Audience::Common)
            .unwrap();
        let (evals, task) = store_durability();

        let result = batch.create_trial(1, Configuration::Baseline, &evals, &task);

        assert!(matches!(result, Err(RunError::Link { .. })), "{result:?}");
    }

    /// The guarantee behind "without Symposium": a baseline trial's `bin/` has no
    /// `symposium`.
    #[cfg(unix)]
    #[test]
    fn only_a_symposium_trial_links_symposium_tools() {
        use crate::run::test_support::trial;

        let host = Host::new();
        let mut batch = host.batch();
        let source = host.join("installed-tool");
        fs::write(&source, "tool").unwrap();
        let tool = Tool::Executable(source);
        batch
            .stage(tool_name("client"), &tool, Audience::Common)
            .unwrap();
        batch
            .stage(tool_name("symposium"), &tool, Audience::Symposium)
            .unwrap();

        let symposium = trial(&batch, 1, Configuration::Symposium);
        let baseline = trial(&batch, 2, Configuration::Baseline);

        let linked = |trial: &Trial| {
            let mut names: Vec<_> = fs::read_dir(trial.bin())
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        };
        assert_eq!(linked(&symposium), ["client", "symposium"]);
        assert_eq!(linked(&baseline), ["client"]);
        assert_eq!(
            fs::read_link(symposium.bin().join("symposium")).unwrap(),
            batch.symposium_tools().join("symposium")
        );
    }
}
