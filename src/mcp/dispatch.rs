//! Reaching backing servers from inside the sandbox.
//!
//! A script calls `await sqlx.query({...})`. That name is a proxy installed
//! by the host; a property lookup yields a Rust closure that hands the call
//! to whoever is driving the sandbox and waits for the answer.
//!
//! The call crosses a runtime boundary. The engine runs on its own thread
//! with its own current-thread runtime, while backing servers are child
//! processes owned by the main runtime, and their I/O can only be polled
//! there. A channel is what keeps each on the runtime it belongs to.
//!
//! Namespaces are built through the object API rather than by generating
//! JavaScript, so server and tool names — which come from plugin manifests —
//! never reach a code position.
//!
//! Nothing is known about a server until a script names one of its tools, so
//! a script does not wait on servers it never mentions.

use rquickjs::function::{Async, Constructor, Opt};
use rquickjs::{CatchResultExt, Ctx, Function, Object};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

/// A tool call made by a script, waiting on its result.
#[derive(Debug)]
pub struct ToolCall {
    /// Server name as declared by the plugin, not the sanitized spelling.
    pub server: String,
    /// The property name the script used. May be a sanitized alias; the host
    /// resolves it against the server's tool list.
    pub tool: String,
    /// The single argument the script passed, or null.
    pub args: Value,
    pub reply: oneshot::Sender<Result<Value, String>>,
}

/// Channel a sandbox sends its tool calls to.
pub type CallSender = mpsc::UnboundedSender<ToolCall>;
/// The receiving half, serviced by whoever drives the sandbox.
pub type CallReceiver = mpsc::UnboundedReceiver<ToolCall>;

pub fn channel() -> (CallSender, CallReceiver) {
    mpsc::unbounded_channel()
}

/// One backing server as the script sees it.
#[derive(Debug, Clone)]
pub struct Namespace {
    /// Global the object is installed on.
    pub key: String,
    /// Server name to send with each call.
    pub server: String,
}

/// Install one global per namespace.
///
/// A proxy answers any property with a callable, so nothing about a server
/// need be known here. The cost is that a lookup cannot say whether a tool
/// exists; the host decides that when the call arrives.
pub fn install<'js>(
    ctx: &Ctx<'js>,
    namespaces: &[Namespace],
    calls: &CallSender,
) -> Result<(), String> {
    let proxy: Constructor = ctx
        .globals()
        .get("Proxy")
        .catch(ctx)
        .map_err(|e| e.to_string())?;

    for namespace in namespaces {
        let target = Object::new(ctx.clone())
            .catch(ctx)
            .map_err(|e| e.to_string())?;

        let handler = Object::new(ctx.clone())
            .catch(ctx)
            .map_err(|e| e.to_string())?;
        handler
            .set("get", get_trap(ctx, &namespace.server, calls)?)
            .catch(ctx)
            .map_err(|e| e.to_string())?;

        let installed: Object = proxy
            .construct((target, handler))
            .catch(ctx)
            .map_err(|e| e.to_string())?;

        ctx.globals()
            .set(namespace.key.as_str(), installed)
            .catch(ctx)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Property names the proxy must not answer to.
///
/// `then` is why this exists: the engine looks for it on anything awaited, so
/// answering makes the namespace a thenable and `await sqlx` dispatches a
/// tool call named `then`. The rest the runtime reaches for on its own.
fn is_reserved_property(key: &str) -> bool {
    matches!(
        key,
        "then"
            | "catch"
            | "finally"
            | "constructor"
            | "prototype"
            | "__proto__"
            | "toJSON"
            | "toString"
            | "valueOf"
            | "inspect"
    )
}

fn get_trap<'js>(
    ctx: &Ctx<'js>,
    server: &str,
    calls: &CallSender,
) -> Result<Function<'js>, String> {
    let server = server.to_string();
    let calls = calls.clone();

    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>,
              _target: rquickjs::Value<'js>,
              property: rquickjs::Value<'js>|
              -> rquickjs::Result<rquickjs::Value<'js>> {
            let undefined = rquickjs::Value::new_undefined(ctx.clone());

            // A symbol key is the runtime asking about the object itself.
            let Some(name) = property.as_string() else {
                return Ok(undefined);
            };
            let key = name.to_string()?;
            if is_reserved_property(&key) {
                return Ok(undefined);
            }

            let function = tool_function(&ctx, &server, &key, &calls)
                .map_err(|e| rquickjs::Exception::throw_message(&ctx, &e))?;
            Ok(function.into_value())
        },
    )
    .catch(ctx)
    .map_err(|e| e.to_string())
}

