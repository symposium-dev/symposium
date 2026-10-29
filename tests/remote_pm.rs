//! End-to-end tests for the out-of-process package-manager protocol.
//!
//! These drive a *real* subprocess rather than a mock, because the things most
//! likely to break at this boundary are the ones a mock cannot reproduce: the
//! handshake, newline framing, a child that dies, a child that never answers.
//!
//! The fixture PMs are shell scripts. A script is enough to exercise the
//! protocol from Symposium's side, and it keeps the test independent of build
//! ordering: no PM binary has to exist before the test can run. They run
//! through `/bin/sh` and splice paths into JSON unescaped, so these tests are
//! Unix-only.
#![cfg(unix)]

use std::path::Path;

use symposium::pm::{
    PackageId, PackageManager, PluginKind, PmInstance, PmRegistry, RemotePm, RemotePmCommand,
};
use symposium_install::UpdateLevel;

/// Write a shell script. Deliberately *not* marked executable: the fixtures run
/// as `sh <path>`, which sidesteps the ETXTBSY race you get from writing a file
/// and exec'ing it while other threads are forking.
fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

/// A PM that answers the handshake and returns one plugin whose manifest it
/// makes up: the synthesis case the whole boundary exists for.
fn synthesizing_pm(dir: &Path, root: &Path) -> std::path::PathBuf {
    script(
        dir,
        "pm-synth.sh",
        &format!(
            r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed 's/.*"id":\([0-9]*\),"method".*/\1/')
  case "$line" in
    *'"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocol_version":1,"name":"demo"}}}}\n' "$id"
      ;;
    *'"active_plugins"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"plugins":[{{"id":{{"pm":"demo","name":"widget","version":"1.0.0"}},"root":"{root}","manifest":{{"name":"widget","depends-on":["serde"],"skills":[{{"source":{{"path":"skills"}}}}]}}}}]}}}}\n' "$id"
      ;;
    *'"load_plugin"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"plugins":[]}}}}\n' "$id"
      ;;
    *'"list_deps"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"deps":[{{"pm":"demo","name":"widget","version":"1.0.0"}}]}}}}\n' "$id"
      ;;
    *'"fetch"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"id":{{"pm":"demo","name":"widget","version":"1.0.0"}},"root":"{root}"}}}}\n' "$id"
      ;;
    *'"refresh"'*'"update":"check"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"refreshed":true}}}}\n' "$id"
      ;;
    *'"refresh"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"refreshed":false}}}}\n' "$id"
      ;;
    *)
      printf '{{"jsonrpc":"2.0","id":%s,"error":{{"code":-32601,"message":"unknown method"}}}}\n' "$id"
      ;;
  esac
done
"#,
            root = root.display()
        ),
    )
}

/// A remote PM driving `sh <script>`.
fn remote(name: &str, script: &Path) -> RemotePm {
    RemotePm::new(
        name,
        RemotePmCommand::new("/bin/sh").arg(script.display().to_string()),
    )
}

#[tokio::test]
async fn a_subprocess_pm_offers_a_synthesized_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let content = tmp.path().join("content");
    std::fs::create_dir_all(content.join("skills/usage")).unwrap();
    std::fs::write(
        content.join("skills/usage/SKILL.md"),
        "---\nname: usage\ndescription: d\n---\nBody.\n",
    )
    .unwrap();

    let pm = remote("demo", &synthesizing_pm(tmp.path(), &content));

    // The raw trait yields unvalidated plugins: an id, a root, and a raw
    // manifest.
    let listing = pm.active_plugins(&[]).await.unwrap();
    assert!(listing.is_complete());
    let unvalidated = listing.plugins;
    assert_eq!(unvalidated.len(), 1);
    assert_eq!(unvalidated[0].id.name, "widget");
    assert_eq!(unvalidated[0].manifest.name.as_deref(), Some("widget"));

    // Through an instance, the same plugin is validated under symposium's
    // policy and its `source.path` group resolves against the plugin's root.
    let registry = PmRegistry::new(vec![PmInstance::new(
        "demo",
        true,
        PluginKind::Registry,
        Box::new(remote("demo", &synthesizing_pm(tmp.path(), &content))),
    )]);
    let loaded = registry
        .instances()
        .next()
        .unwrap()
        .active_plugins(&[])
        .await;
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].manifest.name, "widget");
    // It named a dependency, so it is gated rather than dormant.
    assert!(!loaded[0].manifest.requires_use);
    assert!(loaded[0].manifest.predicates.references_dep("serde"));
}

#[tokio::test]
async fn list_deps_and_fetch_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let content = tmp.path().join("content");
    std::fs::create_dir_all(&content).unwrap();
    let pm = remote("demo", &synthesizing_pm(tmp.path(), &content));

    let deps = pm.list_deps().await.unwrap();
    assert_eq!(deps, vec![PackageId::new("demo", "widget", "1.0.0")]);

    let fetched = pm
        .fetch(&PackageId::any_version("demo", "widget"), UpdateLevel::None)
        .await
        .unwrap();
    assert_eq!(fetched.id.version, "1.0.0");
    assert_eq!(fetched.root, content);
}

