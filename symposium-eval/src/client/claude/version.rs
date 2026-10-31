//! Which Claude Code releases the adapter supports (research notes, Step 4, client
//! provenance).
//!
//! The staged executable reports its version before any trial launches, so an
//! unsupported release is a setup failure, never a trial's.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use crate::grade::process::{Backend, Invocation, Local};
use crate::run::stage::StagedTool;
use crate::run::{Batch, RunError, succeeded};

/// The oldest release the command line and the recorded transcripts were checked
/// against. Exploratory trials need 2.1.285 or later, the first release where
/// `--settings` keeps the repository's settings from loosening the sandbox; this floor
/// is later.
pub(crate) const MINIMUM: Version = Version {
    major: 2,
    minor: 1,
    patch: 316,
};

/// How long `claude --version` may take.
const PROBE_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    /// Reads `claude --version` output, such as `2.1.316 (Claude Code)`. A prerelease,
    /// such as `2.1.316-beta`, is none: it comes before the release of that number.
    pub(super) fn from_output(output: &str) -> Option<Self> {
        let release = output.split_whitespace().next()?;
        let mut parts = release.split('.').map(|part| part.parse::<u32>().ok());
        let version = Self {
            major: parts.next()??,
            minor: parts.next()??,
            patch: parts.next()??,
        };
        parts.next().is_none().then_some(version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The version of `claude`, staged in `batch`, checked against [`MINIMUM`]. It runs
/// outside any sandbox, with the batch's staged tools on `PATH` as in a trial and
/// `scratch` as its home, so it never reads the developer's configuration. Its logs go
/// to `scratch`.
///
/// # Errors
///
/// When it cannot be run, does not answer in time, fails, or reports a version that is
/// unreadable, a prerelease, or older than [`MINIMUM`].
pub(crate) fn preflight(
    batch: &Batch,
    claude: &StagedTool,
    scratch: &Path,
) -> Result<Version, RunError> {
    let invocation = Invocation {
        program: claude.command().into(),
        args: vec!["--version".into()],
        dir: scratch.to_owned(),
        env: vec![
            ("HOME".into(), scratch.into()),
            ("USERPROFILE".into(), scratch.into()),
            ("PATH".into(), batch.tools_path()?),
        ],
        log_name: "preflight-claude".to_owned(),
    };
    let finished = succeeded("claude", Local.run(&invocation, PROBE_DEADLINE, scratch))?;
    let found = finished.stdout.tail().trim().to_owned();
    match Version::from_output(&found) {
        Some(version) if version >= MINIMUM => Ok(version),
        _ => Err(RunError::ClaudeVersion { found }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    #[test]
    fn a_release_is_read_from_its_version_output() {
        assert_eq!(
            Version::from_output("2.1.316 (Claude Code)"),
            Some(version(2, 1, 316))
        );
    }

    #[test]
    fn output_that_is_not_a_release_has_no_version() {
        let outputs = [
            "",
            "Claude Code",
            "2.1",
            "2.1.x",
            "2.1.316.4",
            "2.1.316-beta",
        ];
        for output in outputs {
            assert_eq!(Version::from_output(output), None, "{output:?}");
        }
    }

    #[test]
    fn releases_compare_by_number_not_by_text() {
        assert!(version(2, 1, 316) > version(2, 1, 99));
        assert!(version(2, 10, 0) > version(2, 9, 999));
    }

    #[test]
    fn the_floor_covers_exploratory_sandboxing() {
        assert!(MINIMUM >= version(2, 1, 285));
    }

    #[cfg(target_os = "linux")]
    mod stand_in {
        use std::fs;

        use super::*;
        use crate::run::stage::{Audience, Tool};
        use crate::run::test_support::{Host, executable};
        use crate::task::ToolName;

        /// Stages `claude` as an npm package whose entry needs `node` from `PATH`, and a
        /// stand-in `node` that prints `output`.
        fn preflight_of(output: &str) -> Result<Version, RunError> {
            let host = Host::new();
            let mut batch = host.batch();
            let node = host.join("node");
            executable(
                &node,
                &format!(
                    "#!/bin/sh
echo '{output}'
"
                ),
            );
            batch
                .stage(name("node"), &Tool::Executable(node), Audience::Common)
                .unwrap();
            let package = host.join("claude-package");
            fs::create_dir_all(package.join("bin")).unwrap();
            executable(
                &package.join("bin/cli.js"),
                "#!/usr/bin/env node
",
            );
            let claude = Tool::NodePackage {
                package,
                entry: "bin/cli.js".into(),
            };
            let claude = batch
                .stage(name("claude"), &claude, Audience::Common)
                .unwrap()
                .clone();
            let scratch = host.join("preflight");
            fs::create_dir_all(&scratch).unwrap();
            preflight(&batch, &claude, &scratch)
        }

        fn name(name: &str) -> ToolName {
            ToolName::new(name).unwrap()
        }

        #[test]
        fn an_npm_client_runs_on_the_staged_node() {
            assert_eq!(
                preflight_of("2.1.316 (Claude Code)").unwrap(),
                version(2, 1, 316)
            );
        }

        #[test]
        fn an_older_release_is_refused() {
            let error = preflight_of("2.1.284 (Claude Code)").unwrap_err();

            assert!(
                matches!(&error, RunError::ClaudeVersion { found } if found == "2.1.284 (Claude Code)"),
                "{error:?}"
            );
        }

        #[test]
        fn a_prerelease_of_the_floor_is_refused() {
            let error = preflight_of("2.1.316-beta (Claude Code)").unwrap_err();

            assert!(matches!(error, RunError::ClaudeVersion { .. }), "{error:?}");
        }

        #[test]
        fn unreadable_output_is_refused() {
            let error = preflight_of("not a version").unwrap_err();

            assert!(matches!(error, RunError::ClaudeVersion { .. }), "{error:?}");
        }
    }
}
