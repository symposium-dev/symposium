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
             [[mcp_servers]]\nname = \"sqlx\"\ncommand = {:?}\n\
             args = [\"--config\", {:?}]\n",
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
