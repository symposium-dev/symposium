//! Audits: the payload digest and `plugin digest`.

use std::path::PathBuf;

use symposium::audits::digest::payload_digest;
use symposium_testlib::{TestContext, TestMode, with_fixture};

/// Run `f` with a JSON report layer installed and return the drained events.
async fn with_report<F>(f: F) -> Vec<serde_json::Value>
where
    F: AsyncFnOnce(),
{
    use symposium::report::{ReportLayer, ReportMode};
    use tracing_subscriber::layer::SubscriberExt;

    symposium_testlib::install_tracing_baseline();
    let (layer, handle) = ReportLayer::new(ReportMode::Json, tracing::Level::INFO);
    let subscriber = tracing_subscriber::registry().with(layer);
    let guard = tracing::subscriber::set_default(subscriber);
    f().await;
    drop(guard);
    handle.drain()
}

/// `crate-d`'s source directory in the `audits0` fixture.
fn crate_d(ctx: &TestContext) -> PathBuf {
    ctx.workspace_root.clone().unwrap().join("deps/crate-d")
}

/// `plugin digest` prints the library's digest for a directory, a crate name
/// or `name@version`, says which crate a name resolved to, and fails, saying
/// why, for a payload that has no digest.
#[tokio::test]
async fn plugin_digest_prints_the_digest_or_why_there_is_none() {
    with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
        let expected = payload_digest(&crate_d(&ctx)).unwrap().to_string();
        let digest_event = async |ctx: &mut TestContext, target: &str| {
            let mut result = None;
            let events = with_report(async || {
                result = Some(ctx.symposium(&["plugin", "digest", target]).await);
            })
            .await;
            let event = events.into_iter().find(|e| e["kind"] == "payload_digest");
            (result.unwrap(), event)
        };

        // A crate name, `name@version` (here the release the workspace
        // resolved, served from its own copy), and a directory relative to
        // the working directory.
        for (target, krate) in [
            ("crate-d", Some("crate-d 1.2.0")),
            ("crate-d@1.2.0", Some("crate-d 1.2.0")),
            ("./deps/crate-d", None),
        ] {
            let (result, event) = digest_event(&mut ctx, target).await;
            result?;
            let event = event.expect("a payload_digest event");
            assert_eq!(event["digest"], expected.as_str(), "{target}");
            assert_eq!(event["krate"].as_str(), krate, "{target}");
        }

        let (result, _) = digest_event(&mut ctx, "crate-d@latest").await;
        let err = result.unwrap_err();
        assert!(format!("{err:#}").contains("crate-d@latest"), "{err:#}");

        // A path that is not a directory is never taken for a crate name.
        for target in ["./deps/no-such-crate", "deps/crate-d/Cargo.toml"] {
            let (result, _) = digest_event(&mut ctx, target).await;
            let err = result.unwrap_err();
            assert!(
                format!("{err:#}").contains("is not a directory"),
                "{target}: {err:#}"
            );
        }

        // A payload that cannot be pinned reports why and fails the command.
        std::fs::write(
            crate_d(&ctx).join("Symposium.toml"),
            "[[skills]]\nsource.git = \"https://github.com/example/skills\"\n",
        )?;
        let (result, event) = digest_event(&mut ctx, "./deps/crate-d").await;
        assert!(result.is_err(), "ineligible is a failure");
        let event = event.expect("a payload_digest event");
        assert!(event.get("digest").is_none());
        assert!(
            event["ineligible"].as_str().unwrap().contains("source.git"),
            "{event}"
        );
        Ok(())
    })
    .await
    .unwrap();
}
