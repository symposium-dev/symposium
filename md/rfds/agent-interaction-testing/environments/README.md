# Execution environments

## Environment backends

| Backend | Purpose | Isolation claim |
|---|---|---|
| Host | Fast local iteration and native process behavior | Fresh test state, but installed tools and the host operating system remain observable |
| Linux container | Reproducible Linux conformance | Fresh restricted container with controlled tools, filesystem, and networking |

The host backend creates fresh project, home, configuration, cache, and temporary directories and passes an explicit filtered environment to every child process. Host results remain non-authoritative because installed tools and the operating system can influence them.

The Linux container backend is the isolated conformance environment. The tracer uses Docker behind an interface that can later support another container runtime, a VM, or a remote worker. Linux-container results are authoritative only for Linux.

Windows and macOS coverage is follow-up work. Native deterministic and real-process/PTY lanes use fresh test directories and cover platform-specific paths, command dispatch, shell behavior, PTYs, permissions, and process handling without claiming container-strength isolation. Native real-agent smokes require trusted runners and credentials.

## Scenario isolation

Every scenario and retry attempt receives unique workspace, home, cache, network, container, service, trace, and artifact identifiers. A fresh container is created per scenario, while deliberate CLI and agent restarts within that scenario retain its writable state.

Fixtures are copied into the environment rather than mounting the repository. Containers run as non-root with a read-only root filesystem, dropped capabilities, no Docker socket, explicit writable directories, and CPU, memory, process, and time limits.

Scenarios never mutate the runner's process-global environment. Concurrent scenarios share only immutable or content-addressed build assets. Image and binary preparation use an interprocess lock and publish a completed read-only artifact. Services use isolated networks and dynamic host ports. Resources are labelled by run and scenario, and cleanup is idempotent.

Every host and container run tests host-state exclusion. The runner places a harmless synthetic capability canary in a harness-owned decoy host configuration that is outside the fresh scenario home and, for local-auth tests, exercises the adapter's filtered credential bridge. It never writes a canary into the developer's real home. The runner asserts that the canary is absent from copied homes, agent-visible custom capabilities, and persisted artifacts.

The runner also asserts that the custom-skill inventory in every harness-controlled user and project scope exactly matches the fixture, including before every real-agent query. Agent-provided built-in capabilities are recorded separately and are not claimed as host state.

Agent concurrency is capped separately from CLI-only concurrency. A scenario may request an exclusive resource only for an external tool that genuinely cannot be isolated.

## Symposium binary provenance

By default, the runner builds one Linux `cargo-agents` artifact from the current checkout and reuses it across selected container scenarios. Its content-addressed key includes:

- source revision or dirty-source digest;
- `Cargo.lock` digest;
- Rust toolchain;
- target triple;
- profile and feature set; and
- container base-image digest.

Dependency and intermediate build layers are reused, so preparation is incremental rather than a per-scenario copy pause.

`--symposium-bin` is an explicit override, never an automatic PATH lookup. The runner checks its executable format, operating system, architecture, and available version metadata. Results record the binary digest and provenance as a checkout build or explicit override. Host runs apply the same checks to local artifacts.

## Provisioning and initialization

Placing the checkout binary on the isolated PATH is harness preparation, not an installer assertion. The tracer does not test `cargo install`, `cargo binstall`, release archives, or a future package manager.

Symposium initialization is product behavior. Fresh-user scenarios run the real `cargo agents init` process and assert configuration creation, agent setup, hook installation, optional choices, repeated-init idempotency, and later hook activation. Distribution paths can later feed the same post-provisioning scenarios.

## Network and fixture trust

Tracer fixture content is repository-owned and reviewed. An extension marked untrusted is awaiting Symposium consent; it is not arbitrary hostile code. Testing malicious hooks, MCP servers, agents, or fixtures requires a separate security-testing design.

The agent-free tracer container runs with networking disabled. Rust projects use path dependencies or a controlled local registry prepared before scenario execution. Later registry, git-source, MCP, hook, predicate, and failure scenarios may add declared local fixture processes or pinned image tools. A `cargo agents use` scenario that searches for a non-workspace crate must route `CargoPm::search` to a declared local fixture endpoint; it never queries crates.io. Fixture services can return exact versions, delays, disconnects, malformed data, and cache validators.

Step 4 adds only the provider egress needed by the real-agent query. The scenario container has no direct external egress. HTTPS provider traffic crosses a CONNECT proxy with an allowlist of destination hosts and ports. The proxy does not terminate TLS and the harness installs no interception certificate authority. Any fixture service remains on the internal network. Any additional external endpoint must be declared in the scenario and recorded in the execution plan and manifest.

Image construction and dependency acquisition finish before scenario execution and are identified by lockfiles and digests. Network failures are simulated through fixture services rather than public outages.

## Authentication

Container conformance uses a restricted API key available only to trusted jobs and never mounts local agent state.

Host authentication is explicit: `--auth api-key` or `--auth local`. API-key mode uses a fresh agent home. Local mode bridges only the minimum adapter-supported credential material, read-only, while agent settings, Symposium configuration, skills, hooks, MCP configuration, caches, and query history remain fresh.

If an agent cannot separate credentials from user configuration, the base result carries the [`non-authoritative(contaminated-auth-context)` modifier](../evidence/README.md#results-and-failure-ownership). The manifest records the mode and inherited credential paths without their contents.
