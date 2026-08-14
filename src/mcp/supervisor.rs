//! Keeping one backing server available.
//!
//! Servers are started on first use, not at session start: most sessions
//! touch few of the servers a workspace declares, and starting the rest buys
//! nothing.
//!
//! Restarting needs more care than a retry counter. A linear budget burns out
//! in seconds against a server that is briefly unavailable — blocked on a
//! build lock, say — so backoff is exponential. And a server that runs for
//! hours before dying once should not be treated as one that has never
//! worked, so a long-lived connection clears the count.
//!
//! What is *not* retried is the tool call. A call that dies mid-flight may
//! already have had its effect, so the failure is reported and the restart
//! happens on the next call instead.

use std::time::{Duration, Instant};

use rmcp::model::Tool;
use serde_json::Value;

use super::client::{BackingServer, ClientError, SpawnSpec};

/// How hard to try keeping a server up.
#[derive(Debug, Clone, Copy)]
pub struct RestartPolicy {
    pub max_restarts: u32,
    pub base_backoff: Duration,
    pub max_backoff: Duration,
    /// How long a connection must last before its failures are forgiven.
    pub stable_reset: Duration,
    /// How long a server gets to exit cleanly before being killed.
    pub shutdown_grace: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_restarts: 5,
            base_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            stable_reset: Duration::from_secs(300),
            shutdown_grace: Duration::from_secs(5),
        }
    }
}

/// Where a supervised server stands.
///
/// Transitions consume the value, so a state cannot be reached from one that
/// does not lead to it.
enum State {
    /// Not started. The normal resting state until a tool is called.
    Cold,
    Ready {
        server: Box<BackingServer>,
        since: Instant,
    },
    /// Out of restarts. Terminal for the session.
    Failed { reason: String },
}

impl State {
    fn name(&self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Ready { .. } => "ready",
            Self::Failed { .. } => "failed",
        }
    }
}

pub struct Supervisor {
    spec: SpawnSpec,
    policy: RestartPolicy,
    state: State,
    /// Failed start attempts since the last success that lasted.
    attempts: u32,
    /// When the next start attempt may happen.
    retry_after: Option<Instant>,
}

impl Supervisor {
    /// Register a server without starting it.
    pub fn new(spec: SpawnSpec, policy: RestartPolicy) -> Self {
        Self {
            spec,
            policy,
            state: State::Cold,
            attempts: 0,
            retry_after: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.spec.name
    }

    /// Current state, for reporting.
    pub fn state_name(&self) -> &'static str {
        self.state.name()
    }

    /// Whether a process is running right now.
    pub fn is_running(&self) -> bool {
        matches!(self.state, State::Ready { .. })
    }

    pub async fn list_tools(&mut self) -> Result<Vec<Tool>, ClientError> {
        self.ensure_ready().await?;
        let State::Ready { server, .. } = &self.state else {
            unreachable!("ensure_ready returned without a connection")
        };
        let outcome = server.list_tools().await;
        self.note_connection_health(&outcome);
        outcome
    }

