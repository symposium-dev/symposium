use expect_test::expect;

#[tokio::test]
async fn help() {
    let ctx = symposium_testlib::with_fixture(&["plugins0"]);
    // Clap handles "help" as a built-in, returning a parse error with help text.
    let result = ctx.invoke(&["help"]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    // Just verify it contains the right binary name
    assert!(
        err.contains("symposium"),
        "help should mention symposium: {err}"
    );
}

#[tokio::test]
async fn unknown_command() {
    let ctx = symposium_testlib::with_fixture(&["plugins0"]);
    let result = ctx.invoke(&["nonsense"]).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn crate_list_with_plugins() {
    let ctx = symposium_testlib::with_fixture(&["plugins0"]);
    let output = ctx.invoke(&["crate", "--list"]).await.unwrap();
    let output = ctx.normalize_paths(&output);
    expect!["No skills available for crates in the current dependencies."].assert_eq(&output);
}
