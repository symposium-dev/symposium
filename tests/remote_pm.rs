//! End-to-end tests for the out-of-process package-manager protocol.
//!
//! These drive a *real* subprocess rather than a mock, because the things most
//! likely to break at this boundary are the ones a mock cannot reproduce: the
//! handshake, newline framing, a child that dies, a child that never answers.
//!
//! The fixture PMs are shell scripts. A script is enough to exercise the
//! protocol from Symposium's side, and it keeps the test independent of build
//! ordering: no PM binary has to exist before the test can run.

use std::path::Path;

use symposium::pm::{
    OfferKind, PackageId, PackageManager, PmInstance, PmRegistry, RemotePm, RemotePmCommand,
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

/// A PM that answers the handshake and offers one plugin whose manifest it
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
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocol_version":1,"name":"demo","capabilities":["list_deps"]}}}}\n' "$id"
      ;;
    *'"active_plugins"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"offers":[{{"id":{{"pm":"demo","name":"widget","version":"1.0.0"}},"root":"{root}","manifest":{{"name":"widget","depends-on":["serde"],"skills":[{{"source":{{"path":"skills"}}}}]}}}}]}}}}\n' "$id"
      ;;
    *'"load_plugin"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"offers":[]}}}}\n' "$id"
      ;;
    *'"list_deps"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"deps":[{{"pm":"demo","name":"widget","version":"1.0.0"}}]}}}}\n' "$id"
      ;;
    *'"fetch"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"id":{{"pm":"demo","name":"widget","version":"1.0.0"}},"root":"{root}"}}}}\n' "$id"
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

    // The raw trait yields offers: an id, a root, and an unvalidated manifest.
    let offers = pm.active_plugins(&[]).await;
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].id.name, "widget");
    assert_eq!(offers[0].manifest.name.as_deref(), Some("widget"));

    // Through an instance, the same offer is validated under symposium's
    // policy and its `source.path` group resolves against the offer root.
    let registry = PmRegistry::new(vec![PmInstance {
        name: "demo".to_string(),
        trusted: true,
        kind: OfferKind::Registry,
        pm: Box::new(remote("demo", &synthesizing_pm(tmp.path(), &content))),
    }]);
    let loaded = registry
        .instances()
        .next()
        .unwrap()
        .active_plugins(&[])
        .await;
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].plugin.name, "widget");
    // It named a dependency, so it is gated rather than dormant.
    assert!(!loaded[0].plugin.requires_use);
    assert!(loaded[0].plugin.predicates.references_dep("serde"));
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

#[tokio::test]
async fn a_pm_that_exits_immediately_contributes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let pm = remote(
        "demo",
        &script(tmp.path(), "pm-dead.sh", "#!/bin/sh\nexit 1\n"),
    );

    // Degrades to empty rather than propagating: one broken PM must never
    // abort a sync or a hook.
    assert!(pm.active_plugins(&[]).await.is_empty());
    assert!(
        pm.load_plugin(&PackageId::any_version("demo", "widget"))
            .await
            .is_empty()
    );
    // The typed operations do surface the error, for callers that can report it.
    assert!(pm.list_deps().await.is_err());
}

#[tokio::test]
async fn a_pm_that_returns_garbage_contributes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let pm = remote(
        "demo",
        &script(
            tmp.path(),
            "pm-garbage.sh",
            "#!/bin/sh\nwhile IFS= read -r line; do echo 'not json'; done\n",
        ),
    );
    assert!(pm.active_plugins(&[]).await.is_empty());
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
    assert!(pm.active_plugins(&[]).await.is_empty());
}

#[tokio::test]
async fn a_missing_binary_contributes_nothing() {
    let pm = RemotePm::new(
        "demo",
        RemotePmCommand::new("/nonexistent/symposium-pm-nope"),
    );
    assert!(pm.active_plugins(&[]).await.is_empty());
}