/// `refresh` is a wire request like the rest, so a registry served from
/// another process is pulled by `plugin sync` too.
#[tokio::test]
async fn refresh_round_trips() {
    let tmp = tempfile::tempdir().unwrap();
    let content = tmp.path().join("content");
    std::fs::create_dir_all(&content).unwrap();
    let pm = remote("demo", &synthesizing_pm(tmp.path(), &content));

    assert!(pm.refresh(UpdateLevel::Check).await.unwrap());
    assert!(!pm.refresh(UpdateLevel::None).await.unwrap());
}

/// An error response is an answer, not a broken process: `use` searching
/// offline must not leave the sync that follows without the PM.
#[tokio::test]
async fn an_error_response_leaves_the_pm_usable() {
    let tmp = tempfile::tempdir().unwrap();
    let content = tmp.path().join("content");
    std::fs::create_dir_all(&content).unwrap();
    let pm = remote("demo", &synthesizing_pm(tmp.path(), &content));

    // The fixture does not implement `search`, so it answers with an error.
    let err = pm.search("widget").await.unwrap_err();
    assert!(format!("{err:#}").contains("unknown method"), "{err:#}");
    assert_eq!(
        pm.list_deps().await.unwrap(),
        vec![PackageId::new("demo", "widget", "1.0.0")]
    );
}

/// A PM that cannot be reached answers every operation with an error, which
/// the instance above it records as that PM failing and degrades to nothing:
/// one broken PM must never abort a sync or a hook, and must not read as a PM
/// that offers nothing.
#[tokio::test]
async fn a_pm_that_exits_immediately_fails_rather_than_answering_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dead = || {
        remote(
            "demo",
            &script(tmp.path(), "pm-dead.sh", "#!/bin/sh\nexit 1\n"),
        )
    };

    let pm = dead();
    assert!(pm.active_plugins(&[]).await.is_err());
    assert!(
        pm.load_plugin(&PackageId::any_version("demo", "widget"), UpdateLevel::None)
            .await
            .is_err()
    );
    assert!(pm.list_deps().await.is_err());

    let instance = PmInstance::new("demo", false, PluginKind::Package, Box::new(dead()));
    assert!(instance.active_plugins(&[]).await.is_empty());
    assert!(instance.has_failed());
}

#[tokio::test]
async fn a_pm_that_never_answers_times_out() {
    let tmp = tempfile::tempdir().unwrap();
    let pm = RemotePm::new(
        "demo",
        RemotePmCommand::new("/bin/sh")
            .arg(
                script(
                    tmp.path(),
                    "pm-silent.sh",
                    "#!/bin/sh\nwhile IFS= read -r line; do :; done\n",
                )
                .display()
                .to_string(),
            )
            .timeout(std::time::Duration::from_millis(200)),
    );

    let started = std::time::Instant::now();
    assert!(pm.active_plugins(&[]).await.is_err());
    // Marked dead, so the next call fails fast instead of waiting again.
    let err = pm.list_deps().await.unwrap_err();
    assert!(format!("{err:#}").contains("timed out"), "{err:#}");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[tokio::test]
async fn a_pm_that_returns_garbage_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let pm = remote(
        "demo",
        &script(
            tmp.path(),
            "pm-garbage.sh",
            "#!/bin/sh\nwhile IFS= read -r line; do echo 'not json'; done\n",
        ),
    );
    assert!(pm.active_plugins(&[]).await.is_err());
}

#[tokio::test]
async fn a_name_mismatch_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    // Routing is by the `pm` component of an id, so a PM answering to a
    // different name than it was registered under would silently misroute.
    let pm = remote(
        "demo",
        &script(
            tmp.path(),
            "pm-liar.sh",
            r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed 's/.*"id":\([0-9]*\),"method".*/\1/')
  printf '{"jsonrpc":"2.0","id":%s,"result":{"protocol_version":1,"name":"somethingelse"}}\n' "$id"
done
"#,
        ),
    );
    let err = pm.list_deps().await.unwrap_err();
    assert!(
        format!("{err:#}").contains("unavailable"),
        "expected the PM to be marked unavailable, got: {err:#}"
    );
}

#[tokio::test]
async fn a_protocol_version_mismatch_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let pm = remote(
        "demo",
        &script(
            tmp.path(),
            "pm-future.sh",
            r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed 's/.*"id":\([0-9]*\),"method".*/\1/')
  printf '{"jsonrpc":"2.0","id":%s,"result":{"protocol_version":99,"name":"demo"}}\n' "$id"
done
"#,
        ),
    );
    assert!(pm.active_plugins(&[]).await.is_err());
}

#[tokio::test]
async fn a_missing_binary_fails() {
    let pm = RemotePm::new(
        "demo",
        RemotePmCommand::new("/nonexistent/symposium-pm-nope"),
    );
    assert!(pm.active_plugins(&[]).await.is_err());
}
