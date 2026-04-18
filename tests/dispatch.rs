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