fn tool_function<'js>(
    ctx: &Ctx<'js>,
    server: &str,
    tool: &str,
    calls: &CallSender,
) -> Result<Function<'js>, String> {
    let server = server.to_string();
    let tool = tool.to_string();
    let calls = calls.clone();

    Function::new(
        ctx.clone(),
        Async(move |ctx: Ctx<'js>, args: Opt<rquickjs::Value<'js>>| {
            let server = server.clone();
            let tool = tool.clone();
            let calls = calls.clone();
            async move {
                let args = match args.0 {
                    Some(value) => to_json(&ctx, value)?,
                    None => Value::Null,
                };

                let (reply, answer) = oneshot::channel();
                calls
                    .send(ToolCall {
                        server: server.clone(),
                        tool: tool.clone(),
                        args,
                        reply,
                    })
                    .map_err(|_| throw(&ctx, "tool dispatch is no longer available"))?;

                // A dropped sender means the caller abandoned the script;
                // surfacing it as an exception lets the script's own error
                // handling run.
                let result = answer
                    .await
                    .map_err(|_| throw(&ctx, &format!("{server}.{tool} did not answer")))?;

                // A failing tool throws, so a script uses ordinary try/catch
                // rather than inspecting a result shape.
                let value = result.map_err(|message| throw(&ctx, &message))?;
                from_json(&ctx, &value)
            }
        }),
    )
    .catch(ctx)
    .map_err(|e| e.to_string())
}

fn throw(ctx: &Ctx<'_>, message: &str) -> rquickjs::Error {
    rquickjs::Exception::throw_message(ctx, message)
}

/// Convert a JavaScript value to JSON through the engine's own serializer.
fn to_json<'js>(ctx: &Ctx<'js>, value: rquickjs::Value<'js>) -> rquickjs::Result<Value> {
    if value.is_undefined() || value.is_null() {
        return Ok(Value::Null);
    }
    let Some(encoded) = ctx.json_stringify(value)? else {
        return Ok(Value::Null);
    };
    let text = encoded.to_string()?;
    serde_json::from_str(&text).map_err(|_| rquickjs::Error::Unknown)
}

