//! Unchanged-workspace hook dispatch benchmarks.
//!
//! `PreToolUse` fires once per agent tool call, so its latency is the cost a
//! user feels most often. Preparation asserts every property a measurement
//! depends on, so
//! `cargo test -p symposium-benchsuite --bench hook_dispatch` is a correctness
//! preflight for the dispatch path.
//!
//! # Contract
//!
//! Both cases share these fields; the table records what each one adds.
//!
//! - **Claim:** End-to-end wall-clock latency of the in-process `PreToolUse`
//!   pipeline in an unchanged Cargo workspace, at its floor and with a
//!   representative local registry. Neither case isolates registry processing.
//! - **Workload:** A staged copy of the reference project, plus the three-entry
//!   local registry for that case, with default auto-sync enabled, both builtin
//!   registries disabled, fresh workspace state, and a valid `WorkspaceDeps`
//!   disk cache.
//! - **Timed operation:** `execute_hook`: input parsing, the auto-sync
//!   freshness decision, builtin dispatch, workspace-cache reuse, plugin
//!   activation, and output serialization.
//! - **Excluded setup:** Everything `HookDispatchWorkload::prepare` does,
//!   including fixture staging, `Symposium` and Tokio runtime construction,
//!   cache population, and the invariant checks.
//! - **Invariants:** Auto-sync is enabled; the loaded registries and plugins are
//!   exactly those the scenario names; workspace state and the dependency cache
//!   are valid; `cargo metadata` is not attempted; no external plugin process
//!   runs; and a preflight produces the expected no-op output.
//! - **Metric:** Wall-clock time per in-process `PreToolUse` dispatch.
//! - **Noise:** Two Cargo workspace-lookup subprocesses dominate both results
//!   and can mask changes in registry loading and predicate evaluation.
//!   Filesystem and operating-system caches, process scheduling, shared-runner
//!   hardware, and developer-level Cargo configuration also vary.
//! - **Lifecycle:** Experimental.
//!
//! | Case | Configuration | Adds to the timed operation |
//! | --- | --- | --- |
//! | `pre_tool_use_minimal_config` | no registries | nothing; the pipeline and subprocess floor |
//! | `pre_tool_use_local_registry` | one path registry, three plugins | registry loading, hook selection, and predicate evaluation, with the sentinel absent so the gated hook never spawns |

use std::{
    hint::black_box,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use criterion::{Criterion, SamplingMode, criterion_group, criterion_main};
use serde_json::{Value, json};
use tokio::runtime::{Builder, Runtime};

use symposium::{
    config::Symposium,
    hook::{self, HookAgent, HookEvent},
    plugins,
    workspace_state::WorkspaceState,
};
use symposium_benchsuite::{Fixture, MetadataRejectingCargo, Sandbox, StagedFixture};

const LOCAL_REGISTRY_PLUGINS: &[&str] = &["always-active", "dormant", "predicate-gated"];
const PREDICATE_SENTINEL: &str = ".symposium-benchmark-never-present";

/// The checked-in configuration and fixtures for one dispatch measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookDispatchScenario {
    Minimal,
    LocalRegistry,
}

impl HookDispatchScenario {
    const fn config(self) -> &'static str {
        match self {
            Self::Minimal => include_str!("../../fixtures/config/minimal.toml"),
            Self::LocalRegistry => {
                include_str!("../../fixtures/config/local-registry.toml")
            }
        }
    }

    const fn registry_names(self) -> &'static [&'static str] {
        match self {
            Self::Minimal => &[],
            Self::LocalRegistry => &["benchmark-local"],
        }
    }

    const fn plugin_names(self) -> &'static [&'static str] {
        match self {
            Self::Minimal => &[],
            Self::LocalRegistry => LOCAL_REGISTRY_PLUGINS,
        }
    }

    fn stage_supporting_fixtures(self, sandbox: &Sandbox) -> Result<()> {
        match self {
            Self::Minimal => Ok(()),
            Self::LocalRegistry => {
                sandbox.stage(Fixture::LocalRegistry)?;
                Ok(())
            }
        }
    }

    fn verify_process_state(self) -> Result<()> {
        match self {
            Self::Minimal => Ok(()),
            Self::LocalRegistry => {
                let current_dir = std::env::current_dir()
                    .context("reading the benchmark process working directory")?;
                let sentinel = current_dir.join(PREDICATE_SENTINEL);

                ensure!(
                    !sentinel.try_exists().with_context(|| format!(
                        "checking for predicate sentinel `{}`",
                        sentinel.display()
                    ))?,
                    "predicate sentinel unexpectedly exists: {}",
                    sentinel.display()
                );

                Ok(())
            }
        }
    }
}

