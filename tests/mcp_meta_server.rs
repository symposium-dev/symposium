//! `cargo agents mcp-serve` spoken to as a real MCP client over stdio.
//!
//! The unit tests cover what the server decides; these cover that a client
//! can actually talk to it — process spawn, handshake, framing, and the
//! stdout discipline the transport depends on.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::transport::TokioChildProcess;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cargo-agents"))
}

/// Point the binary at an empty config directory, so a developer's own
/// settings cannot change what a test sees.
fn isolated_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

async fn connect(home: &tempfile::TempDir) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let mut command = tokio::process::Command::new(binary());
    command
        .arg("mcp-serve")
        .env("SYMPOSIUM_HOME", home.path())
        .stderr(Stdio::null());

    let transport = TokioChildProcess::new(command).expect("spawn mcp-serve");
    tokio::time::timeout(Duration::from_secs(30), ().serve(transport))
        .await
        .expect("handshake timed out")
        .expect("handshake failed")
}

/// The point of the design: an agent sees two tools, not every plugin
/// server's tools.
#[tokio::test(flavor = "multi_thread")]
async fn advertises_two_tools() {
    let home = isolated_home();
    let client = connect(&home).await;

    let mut names: Vec<String> = client
        .list_all_tools()
        .await
        .expect("tools/list")
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    names.sort();

    assert_eq!(names, vec!["execute".to_string(), "list_tools".to_string()]);
    let _ = client.cancel().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_itself_as_symposium() {
    let home = isolated_home();
    let client = connect(&home).await;

    let info = client.peer_info().expect("server info");
    let server_info = info.server_info.as_ref().expect("server implementation");
    assert_eq!(server_info.name, "symposium");
    assert!(
        info.capabilities.tools.is_some(),
        "tools capability must be advertised"
    );
    let _ = client.cancel().await;
}

/// A client filtering on read-only annotations must not mistake arbitrary
/// code execution for a safe operation.
#[tokio::test(flavor = "multi_thread")]
async fn execute_is_annotated_as_destructive() {
    let home = isolated_home();
    let client = connect(&home).await;

    let tools = client.list_all_tools().await.expect("tools/list");
    let execute = tools
        .iter()
        .find(|t| t.name.as_ref() == "execute")
        .expect("execute tool");
    let annotations = execute.annotations.as_ref().expect("annotations");

    assert_eq!(annotations.read_only_hint, Some(false));
    assert_eq!(annotations.destructive_hint, Some(true));
    let _ = client.cancel().await;
}

/// With nothing applicable, the tools still answer — and say so, rather than
/// failing in a way that reads as a broken connection.
#[tokio::test(flavor = "multi_thread")]
async fn list_tools_answers_when_nothing_applies() {
    let home = isolated_home();
    let client = connect(&home).await;

    let result = client
        .call_tool(CallToolRequestParams::new("list_tools"))
        .await
        .expect("list_tools should answer");

    assert_ne!(result.is_error, Some(true), "answering is not an error");
    let _ = client.cancel().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_tool_names_the_available_ones() {
    let home = isolated_home();
    let client = connect(&home).await;

    let err = client
        .call_tool(CallToolRequestParams::new("nonexistent"))
        .await
        .expect_err("an unknown tool is a protocol error");
    let text = err.to_string();

    assert!(
        text.contains("list_tools") && text.contains("execute"),
        "the error should name what is available, got: {text}"
    );
    let _ = client.cancel().await;
}

/// A workspace with one real backing MCP server behind a plugin manifest.
struct Workspace {
    _dir: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
}

fn mock_binary() -> PathBuf {
    let mut dir = binary();
    dir.pop();
    dir.join("examples").join("mock-mcp-server")
}

fn workspace_with_backing_server() -> Workspace {
    workspace_serving(serde_json::json!({
        "name": "sqlx",
        "tools": [
            {"name": "query", "description": "Run a SQL query",
             "inputSchema": {"type": "object",
                "properties": {"sql": {"type": "string"}}, "required": ["sql"]},
             "behavior": {"kind": "echo"}},
            {"name": "migrate-status", "description": "Show migrations",
             "behavior": {"kind": "text", "text": "up to date"}}
        ]
    }))
}

fn workspace_serving(mock: serde_json::Value) -> Workspace {
    workspace_serving_as("sqlx", mock)
}

/// As [`workspace_serving`], with the manifest's server name spelled out. That
/// name, not the one the mock reports in its handshake, is the namespace a
/// script addresses.
fn workspace_serving_as(server: &str, mock: serde_json::Value) -> Workspace {
    let dir = tempfile::tempdir().expect("temp dir");
    let base = dir.path().to_path_buf();
    let home = base.join("home");
    let root = base.join("ws");
    std::fs::create_dir_all(home.join("plugins/db")).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();

    let mock_config = base.join("mock.json");
    std::fs::write(&mock_config, mock.to_string()).unwrap();

    std::fs::write(
        home.join("config.toml"),
        "hook-scope = \"project\"\n[defaults]\nsymposium-recommendations = false\n",
    )
    .unwrap();
    std::fs::write(
        home.join("plugins/db/SYMPOSIUM.toml"),
        format!(
            "name = \"db-plugin\"\ndepends-on = [\"*\"]\n\n\
             [[mcp_servers]]\nname = {:?}\ncommand = {:?}\n\
             args = [\"--config\", {:?}]\n",
            server,
            mock_binary().display().to_string(),
            mock_config.display().to_string(),
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"e2e\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
         [dependencies]\nserde = \"1\"\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "// lib\n").unwrap();

    Workspace {
        _dir: dir,
        home,
        root,
    }
}

async fn connect_in(workspace: &Workspace) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let mut command = tokio::process::Command::new(binary());
    command
        .arg("mcp-serve")
        .current_dir(&workspace.root)
        .env("SYMPOSIUM_HOME", &workspace.home)
        .stderr(Stdio::null());

    let transport = TokioChildProcess::new(command).expect("spawn");
    tokio::time::timeout(Duration::from_secs(60), ().serve(transport))
        .await
        .expect("handshake timed out")
        .expect("handshake failed")
}

fn text_of(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| match block {
            rmcp::model::ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tools of a workspace's backing servers, discovered through the plugin
/// registry and a live `tools/list`.
#[tokio::test(flavor = "multi_thread")]
async fn lists_tools_of_a_backing_server() {
    let workspace = workspace_with_backing_server();
    let client = connect_in(&workspace).await;

    let result = client
        .call_tool(CallToolRequestParams::new("list_tools"))
        .await
        .expect("list_tools");
    let text = text_of(&result);

    assert!(text.contains("sqlx:"), "got: {text}");
    assert!(text.contains("query"), "got: {text}");
    assert!(text.contains("migrate-status"), "got: {text}");
    let _ = client.cancel().await;
}

/// Naming a server asks for its signatures, without a second round trip.
#[tokio::test(flavor = "multi_thread")]
async fn naming_a_server_returns_declarations() {
    let workspace = workspace_with_backing_server();
    let client = connect_in(&workspace).await;

    let result = client
        .call_tool(CallToolRequestParams::new("list_tools").with_arguments(
            serde_json::Map::from_iter([("servers".to_string(), serde_json::json!(["sqlx"]))]),
        ))
        .await
        .expect("list_tools");
    let text = text_of(&result);

    assert!(text.contains("declare const sqlx"), "got: {text}");
    assert!(text.contains("Promise<unknown>"), "got: {text}");
    assert!(
        text.contains(r#""migrate-status""#) && text.contains("migrate_status"),
        "both spellings should be declared, got: {text}"
    );
    let _ = client.cancel().await;
}

/// The design's whole claim: several tool calls, the filtering between them,
/// and one round trip — with intermediate data never reaching the agent.
#[tokio::test(flavor = "multi_thread")]
async fn a_script_composes_calls_in_one_round_trip() {
    let workspace = workspace_with_backing_server();
    let client = connect_in(&workspace).await;

    let script = r#"
        const a = await sqlx.query({ sql: "SELECT 1" });
        console.log("intermediate", a);
        const b = await sqlx["migrate-status"]();
        return { echoed: a.sql, status: b };
    "#;
    let result = client
        .call_tool(CallToolRequestParams::new("execute").with_arguments(
            serde_json::Map::from_iter([("script".to_string(), serde_json::json!(script))]),
        ))
        .await
        .expect("execute");
    let text = text_of(&result);

    assert_ne!(result.is_error, Some(true), "got: {text}");
    assert!(text.contains(r#""echoed":"SELECT 1""#), "got: {text}");
    assert!(text.contains(r#""status":"up to date""#), "got: {text}");
    assert!(
        text.contains("console:") && text.contains("intermediate"),
        "console output should be reported, got: {text}"
    );
    let _ = client.cancel().await;
}

/// Every name in the declarations has to dispatch to the tool it was declared
/// for, including a sanitized alias that collided with a real tool's name.
#[tokio::test(flavor = "multi_thread")]
async fn a_declared_name_reaches_the_tool_it_was_declared_for() {
    let workspace = workspace_serving(serde_json::json!({
        "name": "sqlx",
        "tools": [
            {"name": "get-sum", "description": "hyphenated",
             "behavior": {"kind": "text", "text": "FROM-HYPHENATED"}},
            {"name": "get_sum", "description": "underscored",
             "behavior": {"kind": "text", "text": "FROM-UNDERSCORED"}}
        ]
    }));
    let client = connect_in(&workspace).await;

    let declarations = text_of(
        &client
            .call_tool(CallToolRequestParams::new("list_tools").with_arguments(
                serde_json::Map::from_iter([("detail".to_string(), serde_json::json!("full"))]),
            ))
            .await
            .expect("list_tools"),
    );

    let script = r#"
        return {
            viaAlias: await sqlx.get_sum_2(),
            viaQuoted: await sqlx["get-sum"](),
            viaOwnName: await sqlx.get_sum(),
        };
    "#;
    let result = client
        .call_tool(CallToolRequestParams::new("execute").with_arguments(
            serde_json::Map::from_iter([("script".to_string(), serde_json::json!(script))]),
        ))
        .await
        .expect("execute");
    let text = text_of(&result);

    assert!(
        declarations.contains("get_sum_2"),
        "the alias should be declared, got: {declarations}"
    );
    assert_ne!(result.is_error, Some(true), "got: {text}");
    assert!(
        text.contains(r#""viaAlias":"FROM-HYPHENATED""#),
        "the declared alias must reach the hyphenated tool, got: {text}"
    );
    assert!(
        text.contains(r#""viaQuoted":"FROM-HYPHENATED""#),
        "got: {text}"
    );
    assert!(
        text.contains(r#""viaOwnName":"FROM-UNDERSCORED""#),
        "a real tool must keep its own name, got: {text}"
    );
    let _ = client.cancel().await;
}

/// A script that touches one server must not start the others, or it pays
/// for -- and can be blocked by -- servers it never mentions.
#[tokio::test(flavor = "multi_thread")]
async fn a_script_starts_only_the_servers_it_calls() {
    let dir = tempfile::tempdir().expect("temp dir");
    let base = dir.path().to_path_buf();
    let home = base.join("home");
    let root = base.join("ws");
    let started = base.join("started.log");
    std::fs::create_dir_all(home.join("plugins/db")).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();

    let mut configs = Vec::new();
    for name in ["used", "unused"] {
        let path = base.join(format!("{name}.json"));
        std::fs::write(
            &path,
            serde_json::json!({
                "name": name,
                "startup_log": started,
                "tools": [
                    {"name": "ping", "description": "answer",
                     "behavior": {"kind": "text", "text": name}}
                ]
            })
            .to_string(),
        )
        .unwrap();
        configs.push(path);
    }

    let mut manifest = String::from("name = \"db-plugin\"\ndepends-on = [\"*\"]\n");
    for (name, config) in ["used", "unused"].iter().zip(&configs) {
        manifest.push_str(&format!(
            "\n[[mcp_servers]]\nname = {:?}\ncommand = {:?}\nargs = [\"--config\", {:?}]\n",
            name,
            mock_binary().display().to_string(),
            config.display().to_string(),
        ));
    }
    std::fs::write(home.join("plugins/db/SYMPOSIUM.toml"), manifest).unwrap();
    std::fs::write(
        home.join("config.toml"),
        "hook-scope = \"project\"\n[defaults]\nsymposium-recommendations = false\n",
    )
    .unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"e2e\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
         [dependencies]\nserde = \"1\"\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "// lib\n").unwrap();

    let workspace = Workspace {
        _dir: dir,
        home,
        root,
    };
    let client = connect_in(&workspace).await;

    let result = client
        .call_tool(CallToolRequestParams::new("execute").with_arguments(
            serde_json::Map::from_iter([(
                "script".to_string(),
                serde_json::json!("return await used.ping();"),
            )]),
        ))
        .await
        .expect("execute");
    let text = text_of(&result);
    assert!(text.contains("used"), "got: {text}");

    let log = std::fs::read_to_string(&started).unwrap_or_default();
    assert!(
        log.contains("used"),
        "the called server should have started"
    );
    assert!(
        !log.contains("unused"),
        "a server the script never named was started: {log}"
    );
    let _ = client.cancel().await;
}

/// A dependency added mid-session exposes its tools without a restart, and a
/// server that is still applicable is carried across rather than restarted --
/// its startup log must still show a single start.
#[tokio::test(flavor = "multi_thread")]
async fn a_dependency_added_mid_session_appears() {
    let dir = tempfile::tempdir().expect("temp dir");
    let base = dir.path().to_path_buf();
    let home = base.join("home");
    let root = base.join("ws");
    let started = base.join("started.log");
    std::fs::create_dir_all(home.join("plugins/db")).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();

    let mut manifest = String::from("name = \"db-plugin\"\ndepends-on = [\"*\"]\n");
    for (name, dep) in [("always", "serde"), ("later", "regex")] {
        let config = base.join(format!("{name}.json"));
        std::fs::write(
            &config,
            serde_json::json!({
                "name": name,
                "startup_log": started,
                "tools": [
                    {"name": "ping", "description": "answer",
                     "behavior": {"kind": "text", "text": name}}
                ]
            })
            .to_string(),
        )
        .unwrap();
        manifest.push_str(&format!(
            "\n[[mcp_servers]]\nname = {:?}\ndepends-on = [{:?}]\ncommand = {:?}\n\
             args = [\"--config\", {:?}]\n",
            name,
            dep,
            mock_binary().display().to_string(),
            config.display().to_string(),
        ));
    }
    std::fs::write(home.join("plugins/db/SYMPOSIUM.toml"), manifest).unwrap();
    std::fs::write(
        home.join("config.toml"),
        "hook-scope = \"project\"\n[defaults]\nsymposium-recommendations = false\n",
    )
    .unwrap();

    let cargo_toml = root.join("Cargo.toml");
    std::fs::write(
        &cargo_toml,
        "[package]\nname = \"e2e\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
         [dependencies]\nserde = \"1\"\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "// lib\n").unwrap();

    let workspace = Workspace {
        _dir: dir,
        home,
        root: root.clone(),
    };
    let client = connect_in(&workspace).await;

    let first = text_of(
        &client
            .call_tool(CallToolRequestParams::new("list_tools"))
            .await
            .expect("list_tools"),
    );
    assert!(first.contains("always"), "got: {first}");
    assert!(
        !first.contains("later"),
        "not a dependency yet, got: {first}"
    );

    // Add the dependency the second server is gated on, exactly as `cargo
    // add` would, and let cargo rewrite the lock file.
    std::fs::write(
        &cargo_toml,
        "[package]\nname = \"e2e\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
         [dependencies]\nserde = \"1\"\nregex = \"1\"\n",
    )
    .unwrap();
    let generated = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&root)
        .output();
    if !generated.map(|o| o.status.success()).unwrap_or(false) {
        eprintln!("skipping: cargo could not resolve `regex` offline");
        let _ = client.cancel().await;
        return;
    }

    let second = text_of(
        &client
            .call_tool(CallToolRequestParams::new("list_tools"))
            .await
            .expect("list_tools"),
    );
    assert!(
        second.contains("later"),
        "a newly applicable server should appear, got: {second}"
    );

    let log = std::fs::read_to_string(&started).unwrap_or_default();
    assert_eq!(
        log.matches("always").count(),
        1,
        "the still-applicable server was restarted: {log}"
    );
    let _ = client.cancel().await;
}

/// A limit the script exceeded is reported in a form it can act on, rather
/// than as prose it has to interpret.
#[tokio::test(flavor = "multi_thread")]
async fn exceeding_a_limit_reports_a_tagged_error() {
    let workspace = workspace_with_backing_server();
    std::fs::write(
        workspace.home.join("config.toml"),
        "hook-scope = \"project\"\n[defaults]\nsymposium-recommendations = false\n\
         [mcp]\nscript-timeout-secs = 2\ntool-call-timeout-secs = 1\n",
    )
    .unwrap();
    let client = connect_in(&workspace).await;

    let result = client
        .call_tool(CallToolRequestParams::new("execute").with_arguments(
            serde_json::Map::from_iter([(
                "script".to_string(),
                serde_json::json!("while (true) {}"),
            )]),
        ))
        .await
        .expect("execute should answer, not fail");
    let text = text_of(&result);

    assert_eq!(result.is_error, Some(true), "got: {text}");
    assert!(text.contains("script_timeout"), "got: {text}");
    let _ = client.cancel().await;
}

/// A script can end with work outstanding: a promise nothing settles, or a
/// tool call it forgot to await. Both must still produce a reply.
#[tokio::test(flavor = "multi_thread")]
async fn a_script_left_pending_still_answers() {
    let workspace = workspace_with_backing_server();
    std::fs::write(
        workspace.home.join("config.toml"),
        "hook-scope = \"project\"\n[defaults]\nsymposium-recommendations = false\n\
         [mcp]\nscript-timeout-secs = 2\ntool-call-timeout-secs = 1\n",
    )
    .unwrap();
    let client = connect_in(&workspace).await;

    for script in [
        "return new Promise(() => {});",
        // Dispatched but never awaited.
        "sqlx.query({ sql: \"SELECT 1\" }); return \"done\";",
    ] {
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            client.call_tool(CallToolRequestParams::new("execute").with_arguments(
                serde_json::Map::from_iter([("script".to_string(), serde_json::json!(script))]),
            )),
        )
        .await
        .unwrap_or_else(|_| panic!("execute never answered for: {script}"))
        .expect("execute should answer, not fail");

        let text = text_of(&result);
        assert!(
            text.contains("script_timeout") || result.is_error != Some(true),
            "expected an answer either way, got: {text}"
        );
    }
    let _ = client.cancel().await;
}

/// The transport is newline-delimited JSON, so anything else written to
/// stdout corrupts the stream. Reporting output is the likely offender, since
/// every other subcommand sends it there.
#[tokio::test(flavor = "multi_thread")]
async fn stdout_carries_only_json_rpc() {
    let home = isolated_home();

    let mut child = tokio::process::Command::new(binary())
        .arg("mcp-serve")
        // Verbose reporting would go to stdout for any other subcommand.
        .arg("--verbose")
        .env("SYMPOSIUM_HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");

    use tokio::io::AsyncWriteExt;
    let mut stdin = child.stdin.take().expect("stdin");
    stdin
        .write_all(
            concat!(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}"#,
                "\n",
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                "\n",
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                "\n",
            )
            .as_bytes(),
        )
        .await
        .expect("write");
    drop(stdin);

    let output = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .expect("server should exit on stdin close")
        .expect("output");

    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();

    assert_eq!(lines.len(), 2, "one response per request, got: {stdout}");
    for line in lines {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("stdout line is not JSON ({e}): {line}"));
    }
}

/// An install command prints, and under `mcp-serve` stdout is the protocol.
#[tokio::test(flavor = "multi_thread")]
async fn install_command_output_stays_off_stdout() {
    const MARKER: &str = "SYMPOSIUM-INSTALL-STDOUT-MARKER";

    let home = isolated_home();
    let plugin_dir = home.path().join("plugins").join("noisy");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir");
    std::fs::write(
        plugin_dir.join("SYMPOSIUM.toml"),
        format!(
            "name = \"noisy\"\n\
             depends-on = [\"*\"]\n\
             \n\
             [[installations]]\n\
             name = \"warmup\"\n\
             install_commands = [\"echo {MARKER}\"]\n\
             \n\
             [[mcp_servers]]\n\
             name = \"noisy-server\"\n\
             depends-on = [\"*\"]\n\
             command = \"/usr/bin/true\"\n\
             requirements = [\"warmup\"]\n"
        ),
    )
    .expect("write manifest");

    // Server resolution needs a Rust workspace to condition on.
    let workspace = tempfile::tempdir().expect("workspace");
    std::fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");
    std::fs::create_dir_all(workspace.path().join("src")).expect("src dir");
    std::fs::write(workspace.path().join("src/lib.rs"), "").expect("write lib.rs");

    let mut child = tokio::process::Command::new(binary())
        .arg("mcp-serve")
        .current_dir(workspace.path())
        .env("SYMPOSIUM_HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");

    use tokio::io::AsyncWriteExt;
    let mut stdin = child.stdin.take().expect("stdin");
    stdin
        .write_all(
            concat!(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}"#,
                "\n",
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                "\n",
                // Starting the server is what acquires its requirements.
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_tools","arguments":{"detail":"full"}}}"#,
                "\n",
            )
            .as_bytes(),
        )
        .await
        .expect("write");
    drop(stdin);

    let output = tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())
        .await
        .expect("server should exit on stdin close")
        .expect("output");

    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    assert!(
        !stdout.contains(MARKER),
        "install command output reached the protocol stream:\n{stdout}"
    );
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("stdout line is not JSON ({e}): {line}"));
    }
}

// -- declared output schemas --

/// A workspace whose backing server declares an output schema on some tools.
///
/// `echo` returns the arguments as structured content, so a script decides what
/// the server sends back. That is what lets one mock cover both a conforming
/// answer and a violating one.
fn workspace_with_typed_server() -> Workspace {
    workspace_serving_as(
        "typed",
        serde_json::json!({
        "name": "typed",
        "tools": [
            {"name": "count", "description": "Count things",
             "inputSchema": {"type": "object",
                "properties": {"count": {"type": "number"}}, "required": ["count"]},
             "outputSchema": {"type": "object",
                "properties": {"count": {"type": "number"}}, "required": ["count"]},
             "behavior": {"kind": "echo"}},
            {"name": "count_items", "description": "A snake_case tool name",
             "inputSchema": {"type": "object", "properties": {"bin": {"type": "string"}}},
             "behavior": {"kind": "echo"}},
            {"name": "untyped", "description": "Declares no output shape",
             "inputSchema": {"type": "object", "properties": {"a": {"type": "string"}}},
             "behavior": {"kind": "echo"}},
            {"name": "unstructured", "description": "Declares a shape, answers with text",
             "inputSchema": {"type": "object"},
             "outputSchema": {"type": "object",
                "properties": {"count": {"type": "number"}}, "required": ["count"]},
             "behavior": {"kind": "text", "text": "count: 3"}}
        ]
        }),
    )
}

async fn execute(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    script: &str,
) -> rmcp::model::CallToolResult {
    client
        .call_tool(CallToolRequestParams::new("execute").with_arguments(
            serde_json::Map::from_iter([("script".to_string(), serde_json::json!(script))]),
        ))
        .await
        .expect("execute should answer, not fail")
}

/// The declared shape reaches the model as the tool's return type.
#[tokio::test(flavor = "multi_thread")]
async fn a_declared_output_schema_is_shown_as_the_return_type() {
    let workspace = workspace_with_typed_server();
    let client = connect_in(&workspace).await;

    let result = client
        .call_tool(CallToolRequestParams::new("list_tools").with_arguments(
            serde_json::Map::from_iter([("servers".to_string(), serde_json::json!(["typed"]))]),
        ))
        .await
        .expect("list_tools");
    let text = text_of(&result);

    assert!(
        text.contains("count(params: {") && text.contains("): Promise<{"),
        "the typed tool should declare its return shape, got: {text}"
    );
    assert!(
        text.contains("untyped(params?: {\n    a?: string;\n  }): Promise<unknown>;"),
        "a tool without an output schema stays unknown, got: {text}"
    );
    let _ = client.cancel().await;
}

/// A conforming answer passes through untouched.
#[tokio::test(flavor = "multi_thread")]
async fn a_conforming_result_is_returned() {
    let workspace = workspace_with_typed_server();
    let client = connect_in(&workspace).await;

    let result = execute(&client, r#"return await typed.count({ count: 3 });"#).await;
    let text = text_of(&result);

    assert_ne!(result.is_error, Some(true), "got: {text}");
    assert!(text.contains(r#""count":3"#), "got: {text}");
    let _ = client.cancel().await;
}

/// A server that declares a shape and sends another does not fail the call.
/// The value stands and the model is told the shape did not hold, because the
/// value is often still readable and only the model can decide.
#[tokio::test(flavor = "multi_thread")]
async fn a_violating_result_is_passed_through_with_a_notice() {
    let workspace = workspace_with_typed_server();
    let client = connect_in(&workspace).await;

    // `echo` reflects the arguments, so this makes the server answer with a
    // string where its own schema promised a number.
    let result = execute(&client, r#"return await typed.count({ count: "three" });"#).await;
    let text = text_of(&result);

    assert_ne!(
        result.is_error,
        Some(true),
        "a schema mismatch must not fail the call, got: {text}"
    );
    assert!(
        text.contains(r#""count":"three""#),
        "the value should reach the script unchanged, got: {text}"
    );
    assert!(
        text.contains("[typed.count: result off-shape, treat as unknown]"),
        "the model should be tagged, tersely, got: {text}"
    );
}

/// The case from the requirement: a tool declares `{ count: number }` and
/// answers `"count: 3"` as text. Ordinary MCP, so the text is handed over for
/// the model to read rather than refused.
#[tokio::test(flavor = "multi_thread")]
async fn an_unstructured_answer_is_handed_over_to_be_read() {
    let workspace = workspace_with_typed_server();
    let client = connect_in(&workspace).await;

    let result = execute(&client, r#"return await typed.unstructured();"#).await;
    let text = text_of(&result);

    assert_ne!(
        result.is_error,
        Some(true),
        "text where an object was declared must not fail, got: {text}"
    );
    assert!(
        text.contains("count: 3"),
        "the script should receive the text, got: {text}"
    );
    assert!(
        text.contains("[typed.unstructured: result off-shape, treat as unknown]"),
        "the model should be tagged, tersely, got: {text}"
    );
}

/// A mismatch is not an exception, so a script that destructures the declared
/// shape keeps running and simply finds nothing there.
#[tokio::test(flavor = "multi_thread")]
async fn a_violation_does_not_throw_in_the_script() {
    let workspace = workspace_with_typed_server();
    let client = connect_in(&workspace).await;

    let script = r#"
        const r = await typed.unstructured();
        return { raw: r, declared: r.count ?? "absent", threw: false };
    "#;
    let result = execute(&client, script).await;
    let text = text_of(&result);

    assert_ne!(result.is_error, Some(true), "got: {text}");
    assert!(text.contains(r#""threw":false"#), "got: {text}");
    assert!(
        text.contains(r#""declared":"absent""#),
        "destructuring the declared shape should find nothing, got: {text}"
    );
    assert!(text.contains(r#""raw":"count: 3""#), "got: {text}");
}

/// One tool answering off-shape repeatedly is one fact about that tool. The
/// notice must not be repeated per call, or a loop would crowd out the result.
#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_violation_is_reported_once() {
    let workspace = workspace_with_typed_server();
    let client = connect_in(&workspace).await;

    let script = r#"
        for (let i = 0; i < 4; i++) { await typed.unstructured(); }
        return "done";
    "#;
    let result = execute(&client, script).await;
    let text = text_of(&result);

    assert_ne!(result.is_error, Some(true), "got: {text}");
    assert_eq!(
        text.matches("result off-shape").count(),
        1,
        "the tag should appear once, got: {text}"
    );
}

/// A tool that declares nothing is unaffected: no shape was promised, so
/// nothing is enforced.
#[tokio::test(flavor = "multi_thread")]
async fn an_untyped_tool_is_not_checked() {
    let workspace = workspace_with_typed_server();
    let client = connect_in(&workspace).await;

    let result = execute(&client, r#"return await typed.untyped({ a: "anything" });"#).await;
    let text = text_of(&result);

    assert_ne!(result.is_error, Some(true), "got: {text}");
    assert!(text.contains(r#""a":"anything""#), "got: {text}");
    let _ = client.cancel().await;
}

/// Tool names come from third-party servers and none in the wild are camelCase,
/// so a model reaching for one out of TypeScript habit must still land the call.
#[tokio::test(flavor = "multi_thread")]
async fn a_camel_case_spelling_reaches_a_snake_case_tool() {
    let workspace = workspace_with_typed_server();
    let client = connect_in(&workspace).await;

    let result = execute(&client, r#"return await typed.countItems({ bin: "A1" });"#).await;
    let text = text_of(&result);

    assert_ne!(result.is_error, Some(true), "got: {text}");
    assert!(text.contains(r#""bin":"A1""#), "got: {text}");
    let _ = client.cancel().await;
}

/// The declared spelling is what the model is shown, and it keeps working.
#[tokio::test(flavor = "multi_thread")]
async fn the_declared_spelling_still_reaches_its_tool() {
    let workspace = workspace_with_typed_server();
    let client = connect_in(&workspace).await;

    let result = execute(&client, r#"return await typed.count_items({ bin: "A1" });"#).await;
    let text = text_of(&result);

    assert_ne!(result.is_error, Some(true), "got: {text}");
    assert!(text.contains(r#""bin":"A1""#), "got: {text}");
    let _ = client.cancel().await;
}

/// Tolerant lookup must not invent tools: an unknown name still fails, with the
/// existing did-you-mean rather than a silent wrong call.
#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_tool_name_still_fails() {
    let workspace = workspace_with_typed_server();
    let client = connect_in(&workspace).await;

    let result = execute(&client, r#"return await typed.deleteEverything();"#).await;
    let text = text_of(&result);

    assert_eq!(result.is_error, Some(true), "got: {text}");
    let _ = client.cancel().await;
}

/// Only declared names appear; a tolerated spelling is not advertised.
#[tokio::test(flavor = "multi_thread")]
async fn an_alternate_spelling_is_not_declared() {
    let workspace = workspace_with_typed_server();
    let client = connect_in(&workspace).await;

    let result = client
        .call_tool(CallToolRequestParams::new("list_tools").with_arguments(
            serde_json::Map::from_iter([("servers".to_string(), serde_json::json!(["typed"]))]),
        ))
        .await
        .expect("list_tools");
    let text = text_of(&result);

    assert!(text.contains("count_items("), "got: {text}");
    assert!(
        !text.contains("countItems"),
        "the camelCase spelling must not be declared, got: {text}"
    );
    let _ = client.cancel().await;
}
