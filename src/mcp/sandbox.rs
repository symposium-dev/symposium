//! The JavaScript sandbox scripts run in.
//!
//! A script is model-written and untrusted in the sense that matters here: it
//! may loop forever, allocate without bound, or recurse until the stack gives
//! out. None of those may take the agent's session with them.
//!
//! Two properties do the work:
//!
//! * **Deny by construction.** A bare QuickJS context has no filesystem,
//!   network, process or module loader — not because they are switched off,
//!   but because nothing registers them. There is no allowlist to get wrong.
//! * **Two layers of deadline.** An interrupt handler stops a script spinning
//!   in the interpreter; an outer timeout catches one blocked awaiting a host
//!   call, where the interpreter is not running and the interrupt cannot fire.
//!   Neither alone is sufficient.
//!
//! The interrupt raises an exception the script cannot catch, so
//! `try { while(true){} } catch(e) {}` still terminates.

use std::time::{Duration, Instant};

use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Ctx};
use serde::Serialize;
use serde_json::Value;

use super::{console, dispatch, normalize};

/// Extra room beyond the JavaScript stack limit for the interpreter's own
/// frames. Without it a script that hits the JS limit would instead overflow
/// the OS thread stack, which is a crash rather than an exception.
const STACK_HEADROOM: usize = 1 << 20;

/// How long each deadline layer waits past the one inside it.
///
/// The interrupt produces a precise error, so give it a moment to win before
/// the blunter layers fire.
const OUTER_GRACE: Duration = Duration::from_millis(250);

/// Bounds on one script execution.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub timeout: Duration,
    pub memory_bytes: usize,
    pub stack_bytes: usize,
    /// Ceiling on the serialized result. Exceeding it truncates rather than
    /// fails: a script that already called tools should not lose its work.
    pub max_result_bytes: usize,
    /// Ceiling on captured console output.
    pub max_console_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            memory_bytes: 64 << 20,
            stack_bytes: 1 << 20,
            max_result_bytes: 32 << 10,
            max_console_bytes: 8 << 10,
        }
    }
}

/// Why a script produced no value.
///
/// Serializes to a tagged object so the model can tell a limit it exceeded
/// from a mistake in its own code, and retry accordingly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum SandboxError {
    ScriptTimeout { limit_secs: u64 },
    MemoryExhausted { limit_mb: u64 },
    ScriptError { message: String },
    Internal { message: String },
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ScriptTimeout { limit_secs } => {
                write!(f, "script exceeded its {limit_secs}s deadline")
            }
            Self::MemoryExhausted { limit_mb } => {
                write!(f, "script exceeded its {limit_mb}MB memory limit")
            }
            Self::ScriptError { message } | Self::Internal { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for SandboxError {}

/// What a script produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The result, serialized. A prefix of it when `truncated_from` is set.
    pub json: String,
    /// Byte length of the untruncated result, when it did not fit.
    pub truncated_from: Option<usize>,
    /// Console output, in the order it was written.
    pub logs: Vec<String>,
    /// Whether logging stopped for want of budget.
    pub logs_dropped: bool,
}

impl Outcome {
    /// Parse the result back, for callers that know it was not truncated.
    pub fn value(&self) -> Result<Value, SandboxError> {
        serde_json::from_str(&self.json).map_err(|e| SandboxError::Internal {
            message: format!("result was not valid JSON: {e}"),
        })
    }
}

/// Runs one script under [`Limits`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Sandbox {
    limits: Limits,
}

impl Sandbox {
    pub fn new(limits: Limits) -> Self {
        Self { limits }
    }

    /// Evaluate `script` and return its value as JSON.
    ///
    /// The engine runs on its own thread: QuickJS is not `Send`, and the
    /// thread lets the stack be sized against the configured limit. A fresh
    /// runtime per call means no state survives from one script to the next.
    pub async fn eval(&self, script: &str) -> Result<Value, SandboxError> {
        self.eval_inner(script, None, Vec::new(), None)
            .await?
            .value()
    }

    /// Run a model-written script.
    ///
    /// The source is handed to the engine as a value rather than spliced into
    /// program text.
    pub async fn run_script(&self, source: &str) -> Result<Outcome, SandboxError> {
        self.run_script_with(source, &[], dispatch::channel().0)
            .await
    }