/// Construction checks every property a measurement depends on, so a broken
/// setup fails the run rather than shortening a sample.
struct HookDispatchWorkload {
    sandbox: Sandbox,
    symposium: Symposium,
    runtime: Runtime,
    input: String,
}

impl HookDispatchWorkload {
    fn prepare(scenario: HookDispatchScenario) -> Result<Self> {
        let sandbox = Sandbox::new()?;
        let project = sandbox.stage(Fixture::ReferenceProject)?;
        scenario.stage_supporting_fixtures(&sandbox)?;
        sandbox.write_config(scenario.config())?;

        // `from_dir` reads `config.toml` eagerly, so it has to exist by now.
        let symposium = Symposium::from_dir(sandbox.config_dir());
        // The pipeline awaits subprocesses; a worker pool would only add noise.
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .context("building the benchmark Tokio runtime")?;

        let workspace_root = warm_workspace_cache(&symposium, &project)?;
        mark_workspace_synced(&symposium, &workspace_root)?;

        let input = pre_tool_use_payload(project.path())?;
        let workload = Self {
            sandbox,
            symposium,
            runtime,
            input,
        };

        workload.verify_configuration(scenario, &project)?;
        workload.verify_dispatch()?;

        Ok(workload)
    }

    /// Run the operation measured by each dispatch case.
    fn dispatch(&self) -> Result<Vec<u8>> {
        self.dispatch_with(&self.symposium)
    }

    /// Run one dispatch through a supplied context so the preflight can
    /// substitute a guarded Cargo.
    fn dispatch_with(&self, symposium: &Symposium) -> Result<Vec<u8>> {
        self.runtime.block_on(async {
            hook::execute_hook(
                symposium,
                HookAgent::Claude,
                HookEvent::PreToolUse,
                &self.input,
            )
            .await
            .context("dispatching the PreToolUse hook")
        })
    }

    /// Prove the configuration this case describes is the one in effect.
    ///
    /// Each check covers a way the workload could silently measure less:
    /// auto-sync off returns before workspace lookup, the wrong configuration
    /// changes the registry set, and missing or malformed entries reduce the
    /// plugins loaded from the fixture.
    fn verify_configuration(
        &self,
        scenario: HookDispatchScenario,
        project: &StagedFixture,
    ) -> Result<()> {
        ensure!(
            self.symposium.config.auto_sync,
            "the hook-dispatch workload requires auto-sync to be enabled"
        );

        let registries = self.symposium.registry_instances();
        check_names(
            "registry instances",
            scenario.registry_names(),
            registries.iter().map(|registry| registry.name.as_str()),
        )?;

        let resolver = self.symposium.workspace_deps(project.path());
        let workspace = resolver
            .load()
            .context("loading the prepared workspace disk cache")?;
        let registry = self.runtime.block_on(plugins::load_registry_with_workspace(
            &self.symposium,
            Some(workspace),
        ));

        check_names(
            "loaded plugins",
            scenario.plugin_names(),
            registry
                .plugins
                .iter()
                .map(|parsed| parsed.plugin.name.as_str()),
        )?;
        ensure!(
            registry.warnings.is_empty(),
            "the hook-dispatch configuration produced {} plugin load warning(s)",
            registry.warnings.len()
        );
        scenario.verify_process_state()?;

        Ok(())
    }