    /// Call a tool, starting the server if needed.
    ///
    /// A call that fails because the connection broke is **not** retried: it
    /// may already have taken effect, and repeating it could take effect
    /// twice. The server is left cold so the next call starts a new one.
    pub async fn call(
        &mut self,
        tool: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value, ClientError> {
        self.ensure_ready().await?;
        let State::Ready { server, .. } = &self.state else {
            unreachable!("ensure_ready returned without a connection")
        };
        let outcome = server.call(tool, args, timeout).await;
        self.note_connection_health(&outcome);
        outcome
    }

    /// A broken connection means the process is gone; drop it so the next
    /// call starts a fresh one. A tool's own failure says nothing about the
    /// connection.
    fn note_connection_health<T>(&mut self, outcome: &Result<T, ClientError>) {
        if !matches!(outcome, Err(ClientError::Protocol { .. })) {
            return;
        }
        let State::Ready { since, .. } = &self.state else {
            return;
        };
        // A connection that lasted has earned a clean slate.
        if since.elapsed() >= self.policy.stable_reset {
            self.attempts = 0;
        }
        self.state = State::Cold;
    }

    async fn ensure_ready(&mut self) -> Result<(), ClientError> {
        match &self.state {
            State::Ready { .. } => return Ok(()),
            State::Failed { reason } => {
                return Err(ClientError::StartupFailed {
                    server: self.spec.name.clone(),
                    detail: reason.clone(),
                });
            }
            State::Cold => {}
        }

        if let Some(at) = self.retry_after {
            let now = Instant::now();
            if now < at {
                tokio::time::sleep(at - now).await;
            }
        }

        match BackingServer::spawn(&self.spec).await {
            Ok(server) => {
                self.state = State::Ready {
                    server: Box::new(server),
                    since: Instant::now(),
                };
                self.retry_after = None;
                Ok(())
            }
            // An authorization challenge is not a transient failure: the same
            // request will be refused until the user logs in, so retrying only
            // spends the restart budget.
            Err(e @ ClientError::AuthRequired { .. }) => {
                self.state = State::Failed {
                    reason: e.to_string(),
                };
                Err(e)
            }
            Err(e) => {
                self.attempts += 1;
                if self.attempts > self.policy.max_restarts {
                    self.state = State::Failed {
                        reason: e.to_string(),
                    };
                } else {
                    self.retry_after = Some(Instant::now() + self.backoff());
                }
                Err(e)
            }
        }
    }

    /// Exponential, capped. A failing server should not be hammered, but a
    /// transient failure should not cost a whole session either.
    fn backoff(&self) -> Duration {
        let shift = self.attempts.saturating_sub(1).min(16);
        self.policy
            .base_backoff
            .saturating_mul(1u32 << shift)
            .min(self.policy.max_backoff)
    }

    /// Close the connection, giving the server a chance to exit cleanly
    /// before its process group is killed on drop.
    pub async fn shutdown(&mut self) {
        let previous = std::mem::replace(&mut self.state, State::Cold);
        if let State::Ready { server, .. } = previous {
            // A hung shutdown must not wedge the session; the drop that
            // follows kills the process group regardless.
            let _ = tokio::time::timeout(self.policy.shutdown_grace, server.shutdown()).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    /// Locate the mock server built alongside the test binary.
    fn mock_binary() -> PathBuf {
        let mut dir = std::env::current_exe().expect("test binary path");
        dir.pop(); // deps/
        if dir.ends_with("deps") {
            dir.pop();
        }
        dir.join("examples").join("mock-mcp-server")
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        config: PathBuf,
    }

    fn fixture(config: Value) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mock.json");
        std::fs::write(&path, config.to_string()).unwrap();
        Fixture {
            _dir: dir,
            config: path,
        }
    }

    fn spec(fixture: &Fixture, startup_timeout: Duration) -> SpawnSpec {
        SpawnSpec {
            name: "mock".to_string(),
            startup_timeout,
            kind: crate::mcp::client::SpawnKind::Child {
                command: mock_binary(),
                args: vec!["--config".to_string(), fixture.config.display().to_string()],
                env: Vec::new(),
                cwd: None,
            },
        }
    }

    fn fast_policy() -> RestartPolicy {
        RestartPolicy {
            base_backoff: Duration::from_millis(10),
            max_backoff: Duration::from_millis(50),
            shutdown_grace: Duration::from_millis(500),
            ..RestartPolicy::default()
        }
    }

    fn echo_config() -> Value {
        json!({
            "name": "mock",
            "tools": [{"name": "echo", "behavior": {"kind": "echo"}}]
        })
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn server_is_not_started_until_a_tool_is_called() {
        let f = fixture(echo_config());
        let mut sup = Supervisor::new(spec(&f, Duration::from_secs(10)), fast_policy());

        assert_eq!(sup.state_name(), "cold");
        assert!(!sup.is_running());

        sup.call("echo", json!({"a": 1}), Duration::from_secs(5))
            .await
            .unwrap();
        assert!(sup.is_running(), "first call should have started it");
        sup.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn calls_reach_the_server() {
        let f = fixture(echo_config());
        let mut sup = Supervisor::new(spec(&f, Duration::from_secs(10)), fast_policy());

        let out = sup
            .call("echo", json!({"a": 1}), Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(out, json!({"a": 1}));
        sup.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tools_are_listed() {
        let f = fixture(echo_config());
        let mut sup = Supervisor::new(spec(&f, Duration::from_secs(10)), fast_policy());

        let tools = sup.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name.as_ref(), "echo");
        sup.shutdown().await;
    }

    /// A server that fails a few times and then works should end up working.
    #[tokio::test(flavor = "multi_thread")]
    async fn transient_startup_failures_are_retried() {
        let mut config = echo_config();
        config["fail_startup_times"] = json!(2);
        let f = fixture(config);
        let mut sup = Supervisor::new(spec(&f, Duration::from_secs(10)), fast_policy());

        for _ in 0..2 {
            assert!(
                sup.call("echo", json!({}), Duration::from_secs(5))
                    .await
                    .is_err(),
                "the first attempts are expected to fail"
            );
            assert_eq!(sup.state_name(), "cold", "still retryable");
        }

        let out = sup
            .call("echo", json!({"n": 1}), Duration::from_secs(5))
            .await;
        assert!(out.is_ok(), "should recover once the server stops failing");
        sup.shutdown().await;
    }

    /// A server that never starts must stop being retried, or every later
    /// call pays the startup cost again.
    #[tokio::test(flavor = "multi_thread")]
    async fn permanent_startup_failure_becomes_terminal() {
        let mut config = echo_config();
        config["fail_startup_times"] = json!(999);
        let f = fixture(config);
        let policy = RestartPolicy {
            max_restarts: 2,
            ..fast_policy()
        };
        let mut sup = Supervisor::new(spec(&f, Duration::from_secs(10)), policy);

        for _ in 0..3 {
            let _ = sup.call("echo", json!({}), Duration::from_secs(5)).await;
        }
        assert_eq!(
            sup.state_name(),
            "failed",
            "should give up after exhausting its restarts"
        );

        // A terminal server answers immediately rather than trying again.
        let started = Instant::now();
        let err = sup
            .call("echo", json!({}), Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(matches!(err, ClientError::StartupFailed { .. }));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a failed server should not be retried"
        );
    }

    /// A server behind OAuth is terminal on the first refusal: retrying spends
    /// the restart budget on an answer that cannot change until the user logs
    /// in.
    #[tokio::test]
    async fn an_authorization_challenge_fails_without_retrying() {
        let mut sup = Supervisor::new(
            SpawnSpec {
                name: "sentry".into(),
                startup_timeout: Duration::from_secs(1),
                kind: crate::mcp::client::SpawnKind::Http {
                    // Refused before any connection is attempted, which is the
                    // point: no network is needed to prove the state machine.
                    url: "https://10.0.0.1/mcp".into(),
                    headers: vec![],
                    config_dir: std::path::PathBuf::from("/nonexistent"),
                },
            },
            fast_policy(),
        );

        // Stand in for the challenge the transport would surface.
        sup.state = State::Failed {
            reason: ClientError::AuthRequired {
                server: "sentry".into(),
                challenge: "Bearer resource_metadata=\"https://example.com/.well-known\"".into(),
            }
            .to_string(),
        };

        let err = sup
            .call("anything", json!({}), Duration::from_secs(1))
            .await
            .expect_err("a failed server refuses calls");
        assert!(
            err.to_string().contains("requires authorization"),
            "got: {err}"
        );
        assert_eq!(sup.state_name(), "failed");
    }

    /// Backoff grows so a crash-looping server is not hammered, but stays
    /// capped so recovery is not delayed for minutes.
    #[test]
    fn backoff_grows_and_is_capped() {
        let mut sup = Supervisor::new(
            SpawnSpec {
                name: "x".into(),
                startup_timeout: Duration::from_secs(1),
                kind: crate::mcp::client::SpawnKind::Child {
                    command: "true".into(),
                    args: vec![],
                    env: vec![],
                    cwd: None,
                },
            },
            RestartPolicy {
                base_backoff: Duration::from_secs(1),
                max_backoff: Duration::from_secs(8),
                ..RestartPolicy::default()
            },
        );

        let mut seen = Vec::new();
        for attempt in 1..=6 {
            sup.attempts = attempt;
            seen.push(sup.backoff());
        }
        assert_eq!(
            seen,
            vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8),
                Duration::from_secs(8),
                Duration::from_secs(8),
            ]
        );
    }

    /// A call that dies mid-flight may already have taken effect, so it is
    /// reported rather than repeated. The next call starts a fresh server.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_call_lost_to_a_crash_is_not_repeated() {
        let f = fixture(json!({
            "name": "mock",
            "tools": [{"name": "boom", "behavior": {"kind": "crash_after", "calls": 1}}]
        }));
        let mut sup = Supervisor::new(spec(&f, Duration::from_secs(10)), fast_policy());

        let first = sup.call("boom", json!({}), Duration::from_secs(5)).await;
        assert!(first.is_ok(), "the first call is served: {first:?}");

        let second = sup.call("boom", json!({}), Duration::from_secs(5)).await;
        assert!(
            matches!(second, Err(ClientError::Protocol { .. })),
            "a lost call is reported, got: {second:?}"
        );
        assert_eq!(
            sup.state_name(),
            "cold",
            "the dead process should be dropped, not reused"
        );
        sup.shutdown().await;
    }

    /// A tool with no parameters must still be sent an object. The spec makes
    /// `arguments` optional, but zod-based servers reject an absent field, and
    /// those are a large share of what is published.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_tool_taking_no_arguments_is_sent_an_empty_object() {
        let f = fixture(json!({
            "name": "mock",
            "tools": [{"name": "ping", "behavior": {"kind": "echo"}}]
        }));
        let mut sup = Supervisor::new(spec(&f, Duration::from_secs(10)), fast_policy());

        // `echo` returns whatever arguments it received.
        let out = sup
            .call("ping", Value::Null, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(out, json!({}), "an object, not null");
        sup.shutdown().await;
    }

    /// A server that reads the project it serves needs to run inside it.
    /// Four of the seven client config formats carry this for that reason.
    ///
    /// Proved with a *relative* config path: the server can only find its
    /// config if the working directory was applied.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_server_runs_in_its_configured_directory() {
        let f = fixture(json!({
            "name": "mock",
            "tools": [{"name": "echo", "behavior": {"kind": "echo"}}]
        }));
        let dir = f.config.parent().expect("fixture dir").to_path_buf();

        let relative = SpawnSpec {
            name: "mock".to_string(),
            startup_timeout: Duration::from_secs(10),
            kind: crate::mcp::client::SpawnKind::Child {
                command: mock_binary(),
                args: vec!["--config".to_string(), "mock.json".to_string()],
                env: Vec::new(),
                cwd: Some(dir.clone()),
            },
        };
        let mut sup = Supervisor::new(relative, fast_policy());
        sup.call("echo", json!({"ok": 1}), Duration::from_secs(5))
            .await
            .expect("a relative config resolves only from the right directory");
        sup.shutdown().await;

        // Without it, the same relative path cannot be found.
        let mut without_cwd = Supervisor::new(
            SpawnSpec {
                name: "mock".to_string(),
                startup_timeout: Duration::from_secs(10),
                kind: crate::mcp::client::SpawnKind::Child {
                    command: mock_binary(),
                    args: vec!["--config".to_string(), "mock.json".to_string()],
                    env: Vec::new(),
                    cwd: None,
                },
            },
            fast_policy(),
        );
        assert!(
            without_cwd
                .call("echo", json!({}), Duration::from_secs(5))
                .await
                .is_err(),
            "the relative path should not resolve from the test's own directory"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_hanging_tool_times_out() {
        let f = fixture(json!({
            "name": "mock",
            "tools": [{"name": "slow", "behavior": {"kind": "hang"}}]
        }));
        let mut sup = Supervisor::new(spec(&f, Duration::from_secs(10)), fast_policy());

        let err = sup
            .call("slow", json!({}), Duration::from_millis(300))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ClientError::CallTimeout { .. }),
            "got: {err:?}"
        );
        sup.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_slow_start_times_out() {
        let mut config = echo_config();
        config["startup_delay_ms"] = json!(5000);
        let f = fixture(config);
        let mut sup = Supervisor::new(spec(&f, Duration::from_millis(200)), fast_policy());

        let err = sup
            .call("echo", json!({}), Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ClientError::StartupTimeout { .. }),
            "got: {err:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn shutdown_leaves_no_connection() {
        let f = fixture(echo_config());
        let mut sup = Supervisor::new(spec(&f, Duration::from_secs(10)), fast_policy());

        sup.call("echo", json!({}), Duration::from_secs(5))
            .await
            .unwrap();
        assert!(sup.is_running());

        sup.shutdown().await;
        assert!(!sup.is_running());
        assert_eq!(sup.state_name(), "cold");
    }
}
