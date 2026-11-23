//! Session settings belong to the agent command, not the shared test process.

use std::path::Path;

use agent_client_protocol::AcpAgent;
use agent_client_protocol::schema::{EnvVariable, McpServer};
use anyhow::{Context, bail};

pub(super) fn for_session(ctx: &crate::TestContext) -> anyhow::Result<Vec<EnvVariable>> {
    let binary = std::env::var_os("CARGO_BIN_EXE_symposium")
        .context("CARGO_BIN_EXE_symposium must be set (run via cargo test)")?;
    let cwd = ctx
        .workspace_root
        .as_deref()
        .unwrap_or(ctx.sym.config_dir());
    values(Path::new(&binary), ctx.sym.config_dir(), cwd)
}

fn values(binary: &Path, config: &Path, cwd: &Path) -> anyhow::Result<Vec<EnvVariable>> {
    let bin_dir = binary
        .parent()
        .context("Symposium binary has no parent directory")?;
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(bin_dir.to_path_buf()).chain(std::env::split_paths(&inherited_path)),
    )
    .context("failed to build ACP agent PATH")?
    .into_string()
    .map_err(|_| anyhow::anyhow!("ACP agent PATH is not UTF-8"))?;
    let trace = cwd.join("hook-trace.jsonl");
    Ok(vec![
        EnvVariable::new("PATH", path),
        EnvVariable::new(
            "SYMPOSIUM_HOME",
            config.to_str().context("ACP config path is not UTF-8")?,
        ),
        EnvVariable::new(
            "SYMPOSIUM_HOOK_TRACE",
            trace.to_str().context("ACP trace path is not UTF-8")?,
        ),
    ])
}

pub(super) fn registry_wrapper(
    environment: Vec<EnvVariable>,
) -> impl acpr::CommandWrapper + Send + Sync {
    move |mut command: acpr::ResolvedCommand| {
        command.envs.extend(
            environment
                .iter()
                .map(|variable| (variable.name.clone().into(), variable.value.clone().into())),
        );
        command
    }
}

pub(super) fn custom_agent(
    command: &str,
    environment: Vec<EnvVariable>,
) -> anyhow::Result<AcpAgent> {
    let agent: AcpAgent = command.parse().context("bad ACP command")?;
    let McpServer::Stdio(mut server) = agent.into_server() else {
        bail!("custom ACP test agents must use stdio");
    };
    server.env.extend(environment);
    Ok(AcpAgent::new(McpServer::Stdio(server)))
}

#[cfg(test)]
mod tests;