    /// Run a script with backing servers in scope.
    pub async fn run_script_with(
        &self,
        source: &str,
        namespaces: &[dispatch::Namespace],
        calls: dispatch::CallSender,
    ) -> Result<Outcome, SandboxError> {
        self.eval_inner(
            normalize::PROGRAM,
            Some(&normalize::prepare(source)),
            namespaces.to_vec(),
            Some(calls),
        )
        .await
    }

    async fn eval_inner(
        &self,
        script: &str,
        source: Option<&str>,
        namespaces: Vec<dispatch::Namespace>,
        calls: Option<dispatch::CallSender>,
    ) -> Result<Outcome, SandboxError> {
        let limits = self.limits;
        let script = script.to_string();
        let source = source.map(str::to_string);
        let (tx, rx) = tokio::sync::oneshot::channel();

        let spawned = std::thread::Builder::new()
            .name("mcp-sandbox".to_string())
            .stack_size(limits.stack_bytes + STACK_HEADROOM)
            .spawn(move || {
                let outcome = match tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                {
                    // The interrupt only fires while the interpreter runs, so
                    // a script parked on an unresolved promise has nothing to
                    // interrupt. Abandoning the future drops the runtime, and
                    // with it the tool-call sender the caller's pump awaits.
                    Ok(rt) => rt.block_on(async {
                        let bounded = tokio::time::timeout(
                            limits.timeout + OUTER_GRACE,
                            run(
                                &script,
                                source.as_deref(),
                                &namespaces,
                                calls.as_ref(),
                                limits,
                            ),
                        );
                        match bounded.await {
                            Ok(outcome) => outcome,
                            Err(_) => Err(SandboxError::ScriptTimeout {
                                limit_secs: limits.timeout.as_secs(),
                            }),
                        }
                    }),
                    Err(e) => Err(SandboxError::Internal {
                        message: format!("could not start sandbox runtime: {e}"),
                    }),
                };
                // A closed receiver means the outer deadline already fired.
                let _ = tx.send(outcome);
            });

        if let Err(e) = spawned {
            return Err(SandboxError::Internal {
                message: format!("could not start sandbox thread: {e}"),
            });
        }

        // Backstop for a thread that died without reporting. Given room to
        // lose the race, so the precise error above wins.
        match tokio::time::timeout(limits.timeout + OUTER_GRACE * 2, rx).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => Err(SandboxError::Internal {
                message: "sandbox thread ended without reporting".to_string(),
            }),
            Err(_) => Err(SandboxError::ScriptTimeout {
                limit_secs: limits.timeout.as_secs(),
            }),
        }
    }
}

async fn run(
    script: &str,
    source: Option<&str>,
    namespaces: &[dispatch::Namespace],
    calls: Option<&dispatch::CallSender>,
    limits: Limits,
) -> Result<Outcome, SandboxError> {
    let runtime = AsyncRuntime::new().map_err(internal)?;
    runtime.set_memory_limit(limits.memory_bytes).await;
    runtime.set_max_stack_size(limits.stack_bytes).await;

    // The inner layer. Fires while the interpreter is running, and raises an
    // exception the script cannot catch.
    let deadline = Instant::now() + limits.timeout;
    runtime
        .set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)))
        .await;

    let log = console::Log::shared(limits.max_console_bytes);
    let context = AsyncContext::full(&runtime).await.map_err(internal)?;
    let outcome = AsyncContext::async_with(&context, async |ctx| {
        evaluate(ctx, script, source, namespaces, calls, &log).await
    })
    .await;

    // Settle any promises the script left pending before deciding the result.
    runtime.idle().await;

    let (logs, logs_dropped) = {
        let log = log.borrow();
        (log.entries().to_vec(), log.dropped())
    };

    let json = match outcome {
        Ok(json) => json,
        Err(message) => {
            let allocated = runtime.memory_usage().await.malloc_size.max(0) as usize;
            return Err(classify(message, deadline, limits, allocated));
        }
    };

    let (json, truncated_from) = truncate(json, limits.max_result_bytes);
    Ok(Outcome {
        json,
        truncated_from,
        logs,
        logs_dropped,
    })
}

