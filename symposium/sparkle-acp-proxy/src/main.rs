use scp::JsonRpcConnection;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connection = JsonRpcConnection::new(tokio::io::stdout(), tokio::io::stdin());

    Ok(())
}
