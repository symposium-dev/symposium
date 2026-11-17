use std::process::ExitCode;

use symposium::cli::Binary;

#[tokio::main]
async fn main() -> ExitCode {
    // When invoked as `cargo agents`, cargo passes "agents" as the first arg.
    let mut args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|arg| arg == "agents") {
        args.remove(1);
    }
    symposium::binary::main(Binary::CargoAgents, args).await
}
