use symposium::plugins::{ValidationResult, validate_source_dir};
use symposium_testlib::{TestMode, with_fixture};

fn outline(results: &[ValidationResult]) -> Vec<String> {
    let mut lines = Vec::new();
    for result in results {
        let status = match &result.result {
            Err(e) => format!("error: {e:#}"),
            Ok(()) if result.warnings.is_empty() => "ok".to_string(),
            Ok(()) => format!("warning: {}", result.warnings.join("; ")),
        };
        lines.push(format!("{} {} {status}", result.kind, result.id));
        lines.extend(
            outline(&result.children)
                .into_iter()
                .map(|l| format!("  {l}")),
        );
    }
    lines
}

async fn assert_validates(subdir: &str, expected: &[&str]) {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-validate0"],
        async |ctx| {
            let results = validate_source_dir(&ctx.tempdir.join(subdir))?;
            assert_eq!(outline(&results), expected);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn validates_a_registry_folder() {
    assert_validates(
        "registry",
        &[
            "plugin plugin-a ok",
            "  skill a-guide ok",
            "plugin skill-b ok",
            "  skill skill-b ok",
        ],
    )
    .await;
}

#[tokio::test]
async fn validates_a_plugin_folder() {
    assert_validates(
        "registry/plugin-a",
        &["plugin plugin-a ok", "  skill a-guide ok"],
    )
    .await;
}

#[tokio::test]
async fn validates_a_skill_folder() {
    assert_validates(
        "registry/skill-b",
        &["plugin skill-b ok", "  skill skill-b ok"],
    )
    .await;
}

#[tokio::test]
async fn validates_a_crate_with_package_metadata() {
    assert_validates(
        "metadata-crate",
        &["plugin metadata-crate ok", "  skill meta-guide ok"],
    )
    .await;
}

#[tokio::test]
async fn validates_a_crate_manifest_without_a_name() {
    assert_validates(
        "manifest-crate",
        &["plugin manifest-crate ok", "  skill manifest-guide ok"],
    )
    .await;
}
