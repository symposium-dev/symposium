use clap::{Parser, Subcommand};
use std::process::ExitCode;

mod cargo_cmd;
mod mcp;
pub mod tutorial;

#[derive(Parser)]
#[command(name = "symposium", version, about = "AI the Rust Way")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run cargo commands with token-optimized output
    Cargo {
        /// Arguments passed to cargo (e.g., check, build --release, test -- --nocapture)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Show the Symposium tutorial for agents and humans
    Tutorial,

    /// Run as an MCP server (stdio transport)
    Mcp,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Cargo { args }) => cargo_cmd::run(args),
        Some(Commands::Tutorial) => {
            print!("{}", tutorial::render_cli());
            ExitCode::SUCCESS
        }
        Some(Commands::Mcp) => {
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            match rt.block_on(mcp::serve()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("MCP server error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        None => {
            println!("symposium — AI the Rust Way");
            println!();
            println!("Usage: symposium <command>");
            println!();
            println!("Commands:");
            println!("  cargo      Run cargo commands with token-optimized output");
            println!("  tutorial   Show the Symposium tutorial for agents and humans");
            println!("  mcp        Run as an MCP server (stdio transport)");
            println!("  help       Show this message");
            println!();
            println!("Run `symposium <command> --help` for more information.");
            ExitCode::SUCCESS
        }
    }
}