    /// Dispatch once through a Cargo that refuses `metadata`.
    ///
    /// Refusal alone proves nothing: a failed `metadata` becomes "no workspace",
    /// which dispatch accepts and still returns `{}` for. The marker is what
    /// separates reading the disk cache from re-resolving and discarding.
    fn verify_dispatch(&self) -> Result<()> {
        let guard = MetadataRejectingCargo::create_in(self.sandbox.root())?;
        let mut guarded = Symposium::from_dir(self.sandbox.config_dir());
        guarded.set_cargo_override(guard.executable().to_path_buf());

        let output = self.dispatch_with(&guarded)?;

        ensure!(
            !guard.saw_metadata()?,
            "the unchanged path ran `cargo metadata` instead of reading the \
             workspace disk cache"
        );

        let output: Value =
            serde_json::from_slice(&output).context("parsing the hook output as JSON")?;
        ensure!(
            output == json!({}),
            "expected a no-op hook output, found `{output}`"
        );

        Ok(())
    }
}

fn check_names<'a>(
    kind: &str,
    expected: &[&str],
    actual: impl IntoIterator<Item = &'a str>,
) -> Result<()> {
    let mut expected = expected.to_vec();
    let mut actual: Vec<_> = actual.into_iter().collect();
    expected.sort_unstable();
    actual.sort_unstable();

    ensure!(
        actual == expected,
        "unexpected {kind}: expected [{}], found [{}]",
        expected.join(", "),
        actual.join(", ")
    );

    Ok(())
}

/// Validate the resolved graph and leave a warm disk cache behind.
fn warm_workspace_cache(symposium: &Symposium, project: &StagedFixture) -> Result<PathBuf> {
    let resolver = symposium.workspace_deps(project.path());
    let workspace = resolver.load().with_context(|| {
        format!(
            "resolving the staged benchmark workspace `{}`",
            project.path().display()
        )
    })?;

    project.check_workspace(workspace)?;

    Ok(workspace.root.clone())
}

/// `run_auto_sync` skips its work only when recorded state says the workspace
/// is unchanged; without this the dispatch measures a full sync instead. The
/// recorded root mirrors what a real sync writes.
fn mark_workspace_synced(symposium: &Symposium, workspace_root: &Path) -> Result<()> {
    let mut state = WorkspaceState::load(symposium, workspace_root);
    state.record_sync(workspace_root);
    state.workspace_root = Some(workspace_root.to_path_buf());
    state.save(symposium, workspace_root);

    ensure!(
        WorkspaceState::load(symposium, workspace_root).sync_is_fresh(workspace_root),
        "recorded workspace state did not reload as fresh for `{}`",
        workspace_root.display()
    );

    Ok(())
}

/// `cwd` is load-bearing: `execute_hook` falls back to the working directory of
/// the process, which for a bench binary is the Symposium workspace itself.
fn pre_tool_use_payload(project: &Path) -> Result<String> {
    let project = project
        .to_str()
        .with_context(|| format!("staged project path is not UTF-8: {}", project.display()))?;
    let payload = json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "cwd": project,
        "session_id": "benchmark",
        "tool_input": { "command": "true" },
    });

    serde_json::to_string(&payload).context("serializing the PreToolUse payload")
}

fn benchmark_hook_dispatch(criterion: &mut Criterion) {
    let minimal = HookDispatchWorkload::prepare(HookDispatchScenario::Minimal)
        .expect("preparing the minimal hook dispatch workload");
    let local_registry = HookDispatchWorkload::prepare(HookDispatchScenario::LocalRegistry)
        .expect("preparing the local-registry hook dispatch workload");
    let mut group = criterion.benchmark_group("hook_dispatch");

    // Hook dispatch is subprocess-bound, so use Criterion's minimum sample
    // count and collect stability data across runs in the pinned environment.
    // Flat sampling is explicit because Auto can switch modes as warm-up
    // latency changes, making otherwise comparable runs use different
    // statistics and allowing linear sampling to overshoot the time target.
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(15));
    group.sampling_mode(SamplingMode::Flat);
    group.bench_function("pre_tool_use_minimal_config", |bencher| {
        bencher.iter(|| {
            let output = black_box(&minimal)
                .dispatch()
                .expect("the timed minimal hook dispatch failed");
            black_box(output);
        });
    });
    group.bench_function("pre_tool_use_local_registry", |bencher| {
        bencher.iter(|| {
            let output = black_box(&local_registry)
                .dispatch()
                .expect("the timed local-registry hook dispatch failed");
            black_box(output);
        });
    });
    group.finish();
}

criterion_group!(benches, benchmark_hook_dispatch);
criterion_main!(benches);
