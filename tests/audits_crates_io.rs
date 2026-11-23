//! Revocations of crates loaded from cargo's registry cache rather than from
//! the workspace.
//!
//! Each test runs in a child process with its own fake registry cache supplied
//! through `CARGO_HOME` before startup, without changing the parent's environment.

use std::future::Future;
use std::path::{Path, PathBuf};

use symposium::audits::AuditIndex;
use symposium::audits::digest::payload_digest;
use symposium::config::{Symposium, UseEntry};
use symposium::pm::PackageId;
use symposium::status_command::{StatusEntry, StatusState};
use symposium_testlib::{TestContext, TestMode, with_fixture};

/// Put `name-version` into the fake cargo registry cache the way cargo leaves
/// an extracted crate, with one skill whose body is `body`, and return it.
fn cache_crate(cargo_home: &Path, name: &str, version: &str, body: &str) -> PathBuf {
    let dir = cargo_home
        .join("registry/src/index.crates.io-1949cf8c6b5b557f")
        .join(format!("{name}-{version}"));
    std::fs::create_dir_all(dir.join("skills/guide")).unwrap();
    std::fs::write(dir.join(".cargo-ok"), "{\"v\":1}").unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2021\"\n"),
    )
    .unwrap();
    std::fs::write(
        dir.join("skills/guide/SKILL.md"),
        format!("---\nname: guide\ndescription: guidance\n---\n{body}\n"),
    )
    .unwrap();
    dir
}

/// A private cache containing a subcommand and two distinct widget payloads.
/// `crate-y` is cached nowhere.
fn fake_cargo_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let x = cache_crate(home.path(), "crate-x", "2.0.0", "x");
    // A subcommand running a binary that is always on PATH under
    // `cargo test`.
    std::fs::write(
        x.join("Symposium.toml"),
        "[[installations]]\nname = \"x\"\nexecutable = \"rustc\"\n\n\
             [subcommand.xtool]\ndescription = \"x\"\ncommand = \"x\"\n",
    )
    .unwrap();
    cache_crate(home.path(), "widget", "1.5.3", "bad");
    cache_crate(home.path(), "widget", "1.6.0", "ok");
    home
}

/// Run the assertions only in a child whose Cargo environment is already set.
async fn with_cargo_home(test_name: &str, test: impl Future<Output = ()>) {
    const CHILD_ROOT: &str = "SYMPOSIUM_AUDIT_TEST_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        test.await;
        std::fs::write(PathBuf::from(root).join("checked"), "").unwrap();
        return;
    }

    let home = fake_cargo_home();
    let before = std::env::var_os("CARGO_HOME");
    let output = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD_ROOT, home.path())
        .env("CARGO_HOME", home.path())
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{test_name} child failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        home.path().join("checked").is_file(),
        "child must execute {test_name}'s assertions\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::env::var_os("CARGO_HOME"),
        before,
        "the parent environment must remain unchanged"
    );
}

/// Turn the builtin audits list on and write `audits/cargo/<name>.toml` into
/// its cache directory.
fn write_audits(sym: &mut Symposium, name: &str, text: &str) {
    sym.config.defaults.symposium_audits = true;
    let dir = sym.audits_dir().unwrap().join("audits/cargo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.toml")), text).unwrap();
}

/// Whether crate-x's subcommand dispatches. Subcommand dispatch resolves the
/// active plugin set with `UpdateLevel::None`, cargo's cache only, exactly like
/// the hook path.
///
/// (`sync` would be the more direct probe, but `sync::sync` resolves the active
/// set with `UpdateLevel::Check`, which for a crate the workspace does not
/// depend on means a probe project and the network, something a test cannot
/// rely on. The activation logic, including the revocation check, is the same
/// code.)
async fn crate_x_dispatches(ctx: &mut TestContext) -> bool {
    match ctx.symposium(&["xtool", "--version"]).await {
        Ok(out) => {
            assert!(out.contains("rustc"), "{out}");
            true
        }
        Err(_) => false,
    }
}

