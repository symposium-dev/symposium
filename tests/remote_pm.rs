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

// --- The real cargo PM, out of process -------------------------------------
//
// The tests above use shell fixtures to exercise the protocol. These drive the
// actual `symposium-pm-cargo` binary over a real Cargo workspace, which is the
// only way to know the extraction genuinely holds: that the cargo PM needs
// nothing from Symposium's process to answer.

/// A minimal cargo workspace whose one dependency ships a skill.
///
/// `widget` lives *outside* `root` on purpose: cargo silently promotes a path
/// dependency inside the workspace directory to a workspace member, and a
/// member is not a dependency.
fn cargo_workspace(root: &Path, widget: &Path) {
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"app\"]\n",
    )
    .unwrap();

    std::fs::create_dir_all(root.join("app/src")).unwrap();
    std::fs::write(
        root.join("app/Cargo.toml"),
        format!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
             [dependencies]\nwidget = {{ path = \"{}\" }}\n",
            widget.display()
        ),
    )
    .unwrap();
    std::fs::write(root.join("app/src/lib.rs"), "").unwrap();

    std::fs::create_dir_all(widget.join("src")).unwrap();
    std::fs::create_dir_all(widget.join("skills/usage")).unwrap();
    std::fs::write(
        widget.join("Cargo.toml"),
        "[package]\nname = \"widget\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(widget.join("src/lib.rs"), "").unwrap();
    std::fs::write(
        widget.join("skills/usage/SKILL.md"),
        "---\nname: usage\ndescription: how to use widget\n---\nBody.\n",
    )
    .unwrap();
}

/// The real binary, bound to `workspace` through the `initialize` handshake.
fn cargo_pm(workspace: &Path, cache: &Path) -> RemotePm {
    RemotePm::new(
        "cargo",
        RemotePmCommand::new(env!("CARGO_BIN_EXE_symposium-pm-cargo"))
            .workspace(Some(workspace.to_path_buf()))
            .cache_dir(cache.to_path_buf()),
    )
}

#[tokio::test]
async fn the_cargo_pm_answers_from_its_own_process() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    let widget = tmp.path().join("widget");
    std::fs::create_dir_all(&root).unwrap();
    cargo_workspace(&root, &widget);

    let pm = cargo_pm(&root, &tmp.path().join("cache"));

    // Where the workspace is: resolved by the PM, in its own process, from
    // nothing but the path handed over at `initialize`.
    let info = pm.workspace_info().await.unwrap().expect("a workspace");
    assert_eq!(
        std::fs::canonicalize(&info.root).unwrap(),
        std::fs::canonicalize(&root).unwrap()
    );
    let members: Vec<_> = info
        .members
        .iter()
        .filter_map(|m| m.file_name().and_then(|n| n.to_str()))
        .collect();
    assert_eq!(members, vec!["app"], "got members {members:?}");

    // What it depends on.
    let deps = pm.list_deps().await.unwrap();
    assert!(
        deps.iter().any(|d| d.name == "widget" && d.pm == "cargo"),
        "got deps {deps:?}"
    );

    // And the plugin that dependency embeds: a crate with no manifest at all,
    // so the offer carries an empty manifest and its content is the `skills/`
    // directory that validation will add the default group for.
    let offers = pm.active_plugins(&deps).await;
    let widget = offers
        .iter()
        .find(|o| o.id.name == "widget")
        .expect("widget offered");
    assert!(widget.root.join("skills/usage/SKILL.md").is_file());
}

#[tokio::test]
async fn the_cargo_pm_reports_no_workspace_outside_one() {
    let tmp = tempfile::tempdir().unwrap();
    let empty = tmp.path().join("not-a-workspace");
    std::fs::create_dir_all(&empty).unwrap();

    let pm = cargo_pm(&empty, &tmp.path().join("cache"));
    // Absence is an ordinary answer, not an error: symposium runs outside
    // Cargo workspaces and must not treat that as a failure.
    assert_eq!(pm.workspace_info().await.unwrap(), None);
    assert_eq!(pm.list_deps().await.unwrap(), vec![]);
}

// --- `[[package-manager]]` config -------------------------------------------

/// The whole path a configured PM travels: config file → `RemotePm` → a real
/// subprocess → the workspace facts core reads. Naming the entry `cargo`
/// replaces the built-in in-process instance, so this also exercises running
/// the cargo PM out of process end to end.
#[tokio::test]
async fn a_configured_package_manager_replaces_the_builtin_cargo_instance() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    let widget = tmp.path().join("widget");
    std::fs::create_dir_all(&root).unwrap();
    cargo_workspace(&root, &widget);

    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        format!(
            "[[package-manager]]\nname = \"cargo\"\ncommand = {:?}\n",
            env!("CARGO_BIN_EXE_symposium-pm-cargo")
        ),
    )
    .unwrap();

    let sym = symposium::config::Symposium::from_dir(&config_dir);
    assert_eq!(sym.config.package_managers.len(), 1);

    let ws = sym.workspace(&root);
    // Exactly one cargo instance: the configured one displaced the built-in.
    assert_eq!(
        ws.pms()
            .instances()
            .filter(|i| i.name == symposium::pm::CARGO_PM)
            .count(),
        1
    );

    // And it answers, from its own process.
    assert_eq!(
        ws.root().await.map(|r| std::fs::canonicalize(r).unwrap()),
        Some(std::fs::canonicalize(&root).unwrap())
    );
    assert!(
        ws.dep_ids().await.iter().any(|d| d.name == "widget"),
        "got deps {:?}",
        ws.dep_ids().await
    );
}

#[tokio::test]
async fn a_configured_package_manager_that_is_missing_degrades() {
    let tmp = tempfile::tempdir().unwrap();
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        "[[package-manager]]\nname = \"npm\"\ncommand = \"/nonexistent/symposium-pm-npm\"\n",
    )
    .unwrap();

    let sym = symposium::config::Symposium::from_dir(&config_dir);
    let ws = sym.workspace(tmp.path());
    // A broken PM contributes nothing rather than breaking the invocation:
    // the cargo instance is still there and still answers.
    assert!(ws.dep_ids().await.is_empty());
    assert!(ws.pms().instances().any(|i| i.name == "npm"));
}
