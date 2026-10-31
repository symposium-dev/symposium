//! The environment every process in a trial starts with (RFD reference, "Preparing a
//! trial").
//!
//! Nothing is inherited from the runner. `PATH` holds the trial's `bin/`, then the
//! toolchain's own `bin/`, then the system directories, and never the developer's
//! `~/.cargo/bin`, which can hold their own `cargo-agents`.

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

use super::{Batch, Machine, RunError, Trial};

/// Where the linker, `sh`, and the tools the clients need live.
const SYSTEM_PATH: [&str; 2] = ["/usr/bin", "/bin"];

/// The model API key. It reaches `srt`, never the sandbox: the policy masks it.
#[derive(Clone)]
pub(crate) struct ApiKey {
    /// The variable the client reads: `ANTHROPIC_API_KEY` for Claude Code,
    /// `OPENAI_API_KEY` for Codex.
    name: &'static str,
    value: OsString,
}

impl ApiKey {
    pub(crate) fn new(name: &'static str, value: OsString) -> Self {
        Self { name, value }
    }

    pub(crate) fn name(&self) -> &'static str {
        self.name
    }
}

/// Never prints the key, so it cannot reach a log or a result.
impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiKey")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Batch {
    /// `PATH` for running the batch's clients outside a trial, as preflight does: the
    /// staged clients first, so an npm entry's `env node` finds the staged Node, as it
    /// does in a trial.
    ///
    /// # Errors
    ///
    /// When the tools directory cannot be joined, such as one containing `:`.
    pub(crate) fn tools_path(&self) -> Result<OsString, RunError> {
        let path = std::iter::once(self.common_tools()).chain(SYSTEM_PATH.map(PathBuf::from));
        env::join_paths(path).map_err(RunError::PathList)
    }
}

impl Trial {
    /// The whole environment for a process in this trial.
    ///
    /// # Errors
    ///
    /// When a `PATH` entry cannot be joined, such as one containing `:`.
    pub(crate) fn environment(
        &self,
        machine: &Machine,
        api_key: &ApiKey,
    ) -> Result<Vec<(OsString, OsString)>, RunError> {
        let path = [self.bin(), machine.toolchain_bin()]
            .into_iter()
            .chain(SYSTEM_PATH.map(PathBuf::from));
        let path = env::join_paths(path).map_err(RunError::PathList)?;
        let hook_trace = self.evidence().join("hook-trace.jsonl");
        Ok(vec![
            ("HOME".into(), self.home().into()),
            ("PATH".into(), path),
            ("TMPDIR".into(), self.tmp().into()),
            ("CARGO_HOME".into(), self.cargo_home().into()),
            ("EVAL_EVIDENCE_DIR".into(), self.evidence().into()),
            ("SYMPOSIUM_HOOK_TRACE".into(), hook_trace.into()),
            (api_key.name.into(), api_key.value.clone()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grade::process::Invocation;
    use crate::run::Configuration;
    use crate::run::test_support::{Host, trial};

    #[test]
    fn a_trial_starts_with_only_its_own_environment() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);
        let api_key = ApiKey::new("EXAMPLE_API_KEY", "sk-test".into());

        let environment = trial.environment(&host.machine(), &api_key).unwrap();

        let names: Vec<_> = environment.iter().map(|(name, _)| name.clone()).collect();
        assert_eq!(
            names,
            [
                "HOME",
                "PATH",
                "TMPDIR",
                "CARGO_HOME",
                "EVAL_EVIDENCE_DIR",
                "SYMPOSIUM_HOOK_TRACE",
                "EXAMPLE_API_KEY",
            ]
        );
        let value = |name: &str| {
            environment
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
                .unwrap()
        };
        assert_eq!(value("HOME"), trial.home().as_os_str());
        assert_eq!(value("EXAMPLE_API_KEY"), "sk-test");
        assert_eq!(
            value("SYMPOSIUM_HOOK_TRACE"),
            trial.evidence().join("hook-trace.jsonl").as_os_str()
        );
    }

    #[test]
    fn path_holds_the_trial_then_the_toolchain_then_the_system() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Baseline);
        let machine = host.machine();
        let api_key = ApiKey::new("EXAMPLE_API_KEY", "sk-test".into());

        let environment = trial.environment(&machine, &api_key).unwrap();

        let (_, path) = environment.iter().find(|(name, _)| name == "PATH").unwrap();
        let entries: Vec<PathBuf> = env::split_paths(path).collect();
        assert_eq!(
            entries,
            [
                trial.bin(),
                machine.toolchain_bin(),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ]
        );
    }

    #[test]
    fn the_api_key_never_prints() {
        let api_key = ApiKey::new("EXAMPLE_API_KEY", "sk-secret".into());

        let printed = format!("{api_key:?}");

        assert!(!printed.contains("sk-secret"), "{printed}");
        assert!(printed.contains("EXAMPLE_API_KEY"), "{printed}");
    }

    /// The key leaves `ApiKey` as a plain value, so the command carrying it must hide it
    /// too.
    #[test]
    fn the_api_key_never_prints_from_the_command_that_carries_it() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Baseline);
        let api_key = ApiKey::new("EXAMPLE_API_KEY", "sk-secret".into());
        let invocation = Invocation {
            program: "sh".into(),
            args: Vec::new(),
            dir: trial.project(),
            env: trial.environment(&host.machine(), &api_key).unwrap(),
            log_name: "probe".to_owned(),
        };

        let printed = format!("{invocation:?}");

        assert!(!printed.contains("sk-secret"), "{printed}");
        assert!(printed.contains("EXAMPLE_API_KEY"), "{printed}");
    }
}