/// The `status` line for `name`.
async fn status(ctx: &TestContext, name: &str) -> StatusEntry {
    let ws = ctx.sym.workspace(ctx.workspace_root.as_ref().unwrap());
    symposium::status_command::workspace_status(&ctx.sym, &ws)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("no status entry for `{name}`"))
}

/// A revocation applies to a `use`d crate the workspace does not depend on,
/// and `status` shows its version and the reason; revoking a `use`d crate
/// that is cached nowhere is no error.
#[tokio::test]
async fn a_used_crate_that_is_not_a_dependency_can_be_revoked() {
    with_cargo_home(
        "a_used_crate_that_is_not_a_dependency_can_be_revoked",
        check_used_crate_revocation(),
    )
    .await;
}

async fn check_used_crate_revocation() {
    with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.sym.config.plugins.used = vec![
            UseEntry::Global("crate-x".into()),
            UseEntry::Global("crate-y".into()),
        ];

        // Without a revocation the cached crate loads.
        assert!(crate_x_dispatches(&mut ctx).await, "cached and not revoked");
        assert_eq!(status(&ctx, "crate-x").await.state, StatusState::Active);

        for krate in ["crate-x", "crate-y"] {
            write_audits(
                &mut ctx.sym,
                krate,
                "format = 1\n[[revoked]]\nversions = \"*\"\nreason = \"remote-exec\"\n",
            );
        }
        assert!(
            !crate_x_dispatches(&mut ctx).await,
            "a revoked crate is withheld"
        );
        let x = status(&ctx, "crate-x").await;
        assert_eq!(x.state, StatusState::Revoked, "{x:?}");
        assert_eq!(x.version.as_deref(), Some("2.0.0"));
        assert_eq!(
            x.root,
            "revoked by the `symposium-audits` list: it tells the agent to fetch and run remote code"
        );
        // crate-y is in no cache, so its release is unknown and it falls
        // through to the ordinary line.
        let y = status(&ctx, "crate-y").await;
        assert_eq!(y.state, StatusState::Dormant, "{y:?}");
        assert!(y.root.contains("cached crate"), "{y:?}");
        assert_eq!(y.version, None);
        Ok(())
    })
    .await
    .unwrap();
}

/// A digest revocation is checked against the payload of the release that
/// loaded, not a newer cached one.
#[tokio::test]
async fn a_digest_revocation_checks_the_release_that_loaded() {
    with_cargo_home(
        "a_digest_revocation_checks_the_release_that_loaded",
        check_digest_revocation(),
    )
    .await;
}

async fn check_digest_revocation() {
    let bad = PathBuf::from(std::env::var_os("CARGO_HOME").unwrap())
        .join("registry/src/index.crates.io-1949cf8c6b5b557f/widget-1.5.3");
    let config = tempfile::tempdir().unwrap();
    let mut sym = Symposium::from_dir(config.path());
    write_audits(
        &mut sym,
        "widget",
        &format!(
            "format = 1\n[[revoked]]\ndigest = \"{}\"\nreason = \"remote-exec\"\n",
            payload_digest(&bad).unwrap()
        ),
    );
    let index = AuditIndex::configured(&sym);
    let pms = sym.detached_managers();
    let revoked = async |version: &str| {
        index
            .revocation(&pms, &PackageId::new("cargo", "widget", version))
            .await
            .is_some()
    };

    // 1.5.3 carries the revoked payload, even though a newer release that
    // `^1.5.3` would accept is cached beside it.
    assert!(
        revoked("1.5.3").await,
        "the loaded release's payload is revoked"
    );
    // 1.6.0 carries another payload.
    assert!(!revoked("1.6.0").await);
    // A release that is not cached has no payload to match, rather than
    // borrowing a newer release's.
    assert!(!revoked("1.5.2").await);
}
