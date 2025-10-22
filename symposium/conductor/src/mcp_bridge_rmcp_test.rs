//! Integration test for MCP bridge using rmcp crate

use tokio::net::TcpListener;
use tokio::process::Command;

#[tokio::test]
async fn test_mcp_bridge_with_rmcp() {
    use rmcp::{ClientHandler, ServerHandler, ServiceExt, model::*};

    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("conductor=debug".parse().unwrap()),
        )
        .with_test_writer()
        .try_init();

    #[derive(Clone)]
    struct MockServer;

    impl ServerHandler for MockServer {
        fn get_info(&self) -> ServerInfo {
            ServerInfo {
                protocol_version: ProtocolVersion::default(),
                capabilities: ServerCapabilities::default(),
                server_info: Implementation {
                    name: "test-server".to_string(),
                    version: "1.0.0".to_string(),
                },
                instructions: None,
            }
        }
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let server = MockServer;
        let running = server.serve(socket).await.unwrap();
        let _ = running.waiting().await;
    });

    #[derive(Clone)]
    struct MockClient;

    impl ClientHandler for MockClient {
        fn get_info(&self) -> ClientInfo {
            ClientInfo {
                protocol_version: ProtocolVersion::default(),
                capabilities: ClientCapabilities::default(),
                client_info: Implementation {
                    name: "test-client".to_string(),
                    version: "1.0.0".to_string(),
                },
            }
        }
    }

    let cargo_manifest_dir = env!("CARGO_MANIFEST_DIR");
    let conductor_binary = format!("{}/../../target/debug/conductor", cargo_manifest_dir);

    let mut child = Command::new(&conductor_binary)
        .arg("mcp")
        .arg(port.to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .expect("Failed to spawn conductor mcp");

    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = MockClient;
    let running = client
        .serve((stdout, stdin))
        .await
        .expect("Failed to start rmcp client");

    let peer = running.peer();

    let tools_result = peer
        .list_tools(None)
        .await
        .expect("Failed to call list_tools");

    assert_eq!(tools_result.tools.len(), 0);

    running.cancel().await.expect("Failed to cancel client");
    let _ = server_task.await;
    let _ = child.kill().await;
}
