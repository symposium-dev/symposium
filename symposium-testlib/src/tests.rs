use super::*;

const CHILD_ROOT: &str = "SYMPOSIUM_ACP_ENV_TEST_ROOT";

#[tokio::test]
async fn acp_session_collects_streamed_response_until_end_of_turn() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let called = AtomicBool::new(false);
    let ctx = setup_fixture(&[]).await;
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run_with_acp_session(streaming_agent(), ctx, &async |mut ctx| {
            let result = ctx.prompt("Say hello").await?;
            assert_eq!(result.response.as_deref(), Some("Hello, world!"));
            called.store(true, Ordering::Relaxed);
            Ok(())
        }),
    )
    .await
    .expect("the ACP turn must finish")
    .unwrap();
    assert!(called.load(Ordering::Relaxed));
}

/// An in-process peer that streams two text chunks and completes the turn.
fn streaming_agent() -> impl acp::ConnectTo<acp::Client> {
    use acp::schema::{
        ContentBlock, ContentChunk, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptRequest, PromptResponse, SessionNotification, SessionUpdate,
        StopReason, TextContent,
    };
    acp::Agent
        .builder()
        .on_receive_request(
            async |request: InitializeRequest, responder, _| {
                responder.respond(InitializeResponse::new(request.protocol_version))
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async |_: NewSessionRequest, responder, _| {
                responder.respond(NewSessionResponse::new("test-session"))
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async |request: PromptRequest, responder, cx: acp::ConnectionTo<acp::Client>| {
                assert_eq!(request.session_id, "test-session".into());
                assert_eq!(
                    request.prompt,
                    vec![ContentBlock::Text(TextContent::new("Say hello"))]
                );
                for text in ["Hello, ", "world!"] {
                    cx.send_notification(SessionNotification::new(
                        request.session_id.clone(),
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(text.into())),
                    ))?;
                }
                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            acp::on_receive_request!(),
        )
}

/// A failed ACP launch must not leave its configuration in the test process.
#[test]
fn acp_launch_failure_preserves_environment() {
    // Contain the existing environment mutation in a dedicated process.
    let root = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::acp_launch_failure_probe",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ROOT, root.path())
        .env("SYMPOSIUM_HOME", root.path().join("original-home"))
        .env(
            "SYMPOSIUM_HOOK_TRACE",
            root.path().join("original-trace.jsonl"),
        )
        .env(
            "CARGO_BIN_EXE_symposium",
            root.path().join("bin/symposium.exe"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "ACP environment regression failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        root.path().join("checked").is_file(),
        "child must execute the environment assertions\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Invoked only by the parent test with an isolated environment.
#[tokio::test]
#[ignore = "run through acp_launch_failure_preserves_environment"]
async fn acp_launch_failure_probe() {
    let root = PathBuf::from(
        std::env::var_os(CHILD_ROOT).expect("the parent test supplies the child root"),
    );
    let ctx = setup_fixture(&[]).await;
    let missing_agent = root.join("missing-acp-agent");
    assert!(!missing_agent.exists());
    let agent = acp::AcpAgent::from_args([missing_agent.display()]).unwrap();
    let before = ["SYMPOSIUM_HOME", "SYMPOSIUM_HOOK_TRACE", "PATH"]
        .map(|name| (name, std::env::var_os(name)));

    let error = run_with_acp_session(agent, ctx, &async |_| {
        panic!("a nonexistent agent must not start a session")
    })
    .await
    .expect_err("a nonexistent agent must fail to launch");
    eprintln!("expected launch error: {error:#}");

    for (name, value) in before {
        assert_eq!(
            std::env::var_os(name),
            value,
            "ACP launch failure changed {name} in the test process"
        );
    }
    std::fs::write(root.join("checked"), "").unwrap();
}
