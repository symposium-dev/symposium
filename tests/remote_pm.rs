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

/// A PM is asked to canonicalize each name once; its answer is kept for the
/// connection.
#[tokio::test]
async fn a_pm_canonicalizes_each_name_once() {
    let tmp = tempfile::tempdir().unwrap();
    let asked = tmp.path().join("asked");
    let folding = script(
        tmp.path(),
        "pm-folding.sh",
        &format!(
            r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed 's/.*"id":\([0-9]*\),"method".*/\1/')
  case "$line" in
    *'"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocol_version":1,"name":"demo"}}}}\n' "$id" ;;
    *'"canonical_name"'*)
      name=$(printf '%s' "$line" | sed 's/.*"name":"\([^"]*\)".*/\1/')
      echo "$name" >> "{asked}"
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"name":"%s"}}}}\n' "$id" "$(printf '%s' "$name" | tr - _)" ;;
  esac
done
"#,
            asked = asked.display()
        ),
    );
    let pm = remote("demo", &folding);
    for name in ["widget-lib", "widget-lib", "serde"] {
        let expected = name.replace('-', "_");
        assert_eq!(pm.canonical_name(name).await.unwrap(), expected);
    }
    assert_eq!(
        std::fs::read_to_string(&asked).unwrap(),
        "widget-lib\nserde\n"
    );
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

    // What the workspace depends on: resolved by the PM, in its own process,
    // from nothing but the path handed over at `initialize`.
    let deps = pm.list_deps().await.unwrap();
    // Cargo's names, read across the boundary.
    assert_eq!(pm.canonical_name("widget-lib").await.unwrap(), "widget_lib");
    assert!(
        deps.iter().any(|d| d.name == "widget" && d.pm == "cargo"),
        "got deps {deps:?}"
    );

    // What it sees there: the workspace's root and member, and the lockfile
    // whose change means its answers may have changed.
    let project = pm.project_info().await.unwrap();
    assert_eq!(
        project.root.map(|r| std::fs::canonicalize(r).unwrap()),
        Some(std::fs::canonicalize(&root).unwrap())
    );
    let members: Vec<_> = project
        .local_packages
        .iter()
        .filter_map(|m| m.file_name().and_then(|n| n.to_str()))
        .collect();
    assert_eq!(members, vec!["app"], "got members {members:?}");
    assert!(
        project.inputs.iter().any(|i| i.ends_with("Cargo.lock")),
        "got inputs {:?}",
        project.inputs
    );

    // And the plugin that dependency embeds: a crate with no manifest at all,
    // so the plugin carries an empty manifest and its content is the `skills/`
    // directory that validation will add the default group for.
    let plugins = pm.active_plugins(&deps).await.unwrap().plugins;
    let widget = plugins
        .iter()
        .find(|o| o.id.name == "widget")
        .expect("widget offered");
    assert!(widget.root.join("skills/usage/SKILL.md").is_file());
}

#[tokio::test]
async fn the_cargo_pm_reports_no_deps_outside_a_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let empty = tmp.path().join("not-a-workspace");
    std::fs::create_dir_all(&empty).unwrap();

    let pm = cargo_pm(&empty, &tmp.path().join("cache"));
    // Absence is an ordinary answer, not an error: symposium runs outside
    // Cargo workspaces and must not treat that as a failure.
    assert_eq!(pm.list_deps().await.unwrap(), vec![]);
}

// --- `[[package-manager]]` config -------------------------------------------

/// The whole path a configured PM travels: config file → `RemotePm` → a real
/// subprocess → the dependencies core reads. Naming the entry `cargo`
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

    // The workspace is Symposium's own reading; what it depends on is the PM's
    // answer, from its own process.
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
    let root = tmp.path().join("ws");
    let widget = tmp.path().join("widget");
    std::fs::create_dir_all(&root).unwrap();
    cargo_workspace(&root, &widget);

    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        "[[package-manager]]\nname = \"npm\"\ncommand = \"/nonexistent/symposium-pm-npm\"\n",
    )
    .unwrap();

    let sym = symposium::config::Symposium::from_dir(&config_dir);
    let ws = sym.workspace(&root);
    // A broken PM contributes nothing rather than breaking the invocation:
    // the cargo instance is still there and its dependencies still count.
    assert!(ws.pms().instances().any(|i| i.name == "npm"));
    assert!(
        ws.dep_ids().await.iter().any(|d| d.name == "widget"),
        "got deps {:?}",
        ws.dep_ids().await
    );
}

