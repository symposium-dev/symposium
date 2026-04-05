use expect_test::expect;

#[tokio::test]
async fn help() {
    let ctx = symposium_testlib::with_fixture(&["plugins0"]);
    // Clap handles "help" as a built-in, returning a parse error with help text.
    let result = ctx.invoke(&["help"]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    expect![[r#"
        failed to parse args: 
        Usage: symposium <COMMAND>

        Commands:
          start  Get Rust guidance and list available crate skills for the workspace
          crate  Find crate sources and guidance
          help   Print this message or the help of the given subcommand(s)

        Options:
          -h, --help  Print help
    "#]]
    .assert_eq(&err);
}

#[tokio::test]
async fn unknown_command() {
    let ctx = symposium_testlib::with_fixture(&["plugins0"]);
    let result = ctx.invoke(&["nonsense"]).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn start() {
    let ctx = symposium_testlib::with_fixture(&["plugins0"]);
    let output = ctx.invoke(&["start"]).await.unwrap();
    let output = ctx.normalize_paths(&output);
    expect![[r##"
        # Symposium — AI the Rust Way

        Symposium helps agents write better Rust by providing up-to-date language guidance and integration with the Rust ecosystem.

        ## Guidance on a particular crate

        Before authoring Rust code that uses a particular crate, the `symposium::rust` MCP tool with `["crate", "$name"]` will provide you with a path to the crate source, custom instructions for that crate, and a list of available skills that can be loaded.

        ## Skills available for current dependencies

        The custom skills available for the dependencies currently found in the workspace are included below. You can read the skill file to learn more about it.

        To display an updated list of skills, for example if new crates are added, invoke the `symposium::rust` MCP tool with `["crate", "$name"]`.

        No skills available for crates in the current dependencies."##]]
    .assert_eq(&output);
}

#[tokio::test]
async fn crate_list_with_plugins() {
    let ctx = symposium_testlib::with_fixture(&["plugins0"]);
    let output = ctx.invoke(&["crate", "--list"]).await.unwrap();
    let output = ctx.normalize_paths(&output);
    expect!["No skills available for crates in the current dependencies."].assert_eq(&output);
}
