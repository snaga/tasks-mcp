use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tasks_mcp::mcp::{McpServer, ToolsHandler};
use tasks_mcp::task::{TaskManager, TaskStorage};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// CLI arguments definition
#[derive(Parser, Debug)]
#[command(
    name = "tasks-mcp",
    version,
    about = "A Model Context Protocol (MCP) server for task management"
)]
struct Cli {
    /// Directory to persist task state files and audit logs
    #[arg(long, default_value = ".agents")]
    storage_dir: PathBuf,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // stderr 限定ロガーの初期化 (stdout は MCP JSON-RPC に専有させる)
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&cli.log_level));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    tracing::info!(
        storage_dir = %cli.storage_dir.display(),
        "Starting tasks-mcp server"
    );

    let storage = Arc::new(TaskStorage::new(cli.storage_dir));
    let manager = Arc::new(TaskManager::new(storage));
    let tools = ToolsHandler::new(manager);
    let server = McpServer::new(tools);

    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let stdout = tokio::io::stdout();

    server.run_stdio(stdin, stdout).await?;

    tracing::info!("tasks-mcp server terminated");
    Ok(())
}
