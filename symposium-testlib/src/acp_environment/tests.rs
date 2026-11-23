use super::*;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::{Arc, OnceLock};

#[test]
fn session_paths_use_the_config_and_workspace_directories() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let workspace = root.path().join("workspace");
    let environment = values(&root.path().join("bin/symposium"), &config, &workspace).unwrap();

    let environment: BTreeMap<_, _> = environment
        .into_iter()
        .map(|variable| (variable.name, variable.value))
        .collect();
    assert_eq!(Path::new(&environment["SYMPOSIUM_HOME"]), config);
    assert_eq!(
        Path::new(&environment["SYMPOSIUM_HOOK_TRACE"]),
        workspace.join("hook-trace.jsonl")
    );
}

#[test]
fn session_path_prepends_symposium_without_losing_inherited_entries() {
    let root = tempfile::tempdir().unwrap();
    let binary = root.path().join("bin/symposium");
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let environment = values(&binary, root.path(), root.path()).unwrap();
    let path = &environment
        .iter()
        .find(|variable| variable.name == "PATH")
        .unwrap()
        .value;

    let mut entries = std::env::split_paths(path);
    assert_eq!(entries.next().as_deref(), binary.parent());
    assert_eq!(
        entries.collect::<Vec<_>>(),
        std::env::split_paths(&inherited).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn custom_agent_receives_session_settings_without_changing_parent() {
    let root = tempfile::tempdir().unwrap();
    let before = parent_environment();
    let environment = values(
        &root.path().join("bin/symposium"),
        &root.path().join("config"),
        &root.path().join("workspace"),
    )
    .unwrap();
    let received = custom_child_environment(root.path(), environment.clone()).await;

    for variable in environment {
        assert_eq!(
            received[&variable.name], variable.value,
            "child {}",
            variable.name
        );
    }
    assert_eq!(received["ACP_TEST_CUSTOM"], "preserved");
    assert_eq!(parent_environment(), before, "parent environment changed");
}

#[tokio::test]
async fn registry_launch_adds_session_settings_without_changing_the_command() {
    let wrapper = registry_wrapper(vec![EnvVariable::new("ACP_TEST_CUSTOM", "session-value")]);

    let command = capture_registry_launch(wrapper).await;

    assert_eq!(command.program, "npx");
    assert_eq!(command.args, ["-y", "@test/agent@latest", "--acp"]);
    assert!(
        command
            .envs
            .contains(&("ACP_TEST_CUSTOM".into(), "session-value".into()))
    );
}

fn parent_environment() -> BTreeMap<&'static str, Option<OsString>> {
    ["SYMPOSIUM_HOME", "SYMPOSIUM_HOOK_TRACE", "PATH"]
        .map(|name| (name, std::env::var_os(name)))
        .into_iter()
        .collect()
}

/// Run a real child that records its environment, without speaking ACP.
async fn custom_child_environment(
    root: &Path,
    mut environment: Vec<EnvVariable>,
) -> BTreeMap<String, String> {
    let report = root.join("environment.json");
    environment.push(EnvVariable::new(
        "SYMPOSIUM_ACP_ENV_REPORT",
        report.to_str().unwrap(),
    ));
    let executable = std::env::current_exe()
        .unwrap()
        .to_str()
        .unwrap()
        .replace('\\', "/");
    let command = format!(
        "ACP_TEST_CUSTOM=preserved SYMPOSIUM_HOME=overridden \"{executable}\" --exact acp_environment::tests::environment_probe --ignored --nocapture"
    );
    let agent = custom_agent(&command, environment).unwrap();
    let (stdin, stdout, stderr, mut child) = agent.spawn_process().unwrap();
    drop(stdin);
    child.stdout = Some(stdout);
    child.stderr = Some(stderr);
    let output = child.output().await.unwrap();
    assert!(
        output.status.success(),
        "child failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap()
}

#[test]
#[ignore = "run through custom_agent_receives_session_settings_without_changing_parent"]
fn environment_probe() {
    let report = std::env::var_os("SYMPOSIUM_ACP_ENV_REPORT").expect("parent supplies report path");
    let environment: BTreeMap<_, _> = [
        "SYMPOSIUM_HOME",
        "SYMPOSIUM_HOOK_TRACE",
        "PATH",
        "ACP_TEST_CUSTOM",
    ]
    .map(|name| (name, std::env::var(name).unwrap()))
    .into_iter()
    .collect();
    std::fs::write(report, serde_json::to_vec(&environment).unwrap()).unwrap();
}

/// Record the registry's wrapped command, then deliberately fail its launch.
/// This exercises acpr's hook without invoking npx or a model.
async fn capture_registry_launch(
    wrapper: impl acpr::CommandWrapper + Send + Sync + 'static,
) -> acpr::ResolvedCommand {
    let root = tempfile::tempdir().unwrap();
    let registry = root.path().join("registry.json");
    let fixture = serde_json::json!({
        "agents": [{
            "id": "probe",
            "distribution": {
                "npx": {"package": "@test/agent", "args": ["--acp"]}
            }
        }]
    });
    std::fs::write(&registry, fixture.to_string()).unwrap();
    let captured = Arc::new(OnceLock::new());
    let capture = captured.clone();
    let missing = root.path().join("missing-agent");
    let agent = acpr::Acpr::new("probe")
        .with_registry_file(registry)
        .with_cache_dir(root.path().join("cache"))
        .with_command_wrapper(move |command: acpr::ResolvedCommand| {
            let mut command = wrapper.wrap(command);
            capture.get_or_init(|| command.clone());
            command.program = missing.clone().into_os_string();
            command
        });
    let ctx = crate::setup_fixture(&[]).await;
    crate::run_with_acp_session(agent, ctx, &async |_| {
        panic!("a nonexistent agent must not start a session")
    })
    .await
    .expect_err("missing registry agent must fail to launch");
    captured
        .get()
        .expect("registry must call the wrapper")
        .clone()
}
