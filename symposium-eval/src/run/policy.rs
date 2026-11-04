//! The `srt` policy for one trial (RFD reference, "Enforced: `srt`").
//!
//! `srt` reads are deny-then-allow: everything is readable unless a `denyRead` region
//! covers it, and `allowRead` reopens paths inside one, unless a more specific `denyRead`
//! closes them again. Writes and network are denied unless allowed. [`Policy::can_read`]
//! and [`Policy::can_write`] model those rules, so the isolation invariants are tested
//! as rules rather than as JSON (after Codex's `WritableRoot::is_path_writable`).
//!
//! Every entry is written as `{ "path": ..., "literal": true }`, so `srt` never reads
//! `*`, `?`, or `[` in a path as a pattern. On Linux `srt` matches a read rule against the
//! name a path is reached by, not its target, so each denied root is listed both as given
//! and as resolved.
//!
//! The model API key is masked: inside the sandbox its variable holds a sentinel, and
//! `srt`'s proxy, outside it, substitutes the real value only into requests to the API
//! host, over TLS it terminates and re-encrypts to a verified upstream. So neither the
//! client nor any tool the agent starts can read the key, from its environment or
//! anyone's `/proc/<pid>/environ`.
//!
//! The policy cannot keep trials out of each other's temporary files: `srt` lets every
//! sandbox write `/tmp/claude`, and a `denyRead` of it does not hide what is already
//! there (sandbox-runtime issues 654 and 611). The launcher in `srt.rs` does that instead.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{ApiKey, Configuration, RunError, Trial, absolute};

