//! Workspace dependency resolution benchmarks.
//!
//! # `symposium_cache_miss` contract
//!
//! - **Claim:** A Symposium workspace-cache miss measures the complete work
//!   needed to resolve and persist dependency metadata for the reference
//!   project. Most of the measured time belongs to Cargo, so this quantifies
//!   the work avoided by a valid Symposium cache rather than Symposium's own
//!   overhead.
//! - **Workload:** A staged copy of the checked-in reference project, resolved
//!   through isolated Symposium configuration and cache directories.
//! - **Timed operation:** `WorkspaceDeps::load()`, including workspace lookup,
//!   Cargo metadata, result construction, and cache write-through.
//! - **Excluded setup:** Fixture staging, sandbox construction, workspace graph
//!   validation, workspace-cache removal, and resolver construction.
//! - **Invariants:** The staged graph has exactly the promised members and path
//!   dependencies; per-iteration setup establishes an empty workspace cache
//!   before every sample; loading must succeed rather than becoming a false
//!   fast sample.
//! - **Metric:** Wall-clock time per `WorkspaceDeps::load()` call.
//! - **Noise:** Cargo subprocess startup, operating-system filesystem caches,
//!   process scheduling, shared-runner hardware, and developer-level Cargo
//!   configuration during local runs. This is a Symposium cache miss, not a
//!   fully cold machine load.
//! - **Lifecycle:** Experimental.
//!
//! # `new_resolver_disk_cache_hit` contract
//!
//! - **Claim:** A new resolver with a valid Symposium disk cache measures the
//!   recurring component cost paid when dependency metadata can be reused. The
//!   result is normally dominated by `cargo locate-project`; it is not a direct
//!   benchmark of JSON deserialization.
//! - **Workload:** A staged copy of the checked-in reference project with a
//!   populated and validated Symposium workspace cache.
//! - **Timed operation:** `WorkspaceDeps::load()`, including workspace lookup,
//!   cache validation, file reading, deserialization, and memoization.
//! - **Excluded setup:** Fixture staging, sandbox construction, initial Cargo
//!   metadata resolution, workspace validation, cache-hit preflight, and new
//!   resolver construction.
//! - **Invariants:** Every sample receives a new resolver with an empty
//!   in-memory cache; an untimed metadata-rejecting Cargo preflight proves the
//!   disk cache is used; loading must succeed; the fixture lockfile is unchanged.
//! - **Metric:** Wall-clock time per `WorkspaceDeps::load()` call.
//! - **Noise:** Cargo subprocess startup, filesystem and operating-system
//!   caches, process scheduling, shared-runner hardware, and developer-level
//!   Cargo configuration during local runs.
//! - **Lifecycle:** Experimental.

use std::{hint::black_box, time::Duration};

use anyhow::{Context, Result, ensure};
use criterion::{BatchSize, Criterion, SamplingMode, criterion_group, criterion_main};

use symposium::{dirs::SymposiumDirs, pm::WorkspaceDeps};
use symposium_benchsuite::{Fixture, MetadataRejectingCargo, Sandbox, StagedFixture};

struct WorkspaceDepsWorkload {
    sandbox: Sandbox,
    project: StagedFixture,
    dirs: SymposiumDirs,
    guarded_cargo: MetadataRejectingCargo,
}

impl WorkspaceDepsWorkload {
    fn prepare() -> Result<Self> {
        let sandbox = Sandbox::new()?;
        let project = sandbox.stage(Fixture::ReferenceProject)?;
        let dirs = SymposiumDirs::new(
            sandbox.config_dir().to_path_buf(),
            sandbox.cache_dir().to_path_buf(),
            None,
        );
        let guarded_cargo = MetadataRejectingCargo::create_in(sandbox.root())?;
        let workload = Self {
            sandbox,
            project,
            dirs,
            guarded_cargo,
        };

        workload.resolve_and_check_workspace()?;
        workload.verify_cache_reset()?;
        // Cache-reset verification deliberately leaves the cache empty. Rebuild
        // it so the cache-hit case starts from a validated, populated state.
        workload.resolve_and_check_workspace()?;

        Ok(workload)
    }

