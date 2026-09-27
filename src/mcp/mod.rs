//! Model Context Protocol (MCP) 通信層
//!
//! MCP 2024-11-05 仕様および JSON-RPC 2.0 に準拠した stdio 通信、
//! ツール一覧定義、およびツール呼び出しのディスパッチを提供する。

pub mod server;
pub mod tools;

pub use server::McpServer;
pub use tools::ToolsHandler;