/// The settings file `srt --settings` reads.
#[derive(Debug, Serialize)]
pub(crate) struct Policy {
    filesystem: Filesystem,
    network: Network,
    credentials: Credentials,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Filesystem {
    deny_read: Vec<Literal>,
    allow_read: Vec<Literal>,
    allow_write: Vec<Literal>,
    /// Always empty, but `srt` refuses a settings file without it.
    deny_write: Vec<Literal>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Network {
    allowed_domains: Vec<String>,
    /// Always empty, but `srt` refuses a settings file without it.
    denied_domains: Vec<String>,
    /// Required by masking: the proxy decrypts requests to substitute the key.
    tls_terminate: TlsTerminate,
}

/// `srt` makes a CA for the run and points the sandboxed command's trust variables at it.
#[derive(Debug, Serialize)]
struct TlsTerminate {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Credentials {
    env_vars: Vec<MaskedVar>,
}

/// A variable whose real value never enters the sandbox.
#[derive(Debug, Serialize)]
struct MaskedVar {
    name: &'static str,
    mode: &'static str,
}

/// One path `srt` takes as a name, never a pattern.
#[derive(Debug, Serialize)]
struct Literal {
    path: PathBuf,
    literal: bool,
}

/// What the policy needs to know about the machine running the batch.
#[derive(Debug, Clone)]
pub(crate) struct Machine {
    /// Every user's home and the usual mount points, each as given and, where it
    /// resolves elsewhere, as resolved.
    user_data_roots: Vec<PathBuf>,
    /// The Symposium checkout, which holds `evals/`: hidden checks, solutions, and other
    /// tasks' plugins. Canonical.
    checkout: PathBuf,
    /// The checkout as given, when that is another name for it.
    checkout_as_given: PathBuf,
    /// The active Rust toolchain's sysroot, which usually sits under a denied home.
    /// Canonical.
    toolchain: PathBuf,
}

impl Machine {
    /// Denies the platform's user data roots and the current user's home, plus
    /// `extra_user_data_roots`, which can add to those but never replace them.
    /// `toolchain` is a sysroot, as `rustc --print sysroot` reports it.
    ///
    /// # Errors
    ///
    /// When a path is relative, the checkout or toolchain does not exist, the toolchain
    /// is not a sysroot, or it lies inside the checkout.
    pub(crate) fn new(
        extra_user_data_roots: Vec<PathBuf>,
        checkout: &Path,
        toolchain: &Path,
    ) -> Result<Self, RunError> {
        let roots = user_data_roots(
            platform_user_data_roots(),
            std::env::home_dir(),
            extra_user_data_roots,
        );
        Self::with_exact_roots(roots, checkout, toolchain)
    }

    /// A machine whose only user data roots are `user_data_roots`, so tests can predict
    /// the policy exactly.
    fn with_exact_roots(
        user_data_roots: Vec<PathBuf>,
        checkout: &Path,
        toolchain: &Path,
    ) -> Result<Self, RunError> {
        let checkout_as_given = absolute(checkout)?.to_owned();
        let checkout = canonical(checkout)?;
        let toolchain = canonical(toolchain)?;
        if !is_sysroot(&toolchain) {
            return Err(RunError::NotToolchain(toolchain));
        }
        reject_nested(&toolchain, &checkout)?;

        let mut names = Vec::with_capacity(user_data_roots.len());
        for root in user_data_roots {
            absolute(&root)?;
            // A root that cannot be resolved is still denied by the name it was given.
            let resolved = fs::canonicalize(&root).ok();
            for name in [Some(root), resolved].into_iter().flatten() {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        Ok(Self {
            user_data_roots: names,
            checkout,
            checkout_as_given,
            toolchain,
        })
    }
}

impl Machine {
    /// [`Self::with_exact_roots`] for the other `run` tests; production code goes through
    /// [`Self::new`].
    #[cfg(test)]
    pub(super) fn for_tests(
        user_data_roots: Vec<PathBuf>,
        checkout: &Path,
        toolchain: &Path,
    ) -> Self {
        Self::with_exact_roots(user_data_roots, checkout, toolchain).unwrap()
    }

    /// The toolchain's own `bin/`, which holds only Rust tools.
    pub(crate) fn toolchain_bin(&self) -> PathBuf {
        self.toolchain.join("bin")
    }
}

/// The platform's roots, then the current user's home, then the caller's. The home is
/// listed even when a platform root covers it: it can sit anywhere, such as
/// `/var/lib/<service>`.
fn user_data_roots(
    platform: Vec<PathBuf>,
    home: Option<PathBuf>,
    extra: Vec<PathBuf>,
) -> Vec<PathBuf> {
    platform.into_iter().chain(home).chain(extra).collect()
}

/// Every user's home and the usual mount points. The runner uses `srt` only on macOS and
/// Linux, so elsewhere only the current home is denied.
fn platform_user_data_roots() -> Vec<PathBuf> {
    let roots: &[&str] = if cfg!(target_os = "macos") {
        &["/Users", "/Volumes", "/var/root"]
    } else if cfg!(unix) {
        &["/home", "/root", "/mnt", "/media", "/run/media"]
    } else {
        &[]
    };
    roots.iter().map(PathBuf::from).collect()
}

/// What a trial may read and write under `srt`, and in exploratory mode under Claude
/// Code's sandbox, which is built on `srt` and reads the same rules. Codex's sandbox
/// cannot express it.
#[derive(Debug, Clone)]
pub(crate) struct Reach {
    pub(crate) deny_read: Vec<PathBuf>,
    pub(crate) allow_read: Vec<PathBuf>,
    pub(crate) allow_write: Vec<PathBuf>,
}

impl Reach {
    /// Confines `trial` to its own directories, the tools its configuration may run, and
    /// the toolchain.
    ///
    /// # Errors
    ///
    /// When the evaluation root lies inside the checkout, or the toolchain inside the
    /// evaluation root: either would let one rule silently undo another.
    pub(crate) fn for_trial(trial: &Trial, machine: &Machine) -> Result<Self, RunError> {
        let batch = trial.batch();
        reject_nested(batch.eval_root(), &machine.checkout)?;
        reject_nested(&machine.toolchain, batch.eval_root())?;

        let mut deny_read = machine.user_data_roots.clone();
        for name in batch
            .eval_root_names()
            .into_iter()
            .chain([machine.checkout_as_given.clone(), machine.checkout.clone()])
        {
            if !deny_read.contains(&name) {
                deny_read.push(name);
            }
        }

        // Only the trial's read-only directories: srt 0.0.81 rebinds an `allowRead` read-only
        // over the writable directories inside it, and then cannot create its own guard
        // files there. The writable directories are readable through `allowWrite`.
        let mut allow_read = vec![
            trial.fixtures(),
            trial.bin(),
            batch.common_tools(),
            machine.toolchain.clone(),
        ];
        // The one difference between the configurations: a baseline cannot read Symposium.
        if trial.configuration() == Configuration::Symposium {
            allow_read.extend([trial.registry(), batch.symposium_tools()]);
        }

        let allow_write = vec![
            trial.project(),
            trial.home(),
            trial.cargo_home(),
            trial.tmp(),
            trial.evidence(),
        ];
        Ok(Self {
            deny_read,
            allow_read,
            allow_write,
        })
    }
}

impl Policy {
    /// [`Reach::for_trial`], with network access to `api_host` only, where `srt` sends
    /// the real `api_key` in place of the sentinel the sandbox sees.
    ///
    /// # Errors
    ///
    /// As [`Reach::for_trial`].
    pub(crate) fn for_trial(
        trial: &Trial,
        machine: &Machine,
        api_host: &str,
        api_key: &ApiKey,
    ) -> Result<Self, RunError> {
        let reach = Reach::for_trial(trial, machine)?;
        Ok(Self {
            filesystem: Filesystem {
                deny_read: literals(reach.deny_read),
                allow_read: literals(reach.allow_read),
                allow_write: literals(reach.allow_write),
                deny_write: Vec::new(),
            },
            network: Network {
                allowed_domains: vec![api_host.to_owned()],
                denied_domains: Vec::new(),
                tls_terminate: TlsTerminate {},
            },
            credentials: Credentials {
                env_vars: vec![MaskedVar {
                    name: api_key.name(),
                    mode: "mask",
                }],
            },
        })
    }

    /// A policy for preflight's probe: it reads like any process outside a trial, writes
    /// only `scratch`, and starts the same network proxy, TLS termination included, as a
    /// trial's.
    pub(crate) fn for_preflight(scratch: &Path) -> Self {
        Self {
            filesystem: Filesystem {
                deny_read: Vec::new(),
                allow_read: Vec::new(),
                allow_write: literals(vec![scratch.to_owned()]),
                deny_write: Vec::new(),
            },
            network: Network {
                allowed_domains: vec!["api.example.com".to_owned()],
                denied_domains: Vec::new(),
                tls_terminate: TlsTerminate {},
            },
            credentials: Credentials {
                env_vars: Vec::new(),
            },
        }
    }

    /// Writes the settings file `srt --settings` reads.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub(crate) fn write(&self, path: &Path) -> Result<(), RunError> {
        let text = serde_json::to_string_pretty(self).map_err(RunError::Policy)?;
        fs::write(path, text).map_err(|source| RunError::Create {
            path: path.to_owned(),
            source,
        })
    }

    /// Whether `srt` lets the trial read `path`: no `denyRead` covers it, or the most
    /// specific `allowRead` or `allowWrite` covering it is at least as specific as every
    /// such deny. `srt` binds a writable directory read-write, so it is readable too.
    pub(crate) fn can_read(&self, path: &Path) -> bool {
        let filesystem = &self.filesystem;
        match most_specific(&filesystem.deny_read, path) {
            None => true,
            Some(deny) => most_specific(&filesystem.allow_read, path)
                .max(most_specific(&filesystem.allow_write, path))
                .is_some_and(|allow| allow >= deny),
        }
    }

    /// Whether `srt` lets the trial write `path`. `srt`'s own rules are not modelled: it
    /// always allows writing its shared temporary directory and never `.git/hooks` or
    /// shell startup files.
    pub(crate) fn can_write(&self, path: &Path) -> bool {
        most_specific(&self.filesystem.allow_write, path).is_some()
    }
}

fn literals(paths: Vec<PathBuf>) -> Vec<Literal> {
    paths
        .into_iter()
        .map(|path| Literal {
            path,
            literal: true,
        })
        .collect()
}

/// The depth of the deepest entry that is `path` or one of its ancestors.
fn most_specific(entries: &[Literal], path: &Path) -> Option<usize> {
    entries
        .iter()
        .filter(|entry| path.starts_with(&entry.path))
        .map(|entry| entry.path.components().count())
        .max()
}

/// `path`, absolute and with links resolved. It must exist: on Linux, `srt` ignores a
/// `denyRead` for a path that does not.
fn canonical(path: &Path) -> Result<PathBuf, RunError> {
    fs::canonicalize(absolute(path)?).map_err(|source| RunError::Canonicalize {
        path: path.to_owned(),
        source,
    })
}

/// Whether `path` holds a toolchain, rather than, say, the `~/.cargo` above one, which
/// would reopen the credentials and binaries beside it.
fn is_sysroot(path: &Path) -> bool {
    let cargo = format!("cargo{}", std::env::consts::EXE_SUFFIX);
    path.join("bin").join(cargo).is_file() && path.join("lib").join("rustlib").is_dir()
}

fn reject_nested(inner: &Path, outer: &Path) -> Result<(), RunError> {
    if inner.starts_with(outer) {
        Err(RunError::Nested {
            inner: inner.to_owned(),
            outer: outer.to_owned(),
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::run::Batch;
    use crate::run::test_support::{Host, api_key, trial};
    use crate::task::BatchName;

    fn policy(trial: &Trial, machine: &Machine) -> Policy {
        Policy::for_trial(trial, machine, "api.example.com", &api_key()).unwrap()
    }

    #[test]
    fn a_trial_cannot_read_the_batchs_copy_of_the_task() {
        let host = Host::new();
        let batch = host.batch();
        let symposium = trial(&batch, 1, Configuration::Symposium);

        let policy = policy(&symposium, &host.machine());

        // It holds the hidden checks.
        assert!(!policy.can_read(&batch.inputs().join("tasks/store-durability/checks")));
    }

    #[test]
    fn a_trial_cannot_read_another_trial() {
        let host = Host::new();
        let batch = host.batch();
        let first = trial(&batch, 1, Configuration::Symposium);
        let second = trial(&batch, 2, Configuration::Baseline);

        let policy = policy(&first, &host.machine());

        let fixture = "fixture-store/src/lib.rs";
        assert!(policy.can_read(&first.project().join("src/lib.rs")));
        assert!(policy.can_read(&first.fixtures().join(fixture)));
        assert!(policy.can_read(&first.registry().join("fixture-store/Symposium.toml")));
        assert!(!policy.can_read(&second.project().join("src/lib.rs")));
        assert!(!policy.can_read(&second.fixtures().join(fixture)));
        assert!(!policy.can_read(&second.policy_file()));
    }

    #[test]
    fn only_a_symposium_trial_can_read_the_staged_cargo_agents() {
        let host = Host::new();
        let batch = host.batch();
        let staged = batch.symposium_tools().join("cargo-agents");
        let symposium = trial(&batch, 1, Configuration::Symposium);
        let baseline = trial(&batch, 2, Configuration::Baseline);

        assert!(policy(&symposium, &host.machine()).can_read(&staged));
        assert!(!policy(&baseline, &host.machine()).can_read(&staged));
    }

    #[test]
    fn a_trial_cannot_read_the_checkout_or_a_home() {
        let host = Host::new();
        let batch = host.batch();
        let trial = trial(&batch, 1, Configuration::Symposium);

        let policy = policy(&trial, &host.machine());

        for path in [
            host.join("checkout/evals/tasks/formatting/checks/double.rs"),
            host.join("home/dev/.cargo/credentials.toml"),
            host.join("home/dev/.cargo/bin/cargo-agents"),
        ] {
            assert!(!policy.can_read(&path), "{}", path.display());
        }
        for path in [
            host.join("home/dev/.rustup/toolchains/stable/bin/cargo"),
            batch.common_tools().join("claude"),
        ] {
            assert!(policy.can_read(&path), "{}", path.display());
        }
    }

    #[test]
    fn a_trial_writes_only_its_working_directories() {
        let host = Host::new();
        let batch = host.batch();
        let trial = trial(&batch, 1, Configuration::Symposium);

        let policy = policy(&trial, &host.machine());

        for dir in [
            trial.project(),
            trial.home(),
            trial.cargo_home(),
            trial.tmp(),
            trial.evidence(),
        ] {
            assert!(policy.can_write(&dir.join("file")), "{}", dir.display());
        }
        for path in [
            trial.fixtures().join("fixture-store/src/lib.rs"),
            trial.registry().join("fixture-store/Symposium.toml"),
            trial.bin().join("cargo"),
            trial.policy_file(),
            batch.symposium_tools().join("cargo-agents"),
        ] {
            assert!(!policy.can_write(&path), "{}", path.display());
        }
    }

    /// The file `srt` reads, kept as a record of its format.
    #[test]
    fn the_policy_is_written_as_literal_paths() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);

        let policy = serde_json::to_value(policy(&trial, &host.machine())).unwrap();

        let literal = |path: PathBuf| json!({ "path": path, "literal": true });
        let at = |relative: &str| literal(host.join(relative));
        let trial_dir = |name: &str| at(&format!("eval/batch/trial-1-with-symposium/{name}"));
        let expected: Value = json!({
            "filesystem": {
                "denyRead": [
                    at("home"),
                    at("eval"),
                    at("checkout"),
                ],
                "allowRead": [
                    trial_dir("fixtures"),
                    trial_dir("bin"),
                    at("eval/batch/tools/common"),
                    at("home/dev/.rustup/toolchains/stable"),
                    trial_dir("registry"),
                    at("eval/batch/tools/symposium"),
                ],
                "allowWrite": [
                    trial_dir("project"),
                    trial_dir("home"),
                    trial_dir("cargo-home"),
                    trial_dir("tmp"),
                    trial_dir("evidence"),
                ],
                "denyWrite": [],
            },
            "network": {
                "allowedDomains": ["api.example.com"],
                "deniedDomains": [],
                "tlsTerminate": {},
            },
            "credentials": {
                "envVars": [{ "name": "EXAMPLE_API_KEY", "mode": "mask" }],
            },
        });
        assert_eq!(policy, expected);
    }

    #[test]
    fn machine_paths_must_be_absolute() {
        let host = Host::new();
        let checkout = host.join("checkout");
        let toolchain = host.join("home/dev/.rustup/toolchains/stable");
        let relative = Path::new("relative");

        for (roots, checkout, toolchain) in [
            (
                vec![relative.to_owned()],
                checkout.as_path(),
                toolchain.as_path(),
            ),
            (Vec::new(), relative, toolchain.as_path()),
            (Vec::new(), checkout.as_path(), relative),
        ] {
            let result = Machine::with_exact_roots(roots, checkout, toolchain);
            assert!(
                matches!(result, Err(RunError::NotAbsolute(_))),
                "{result:?}"
            );
        }
    }

    #[test]
    fn a_missing_checkout_is_an_error() {
        let host = Host::new();

        let result = Machine::with_exact_roots(
            Vec::new(),
            &host.join("misspelled"),
            &host.join("home/dev/.rustup/toolchains/stable"),
        );

        assert!(
            matches!(result, Err(RunError::Canonicalize { .. })),
            "{result:?}"
        );
    }

    #[test]
    fn the_toolchain_must_be_a_sysroot() {
        let host = Host::new();

        for not_a_sysroot in ["home", "home/dev/.rustup"] {
            let result = Machine::with_exact_roots(
                Vec::new(),
                &host.join("checkout"),
                &host.join(not_a_sysroot),
            );
            assert!(
                matches!(result, Err(RunError::NotToolchain(_))),
                "{not_a_sysroot}: {result:?}"
            );
        }
    }

    #[test]
    fn overlapping_layouts_are_rejected() {
        let host = Host::new();
        let machine = host.machine();

        let inside_checkout =
            Batch::create(&host.join("checkout/eval"), &BatchName::new("b").unwrap()).unwrap();
        let trial = trial(&inside_checkout, 1, Configuration::Baseline);
        let result = Policy::for_trial(&trial, &machine, "api.example.com", &api_key());
        assert!(matches!(result, Err(RunError::Nested { .. })), "{result:?}");

        let toolchain = host.add_toolchain("checkout/toolchain");
        let result = Machine::with_exact_roots(Vec::new(), &host.join("checkout"), &toolchain);
        assert!(matches!(result, Err(RunError::Nested { .. })), "{result:?}");
    }

    #[test]
    fn extra_roots_add_to_the_platform_roots_and_the_home() {
        let [platform, home, extra] = ["/home", "/var/lib/service", "/srv"].map(PathBuf::from);

        let roots = user_data_roots(
            vec![platform.clone()],
            Some(home.clone()),
            vec![extra.clone()],
        );

        assert_eq!(roots, [platform, home, extra]);
    }

    #[test]
    fn a_root_named_twice_is_denied_once() {
        let host = Host::new();
        let home = host.join("home");

        let machine = Machine::with_exact_roots(
            vec![home.clone(), home.clone()],
            &host.join("checkout"),
            &host.join("home/dev/.rustup/toolchains/stable"),
        )
        .unwrap();

        assert_eq!(machine.user_data_roots, [home]);
    }

    /// `srt` matches read rules by the name a path is reached through, so a root reached
    /// through a link is denied by both names.
    #[cfg(unix)]
    #[test]
    fn a_root_reached_through_a_link_is_denied_by_both_names() {
        use std::os::unix::fs::symlink;

        let host = Host::new();
        for (link, target) in [("home-link", "home"), ("checkout-link", "checkout")] {
            symlink(host.join(target), host.join(link)).unwrap();
        }
        fs::create_dir(host.join("eval")).unwrap();
        symlink(host.join("eval"), host.join("eval-link")).unwrap();
        let machine = Machine::with_exact_roots(
            vec![host.join("home-link")],
            &host.join("checkout-link"),
            &host.join("home/dev/.rustup/toolchains/stable"),
        )
        .unwrap();
        let batch =
            Batch::create(&host.join("eval-link"), &BatchName::new("batch").unwrap()).unwrap();
        let trial = trial(&batch, 1, Configuration::Baseline);

        let policy = policy(&trial, &machine);

        for name in [
            "home-link",
            "home",
            "eval-link",
            "eval",
            "checkout-link",
            "checkout",
        ] {
            let path = host.join(name).join("file");
            assert!(!policy.can_read(&path), "{}", path.display());
        }
    }
}
