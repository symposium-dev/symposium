use regex::Regex;

const TEMPLATE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/md/start.md"));

/// Whether the output is for the CLI or the MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Cli,
    Mcp,
}

/// Render the start template, expanding `$INVOKE(arg1,arg2,...)` placeholders
/// according to the render mode.
pub fn render(mode: RenderMode) -> String {
    let re = Regex::new(r"\$INVOKE\(([^)]+)\)").unwrap();
    re.replace_all(TEMPLATE, |caps: &regex::Captures| {
        let args: Vec<&str> = caps[1].split(',').map(|s| s.trim()).collect();
        match mode {
            RenderMode::Cli => {
                let joined = args.join(" ");
                format!("`cargo agents {joined}`")
            }
            RenderMode::Mcp => {
                let json_args: Vec<String> = args.iter().map(|a| format!("\"{a}\"")).collect();
                let joined = json_args.join(", ");
                format!("the `cargo_agents::rust` MCP tool with `[{joined}]`")
            }
        }
    })
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_cli_expands_invoke() {
        let output = render(RenderMode::Cli);
        assert!(
            output.contains("`cargo agents crate $name`"),
            "CLI should expand $INVOKE to cargo agents: {output}"
        );
        assert!(!output.contains("$INVOKE"));
    }

    #[test]
    fn render_mcp_expands_invoke() {
        let output = render(RenderMode::Mcp);
        assert!(
            output.contains("the `cargo_agents::rust` MCP tool with `[\"crate\", \"$name\"]`"),
            "MCP should expand $INVOKE to MCP tool: {output}"
        );
        assert!(!output.contains("$INVOKE"));
    }
}