/// A workspace-independent operation (registry listing, search) tells a
/// spawned PM it has no workspace, rather than handing it an empty path to go
/// looking in.
#[tokio::test]
async fn a_detached_operation_hands_the_pm_no_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    // Answers to its configured name only when `initialize` named no
    // workspace, and is refused for a name mismatch otherwise.
    let pm = script(
        tmp.path(),
        "pm-detached.sh",
        r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed 's/.*"id":\([0-9]*\),"method".*/\1/')
  case "$line" in
    *'"initialize"'*'"workspace"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocol_version":1,"name":"given-a-workspace"}}\n' "$id"
      ;;
    *'"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocol_version":1,"name":"demo"}}\n' "$id"
      ;;
    *)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"deps":[{"pm":"demo","name":"widget","version":"1.0.0"}]}}\n' "$id"
      ;;
  esac
done
"#,
    );
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        format!(
            "[[package-manager]]\nname = \"demo\"\ncommand = \"/bin/sh\"\nargs = [{:?}]\n",
            pm.display().to_string()
        ),
    )
    .unwrap();
    let sym = symposium::config::Symposium::from_dir(&config_dir);

    let detached = sym.detached_managers();
    assert!(detached.list_deps().await.iter().any(|d| d.pm == "demo"));
    // The same PM, bound to a directory, is told about it.
    let bound = sym.workspace(tmp.path());
    assert!(!bound.dep_ids().await.iter().any(|d| d.pm == "demo"));
}

/// An `npm` package manager whose project at `ws` depends on `widget`, whose
/// content (at `content`) carries a `skills/` directory and no manifest.
fn npm_workspace_pm(dir: &Path, ws: &Path, content: &Path) -> std::path::PathBuf {
    let plugin = format!(
        r#"{{"id":{{"pm":"npm","name":"widget","version":"1.0.0"}},"root":"{}","manifest":{{}}}}"#,
        content.display()
    );
    script(
        dir,
        "pm-npm.sh",
        &format!(
            r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed 's/.*"id":\([0-9]*\),"method".*/\1/')
  case "$line" in
    *'"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocol_version":1,"name":"npm"}}}}\n' "$id" ;;
    *'"project_info"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"project":{{"root":"{ws}"}}}}}}\n' "$id" ;;
    *'"list_deps"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"deps":[{{"pm":"npm","name":"widget","version":"1.0.0"}}]}}}}\n' "$id" ;;
    *'"active_plugins"'*|*'"load_plugin"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"plugins":[{plugin}]}}}}\n' "$id" ;;
    *)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"plugins":[]}}}}\n' "$id" ;;
  esac
done
"#,
            ws = ws.display(),
        ),
    )
}

/// The consent path holds for a configured package manager exactly as it does
/// for cargo: the plugin a dependency embeds is a candidate until consented
/// to, under its own package manager's name, and then activates.
#[tokio::test]
async fn a_configured_pms_dependency_plugin_activates_once_consented() {
    let tmp = tempfile::tempdir().unwrap();
    let ws_root = tmp.path().join("ws");
    let content = tmp.path().join("widget");
    std::fs::create_dir_all(&ws_root).unwrap();
    std::fs::create_dir_all(content.join("skills/usage")).unwrap();
    std::fs::write(
        content.join("skills/usage/SKILL.md"),
        "---\nname: usage\ndescription: d\n---\nBody.\n",
    )
    .unwrap();
    let pm = npm_workspace_pm(tmp.path(), &ws_root, &content);

    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let write_config = |plugins: &str| {
        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                "[[agent]]\nname = \"claude\"\n\n\
                 [defaults]\nsymposium-recommendations = false\nuser-plugins = false\n\n\
                 [[package-manager]]\nname = \"npm\"\ncommand = \"/bin/sh\"\nargs = [{:?}]\n\n{plugins}",
                pm.display().to_string()
            ),
        )
        .unwrap();
    };

    // Whether a sync installs the package's skill.
    async fn syncs_npm_skill(config_dir: &Path, ws_root: &Path) -> bool {
        let sym = symposium::config::Symposium::from_dir(config_dir);
        let ws = sym.workspace(ws_root);
        symposium::sync::sync(&sym, &ws, symposium::UpdateLevel::None)
            .await
            .unwrap();
        ws_root.join(".claude/skills/usage/SKILL.md").is_file()
    }

    // Undecided: offered as a candidate, not active.
    write_config("");
    let sym = symposium::config::Symposium::from_dir(&config_dir);
    let ws = sym.workspace(&ws_root);
    let found = symposium::discovery::discover_for(&sym, &ws).await;
    assert_eq!(found.candidates.len(), 1, "{found:?}");
    assert_eq!(found.candidates[0].id.pm, "npm");
    assert!(!syncs_npm_skill(&config_dir, &ws_root).await);

    // Consent to the crate of that name is not consent to the npm package.
    write_config("[plugins]\nauto-enable = [{ pm = \"cargo\", name = \"widget\" }]\n");
    assert!(!syncs_npm_skill(&config_dir, &ws_root).await);

    write_config("[plugins]\nauto-enable = [{ pm = \"npm\", name = \"widget\" }]\n");
    assert!(syncs_npm_skill(&config_dir, &ws_root).await);

    // Nothing else here is called `widget`, so the bare name means it.
    write_config("[plugins]\nauto-enable = [\"widget\"]\n");
    assert!(syncs_npm_skill(&config_dir, &ws_root).await);
}

