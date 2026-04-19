use symposium_testlib::TestMode;

#[tokio::test]
async fn help() {
    symposium_testlib::with_fixture(TestMode::SimulationOnly, &["plugins0"], async |mut ctx| {
        let result = ctx.symposium(&["help"]).await;
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("symposium"),
            "help should mention symposium: {err}"
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn unknown_command() {
    symposium_testlib::with_fixture(TestMode::SimulationOnly, &["plugins0"], async |mut ctx| {
        let result = ctx.symposium(&["nonsense"]).await;
        assert!(result.is_err());
        Ok(())
    })
    .await
    .unwrap();
}