/// Bound the serialized result.
///
/// Truncating rather than rejecting keeps whatever the script achieved: it may
/// already have called tools with side effects, and an error would discard
/// that along with the data. The cut lands on a character boundary, so the
/// prefix is still valid text even though it is no longer valid JSON.
fn truncate(json: String, limit: usize) -> (String, Option<usize>) {
    if json.len() <= limit {
        return (json, None);
    }
    let original = json.len();
    let mut end = limit;
    while end > 0 && !json.is_char_boundary(end) {
        end -= 1;
    }
    let mut cut = json;
    cut.truncate(end);
    (cut, Some(original))
}

async fn evaluate<'js>(
    ctx: Ctx<'js>,
    script: &str,
    source: Option<&str>,
    namespaces: &[dispatch::Namespace],
    calls: Option<&dispatch::CallSender>,
    log: &console::Shared,
) -> Result<String, String> {
    console::install(&ctx, log)?;
    if let Some(calls) = calls {
        dispatch::install(&ctx, namespaces, calls)?;
    }

    if let Some(source) = source {
        ctx.globals()
            .set(normalize::SOURCE_GLOBAL, source)
            .catch(&ctx)
            .map_err(|e| e.to_string())?;
    }

    // Evaluated as a plain script rather than a module: scripts arrive
    // normalized into a self-calling async function, so the value is already a
    // promise and top-level `await` is never needed.
    let value: rquickjs::Value<'js> = ctx
        .eval(script.as_bytes().to_vec())
        .catch(&ctx)
        .map_err(|e| e.to_string())?;

    // A script is normally an async function call, so the value is a promise.
    let resolved = match value.as_promise() {
        Some(promise) => promise
            .clone()
            .into_future::<rquickjs::Value>()
            .await
            .catch(&ctx)
            .map_err(|e| e.to_string())?,
        None => value,
    };

    to_json(&ctx, resolved)
}

/// Convert a JavaScript value to JSON via the engine's own serializer, so
/// `toJSON` and nested structures behave as the script author expects.
fn to_json<'js>(ctx: &Ctx<'js>, value: rquickjs::Value<'js>) -> Result<String, String> {
    if value.is_undefined() {
        return Ok("null".to_string());
    }
    let encoded = ctx
        .json_stringify(value)
        .catch(ctx)
        .map_err(|e| e.to_string())?;
    let Some(encoded) = encoded else {
        // `JSON.stringify` yields nothing for values with no JSON form.
        return Ok("null".to_string());
    };
    encoded.to_string().map_err(|e| e.to_string())
}

/// Decide which limit, if any, a failure represents.
///
/// QuickJS surfaces both an interrupt and an allocation failure as ordinary
/// exceptions, so the cause has to be recovered from the surrounding state
/// rather than the message. An exhausted heap in particular throws a null
/// value, because there is no memory left to build an error object with.
fn classify(message: String, deadline: Instant, limits: Limits, allocated: usize) -> SandboxError {
    if Instant::now() >= deadline {
        return SandboxError::ScriptTimeout {
            limit_secs: limits.timeout.as_secs(),
        };
    }
    // Still holding most of the budget when the exception surfaced.
    if allocated * 10 >= limits.memory_bytes * 8 {
        return SandboxError::MemoryExhausted {
            limit_mb: (limits.memory_bytes / (1 << 20)) as u64,
        };
    }
    SandboxError::ScriptError { message }
}