/// A package manager that appends a line to `log` each time it starts, finds
/// a project at `ws`, and names `input` as the one file its answers depend on.
fn logging_pm(dir: &Path, log: &Path, ws: &Path, input: &Path) -> std::path::PathBuf {
    script(
        dir,
        "pm-logging.sh",
        &format!(
            r#"#!/bin/sh
echo started >> "{log}"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed 's/.*"id":\([0-9]*\),"method".*/\1/')
  case "$line" in
    *'"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocol_version":1,"name":"npm"}}}}\n' "$id" ;;
    *'"project_info"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"project":{{"root":"{ws}","inputs":["{input}"]}}}}}}\n' "$id" ;;
    *'"list_deps"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"deps":[]}}}}\n' "$id" ;;
    *)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"plugins":[]}}}}\n' "$id" ;;
  esac
done
"#,
            log = log.display(),
            ws = ws.display(),
            input = input.display(),
        ),
    )
}

/// Deciding whether to sync reads the modification times recorded at the last
/// sync, so an event where nothing changed starts no package manager. Once an
/// input changes, the next event syncs, and asks it again.
#[tokio::test]
async fn an_event_where_nothing_changed_spawns_no_package_manager() {
    let tmp = tempfile::tempdir().unwrap();
    let ws_root = tmp.path().join("ws");
    std::fs::create_dir_all(&ws_root).unwrap();
    let log = tmp.path().join("spawns.log");
    let input = ws_root.join("package-lock.json");
    let pm = logging_pm(tmp.path(), &log, &ws_root, &input);

    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        format!(
            "[defaults]\nsymposium-recommendations = false\nuser-plugins = false\n\n\
             [[package-manager]]\nname = \"npm\"\ncommand = \"/bin/sh\"\nargs = [{:?}]\n",
            pm.display().to_string()
        ),
    )
    .unwrap();

    let agent = symposium::hook_schema::HookAgent::Claude;
    let event = symposium::hook_schema::HookEvent::PreToolUse;
    let step = symposium_testlib::HookStep::PreToolUse {
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({}),
    };
    let payload = symposium::hook_schema::agent_event(agent, event)
        .unwrap()
        .translate_input(&step.to_input_event(&ws_root.to_string_lossy()))
        .to_string()
        .unwrap();
    let spawned = |sym: &symposium::config::Symposium| {
        let sym = sym.clone();
        let payload = payload.clone();
        let log = log.clone();
        async move {
            let _ = std::fs::remove_file(&log);
            symposium::hook::execute_hook(&sym, agent, event, &payload)
                .await
                .unwrap();
            log.exists()
        }
    };
    let sym = symposium::config::Symposium::from_dir(&config_dir);

    assert!(spawned(&sym).await, "the first event syncs, asking the PM");
    assert!(!spawned(&sym).await, "nothing changed, so nothing is asked");
    std::fs::write(&input, "{}").unwrap();
    assert!(spawned(&sym).await, "an input appeared, so the event syncs");
}
