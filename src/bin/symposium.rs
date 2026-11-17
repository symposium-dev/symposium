use std::process::ExitCode;

use symposium::cli::Binary;

#[tokio::main]
async fn main() -> ExitCode {
    symposium::binary::main(Binary::Symposium, std::env::args_os().collect()).await
}