fn internal(e: impl std::fmt::Display) -> SandboxError {
    SandboxError::Internal {
        message: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fast() -> Limits {
        Limits {
            timeout: Duration::from_millis(300),
            ..Limits::default()
        }
    }

    async fn eval(script: &str) -> Result<Value, SandboxError> {
        Sandbox::new(fast()).eval(script).await
    }

    #[tokio::test]
    async fn evaluates_an_expression() {
        assert_eq!(eval("1 + 1").await.unwrap(), json!(2));
    }

    #[tokio::test]
    async fn returns_structured_values() {
        let out = eval(r#"({ rows: [{ id: 1 }], ok: true })"#).await.unwrap();
        assert_eq!(out, json!({"rows": [{"id": 1}], "ok": true}));
    }

    #[tokio::test]
    async fn awaits_a_returned_promise() {
        let out = eval("(async () => 42)()").await.unwrap();
        assert_eq!(out, json!(42));
    }

    #[tokio::test]
    async fn undefined_becomes_null() {
        assert_eq!(eval("undefined").await.unwrap(), Value::Null);
    }

    // -- deadlines --

    #[tokio::test]
    async fn cpu_bound_loop_hits_the_deadline() {
        let err = eval("while (true) {}").await.unwrap_err();
        assert_eq!(err, SandboxError::ScriptTimeout { limit_secs: 0 });
    }

    /// The interrupt raises an exception the script cannot catch. Were it
    /// catchable, a model could sit inside `try`/`catch` and never yield.
    #[tokio::test]
    async fn script_cannot_catch_the_deadline() {
        let err =
            eval(r#"(() => { try { while (true) {} } catch (e) { return "swallowed"; } })()"#)
                .await
                .unwrap_err();
        assert!(
            matches!(err, SandboxError::ScriptTimeout { .. }),
            "deadline must not be catchable, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn deadline_is_enforced_promptly() {
        let started = Instant::now();
        let _ = eval("while (true) {}").await;
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "took {:?}",
            started.elapsed()
        );
    }

    /// Nothing to interrupt: the deadline is the only thing that can end it.
    #[tokio::test]
    async fn a_promise_that_never_settles_still_reports() {
        let started = Instant::now();
        let err = Sandbox::new(fast())
            .run_script("return new Promise(() => {})")
            .await
            .unwrap_err();

        assert!(
            matches!(err, SandboxError::ScriptTimeout { .. }),
            "got: {err:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "took {:?}",
            started.elapsed()
        );
    }

    /// The engine thread owns the only tool-call sender, so the channel
    /// closing is how a caller knows the thread went away.
    #[tokio::test]
    async fn a_wedged_script_releases_the_dispatch_channel() {
        let (calls, mut receiver) = dispatch::channel();
        let err = Sandbox::new(fast())
            .run_script_with("return new Promise(() => {})", &[], calls)
            .await
            .unwrap_err();
        assert!(
            matches!(err, SandboxError::ScriptTimeout { .. }),
            "got: {err:?}"
        );

        let closed = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("the sandbox thread must drop its sender, not hold it");
        assert!(
            closed.is_none(),
            "channel should be closed, got: {closed:?}"
        );
    }

    /// An un-awaited call must not keep the engine past its deadline.
    #[tokio::test]
    async fn an_unawaited_tool_call_does_not_outlive_the_deadline() {
        let (calls, _receiver) = dispatch::channel();
        let namespaces = vec![dispatch::Namespace {
            key: "srv".to_string(),
            server: "srv".to_string(),
        }];

        let started = Instant::now();
        // No `await`, and nothing is servicing the channel.
        let err = Sandbox::new(fast())
            .run_script_with("srv.go({}); return 1;", &namespaces, calls)
            .await
            .unwrap_err();

        assert!(
            matches!(err, SandboxError::ScriptTimeout { .. }),
            "got: {err:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "took {:?}",
            started.elapsed()
        );
    }

    // -- memory --

    /// `set_memory_limit` is inert under a custom allocator, and feature
    /// unification means a transitive dependency could enable
    /// `rquickjs/rust-alloc` invisibly. `MemoryExhausted` alone: a timeout
    /// would also be reached with the limit inert.
    #[tokio::test]
    async fn allocation_is_bounded() {
        let sandbox = Sandbox::new(Limits {
            memory_bytes: 1 << 20,
            ..fast()
        });
        let err = sandbox
            .eval("const a = []; while (true) { a.push(new Array(10000)); }")
            .await
            .unwrap_err();
        assert!(
            matches!(err, SandboxError::MemoryExhausted { .. }),
            "the memory limit must be what stops this, got: {err:?}"
        );
    }

    /// Deep recursion must raise a JavaScript exception rather than overflow
    /// the thread's own stack, which would abort the process.
    #[tokio::test]
    async fn recursion_is_bounded() {
        let err = eval("(function f() { return f(); })()").await.unwrap_err();
        assert!(
            matches!(err, SandboxError::ScriptError { .. }),
            "got: {err:?}"
        );
    }

    // -- errors --

    #[tokio::test]
    async fn script_errors_carry_their_message() {
        let err = eval(r#"throw new Error("boom")"#).await.unwrap_err();
        let SandboxError::ScriptError { message } = &err else {
            panic!("expected a script error, got: {err:?}");
        };
        assert!(message.contains("boom"), "got: {message}");
    }

    #[tokio::test]
    async fn syntax_errors_are_reported() {
        let err = eval("this is not javascript").await.unwrap_err();
        assert!(
            matches!(err, SandboxError::ScriptError { .. }),
            "got: {err:?}"
        );
    }

    /// The model is told what it hit, in a form it can act on.
    #[tokio::test]
    async fn errors_serialize_as_tagged_objects() {
        let err = SandboxError::ScriptTimeout { limit_secs: 120 };
        assert_eq!(
            serde_json::to_value(&err).unwrap(),
            json!({"error": "script_timeout", "limit_secs": 120})
        );
    }

    // -- result size --

    /// Truncation rather than rejection: a script that already called tools
    /// may have caused side effects, and an error would discard the data it
    /// gathered along with them.
    #[tokio::test]
    async fn oversized_results_are_truncated_not_rejected() {
        let sandbox = Sandbox::new(Limits {
            max_result_bytes: 64,
            ..fast()
        });
        let outcome = sandbox
            .run_script(r#"return "x".repeat(5000);"#)
            .await
            .expect("an oversized result is not a failure");

        assert_eq!(outcome.json.len(), 64);
        assert_eq!(
            outcome.truncated_from,
            Some(5002),
            "the original length is reported so the model can judge the shortfall"
        );
    }

    #[tokio::test]
    async fn results_within_the_limit_are_untouched() {
        let outcome = Sandbox::new(fast()).run_script("return 42;").await.unwrap();
        assert_eq!(outcome.json, "42");
        assert_eq!(outcome.truncated_from, None);
    }

    /// The cut lands on a character boundary, so the prefix is still valid
    /// text even though it is no longer valid JSON.
    #[tokio::test]
    async fn truncation_does_not_split_a_character() {
        for limit in 8..24 {
            let sandbox = Sandbox::new(Limits {
                max_result_bytes: limit,
                ..fast()
            });
            let outcome = sandbox.run_script(r#"return "éééééééééé";"#).await.unwrap();
            assert!(
                outcome.json.len() <= limit,
                "limit {limit}: kept {} bytes",
                outcome.json.len()
            );
            // Valid UTF-8 by construction in Rust; assert the cut is where we
            // expect rather than mid-sequence.
            assert!(
                outcome.json.is_char_boundary(outcome.json.len()),
                "limit {limit}: cut mid-character"
            );
        }
    }

    // -- ambient capabilities --

    /// Nothing registers these; the assertion guards against a dependency
    /// bump quietly introducing one.
    #[tokio::test]
    async fn host_capabilities_are_absent() {
        for global in [
            "fetch",
            "require",
            "process",
            "XMLHttpRequest",
            "WebSocket",
            "globalThis.os",
            "globalThis.std",
        ] {
            let out = eval(&format!("typeof {global}")).await.unwrap();
            assert_eq!(out, json!("undefined"), "{global} should not exist");
        }
    }

    #[tokio::test]
    async fn modules_cannot_be_imported() {
        let err = eval(r#"import("fs")"#).await.unwrap_err();
        assert!(
            matches!(err, SandboxError::ScriptError { .. }),
            "got: {err:?}"
        );
    }

    /// Each call gets a fresh runtime, so nothing a script leaves behind can
    /// influence the next one.
    #[tokio::test]
    async fn state_does_not_leak_between_scripts() {
        let sandbox = Sandbox::new(fast());
        sandbox.eval("globalThis.leaked = 1").await.unwrap();
        let out = sandbox.eval("typeof globalThis.leaked").await.unwrap();
        assert_eq!(out, json!("undefined"));
    }
}
