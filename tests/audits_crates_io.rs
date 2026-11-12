//! Revocations of crates loaded from cargo's registry cache rather than from
//! the workspace.
//!
//! That cache is found through `CARGO_HOME`, which is process-global, so these
//! tests live in their own binary and share one fake cache, set up once
//! before either reads the environment.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use symposium::audits::AuditIndex;
use symposium::audits::digest::payload_digest;
use symposium::config::{Symposium, UseEntry};
use symposium::pm::PackageId;
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

/// The fake cache, created and named in `CARGO_HOME` by whichever test asks
/// first: `crate-x` 2.0.0, which vends a subcommand, and two releases of
/// `widget` with different payloads. `crate-y` is cached nowhere.
fn cargo_home() -> &'static Path {
    static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    HOME.get_or_init(|| {
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
        // SAFETY: each test calls this before anything else, and the other
        // waits on the lock until it is done, so nothing reads the
        // environment concurrently.
        unsafe { std::env::set_var("CARGO_HOME", home.path()) };
        home
    })
    .path()
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

/// A revocation applies to a `use`d crate the workspace does not depend on;
/// revoking a `use`d crate that is cached nowhere is no error.
#[tokio::test]
async fn a_used_crate_that_is_not_a_dependency_can_be_revoked() {
    cargo_home();
    with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.sym.config.plugins.used = vec![
            UseEntry::Global("crate-x".into()),
            UseEntry::Global("crate-y".into()),
        ];

        // Without a revocation the cached crate loads.
        assert!(crate_x_dispatches(&mut ctx).await, "cached and not revoked");

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
        Ok(())
    })
    .await
    .unwrap();
}

/// A digest revocation is checked against the payload of the release that
/// loaded, not a newer cached one.
#[tokio::test]
async fn a_digest_revocation_checks_the_release_that_loaded() {
    let bad = cargo_home().join("registry/src/index.crates.io-1949cf8c6b5b557f/widget-1.5.3");
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