/// Convert JSON back into a JavaScript value.
fn from_json<'js>(ctx: &Ctx<'js>, value: &Value) -> rquickjs::Result<rquickjs::Value<'js>> {
    ctx.json_parse(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::sandbox::{Limits, Sandbox};
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn namespace(server: &str) -> Namespace {
        Namespace {
            key: server.to_string(),
            server: server.to_string(),
        }
    }

    /// Drive a script, answering every tool call with `responder`, and record
    /// what was asked.
    async fn run_with(
        source: &str,
        namespaces: Vec<Namespace>,
        responder: impl Fn(&ToolCall) -> Result<Value, String> + Send + 'static,
    ) -> (Result<Value, String>, Vec<(String, String, Value)>) {
        let (calls, mut receiver) = channel();
        let seen = Arc::new(Mutex::new(Vec::new()));

        let recorded = Arc::clone(&seen);
        let pump = tokio::spawn(async move {
            while let Some(call) = receiver.recv().await {
                recorded.lock().unwrap().push((
                    call.server.clone(),
                    call.tool.clone(),
                    call.args.clone(),
                ));
                let answer = responder(&call);
                let _ = call.reply.send(answer);
            }
        });

        let sandbox = Sandbox::new(Limits {
            timeout: Duration::from_secs(5),
            ..Limits::default()
        });
        let outcome = match sandbox.run_script_with(source, &namespaces, calls).await {
            Ok(o) => o.value().map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };

        pump.await.unwrap();
        let asked = seen.lock().unwrap().clone();
        (outcome, asked)
    }

    #[tokio::test]
    async fn script_calls_a_tool_and_receives_its_result() {
        let (out, asked) = run_with(
            r#"await sqlx.query({ sql: "SELECT 1" })"#,
            vec![namespace("sqlx")],
            |_| Ok(json!({"rows": [{"n": 1}]})),
        )
        .await;

        assert_eq!(out.unwrap(), json!({"rows": [{"n": 1}]}));
        assert_eq!(
            asked,
            vec![(
                "sqlx".to_string(),
                "query".to_string(),
                json!({"sql": "SELECT 1"})
            )]
        );
    }

    /// The point of running code rather than proxying one call at a time:
    /// several calls, and the filtering between them, happen without the
    /// intermediate results ever leaving the sandbox.
    #[tokio::test]
    async fn script_composes_several_calls() {
        let (out, asked) = run_with(
            r#"
            const all = await sqlx.query({ sql: "SELECT" });
            const keep = all.rows.filter(r => r.n > 1);
            const out = [];
            for (const row of keep) {
              out.push(await sqlx.explain({ n: row.n }));
            }
            return out;
            "#,
            vec![namespace("sqlx")],
            |call| match call.tool.as_str() {
                "query" => Ok(json!({"rows": [{"n": 1}, {"n": 2}, {"n": 3}]})),
                _ => Ok(json!({"plan": call.args["n"]})),
            },
        )
        .await;

        assert_eq!(out.unwrap(), json!([{"plan": 2}, {"plan": 3}]));
        assert_eq!(asked.len(), 3, "one query and two explains");
    }

    #[tokio::test]
    async fn tool_failure_throws_into_the_script() {
        let (out, _) = run_with(
            r#"
            try {
              await sqlx.query({});
              return "no throw";
            } catch (e) {
              return "caught: " + e.message;
            }
            "#,
            vec![namespace("sqlx")],
            |_| Err("table not found".to_string()),
        )
        .await;

        assert_eq!(out.unwrap(), json!("caught: table not found"));
    }

    /// An uncaught tool failure ends the script rather than resolving to a
    /// value the model might mistake for success.
    #[tokio::test]
    async fn uncaught_tool_failure_fails_the_script() {
        let (out, _) = run_with(r#"await sqlx.query({})"#, vec![namespace("sqlx")], |_| {
            Err("boom".to_string())
        })
        .await;

        let err = out.unwrap_err();
        assert!(err.contains("boom"), "got: {err}");
    }

    #[tokio::test]
    async fn calling_without_arguments_sends_null() {
        let (out, asked) = run_with("await clock.now()", vec![namespace("clock")], |_| {
            Ok(json!(123))
        })
        .await;

        assert_eq!(out.unwrap(), json!(123));
        assert_eq!(asked[0].2, Value::Null);
    }

    /// Both spellings reach the host as written; mapping an alias to its
    /// wire name needs the tool list, so it happens where that list is.
    #[tokio::test]
    async fn both_spellings_reach_the_host_as_written() {
        let (out, asked) = run_with(
            r#"
            const a = await sqlx["migrate-status"]();
            const b = await sqlx.migrate_status();
            return [a, b];
            "#,
            vec![namespace("sqlx")],
            |_| Ok(json!("ok")),
        )
        .await;

        assert_eq!(out.unwrap(), json!(["ok", "ok"]));
        let keys: Vec<&str> = asked.iter().map(|(_, tool, _)| tool.as_str()).collect();
        assert_eq!(keys, vec!["migrate-status", "migrate_status"]);
    }

    /// A namespace that answers `then` becomes a thenable, and `await sqlx`
    /// dispatches a tool call named `then` instead of resolving.
    #[tokio::test]
    async fn a_namespace_is_not_a_thenable() {
        let (out, asked) = run_with(
            "const v = await sqlx; return typeof v;",
            vec![namespace("sqlx")],
            |_| Ok(json!("ok")),
        )
        .await;

        assert_eq!(out.unwrap(), json!("object"));
        assert!(asked.is_empty(), "awaiting a namespace called: {asked:?}");
    }

    /// A name the script invents still reaches the host, which is where it
    /// can be checked against the real tool list.
    #[tokio::test]
    async fn an_unknown_name_reaches_the_host() {
        let (_, asked) = run_with(
            "try { await sqlx.nonexistent(); } catch (e) {} return 1;",
            vec![namespace("sqlx")],
            |_| Err("no such tool".to_string()),
        )
        .await;

        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].1, "nonexistent");
    }

    #[tokio::test]
    async fn several_servers_are_separate_namespaces() {
        let (out, asked) = run_with(
            "return [await a.ping(), await b.ping()];",
            vec![namespace("a"), namespace("b")],
            |call| Ok(json!(call.server)),
        )
        .await;

        assert_eq!(out.unwrap(), json!(["a", "b"]));
        assert_eq!(asked.len(), 2);
    }

    /// Nothing beyond the declared namespaces appears.
    #[tokio::test]
    async fn undeclared_servers_are_absent() {
        let (out, _) = run_with("typeof other", vec![namespace("sqlx")], |_| Ok(Value::Null)).await;
        assert_eq!(out.unwrap(), json!("undefined"));
    }
}