    fn resolve_and_check_workspace(&self) -> Result<()> {
        let resolver = self.dirs.workspace_deps(self.project.path());
        let workspace = resolver.load().with_context(|| {
            format!(
                "resolving staged benchmark workspace `{}`",
                self.project.path().display()
            )
        })?;

        self.project.check_workspace(workspace)
    }

    /// Prove that the sandbox clears the cache location `WorkspaceDeps` uses.
    ///
    /// The directory name is duplicated across the two crates. If Symposium
    /// changes it without updating the benchsuite, cache clearing would become
    /// a no-op and silently turn iterations after the first into cache hits.
    fn verify_cache_reset(&self) -> Result<()> {
        ensure!(
            self.cache_contains_entries()?,
            "the validating load wrote no cache entry under `{}`",
            self.sandbox.cache_dir().display()
        );

        self.sandbox.clear_workspace_cache()?;

        ensure!(
            !self.cache_contains_entries()?,
            "workspace cache reset left entries under `{}`",
            self.sandbox.cache_dir().display()
        );

        Ok(())
    }

    fn cache_contains_entries(&self) -> Result<bool> {
        let cache_dir = self.sandbox.cache_dir();
        let first_entry = cache_dir
            .read_dir()
            .with_context(|| format!("reading benchmark cache `{}`", cache_dir.display()))?
            .next()
            .transpose()
            .with_context(|| format!("reading an entry in `{}`", cache_dir.display()))?;

        Ok(first_entry.is_some())
    }

    fn verify_disk_cache_hit(&self) -> Result<()> {
        let guarded_dirs = SymposiumDirs::new(
            self.sandbox.config_dir().to_path_buf(),
            self.sandbox.cache_dir().to_path_buf(),
            Some(self.guarded_cargo.executable().to_path_buf()),
        );
        let resolver = guarded_dirs.workspace_deps(self.project.path());
        let workspace = resolver.load().context(
            "loading the prepared disk cache through metadata-rejecting Cargo; \
             the cache lookup or workspace lookup missed",
        )?;

        self.project.check_workspace(workspace)
    }

    fn cache_miss_resolver(&self) -> Result<WorkspaceDeps> {
        self.sandbox.clear_workspace_cache()?;
        Ok(self.dirs.workspace_deps(self.project.path()))
    }

    fn disk_cache_hit_resolver(&self) -> WorkspaceDeps {
        self.dirs.workspace_deps(self.project.path())
    }
}

fn load_for_measurement(resolver: WorkspaceDeps) {
    let resolver = black_box(resolver);
    let workspace = resolver
        .load()
        .expect("workspace resolution failed during measurement");
    black_box(workspace);
}

fn benchmark_workspace_deps(criterion: &mut Criterion) {
    let workload = WorkspaceDepsWorkload::prepare()
        .expect("preparing the workspace dependency benchmark workload");
    let mut group = criterion.benchmark_group("workspace_deps");

    // These subprocess-bound cases can take more than a second per iteration.
    // Criterion's minimum sample count keeps each run bounded; stability comes
    // from repeated runs in the pinned benchmark environment.
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(15));
    // Cache misses are long-running, so flat sampling bounds the work. Making
    // the mode explicit also prevents latency drift from changing the model.
    group.sampling_mode(SamplingMode::Flat);
    group.bench_function("symposium_cache_miss", |bencher| {
        bencher.iter_batched(
            || {
                workload
                    .cache_miss_resolver()
                    .expect("preparing a Symposium workspace-cache miss")
            },
            load_for_measurement,
            // PerIteration is load-bearing, not a memory choice: setup clears the
            // workspace cache, and a larger batch runs every setup before timing
            // the batch, so only the first iteration would be a cache miss.
            BatchSize::PerIteration,
        );
    });
    workload
        .verify_disk_cache_hit()
        .expect("the disk cache must be valid before measuring cache hits");
    // Cache hits are short enough for linear sampling, which retains
    // Criterion's per-iteration regression model at an affordable cost.
    group.sampling_mode(SamplingMode::Linear);
    group.bench_function("new_resolver_disk_cache_hit", |bencher| {
        bencher.iter_batched(
            || workload.disk_cache_hit_resolver(),
            load_for_measurement,
            // Setup only creates independent resolvers; every timed load reads
            // the same immutable disk cache, so batching cannot change the path.
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(benches, benchmark_workspace_deps);
criterion_main!(benches);
