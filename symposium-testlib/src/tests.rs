use super::*;

const CHILD_ROOT: &str = "SYMPOSIUM_ACP_ENV_TEST_ROOT";

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
    let agent = sacp_tokio::AcpAgent::from_args([missing_agent.display()]).unwrap();
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
