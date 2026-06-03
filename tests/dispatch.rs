use symposium_testlib::TestMode;

#[tokio::test]
async fn help() {
    symposium_testlib::with_fixture(TestMode::SimulationOnly, &["plugins0"], async |mut ctx| {
        let out = ctx.symposium(&["help"]).await?;
        assert!(
            out.contains("Commands for humans:"),
            "help should render audience-grouped sections: {out}"
        );
        assert!(
            out.contains("Commands for agents:"),
            "help should render audience-grouped sections: {out}"
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
