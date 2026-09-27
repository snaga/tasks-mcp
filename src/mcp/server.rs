//! stdio JSON-RPC MCP サーバー
//!
//! MCP (Model Context Protocol) 2024-11-05 仕様に準拠し、
//! stdio 上で JSON-RPC 2.0 メッセージの送受信をハンドリングする。

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use serde_json::json;
use tracing::{debug, error, info};

use crate::mcp::tools::ToolsHandler;

/// MCP JSON-RPC サーバー
pub struct McpServer {
    tools: ToolsHandler,
}

impl McpServer {
    /// 新規 McpServer インスタンスを作成
    pub fn new(tools: ToolsHandler) -> Self {
        Self { tools }
    }

    /// 受信した1行のメッセージを処理し、返信が必要な場合は JSON 文字列を返却
    pub async fn handle_message(&self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        let parsed: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to parse JSON: {}", e);
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": "Parse error"
                    }
                });
                return Some(err_resp.to_string());
            }
        };

        let method = match parsed.get("method").and_then(|m| m.as_str()) {
            Some(m) => m,
            None => {
                // 通知やレスポンス等で method がない場合
                return None;
            }
        };

        let id = parsed.get("id");
        let is_notification = id.is_none() || id == Some(&serde_json::Value::Null);

        debug!(method = %method, is_notification = %is_notification, "Handling MCP request");

        // 通知イベント（レスポンス不要）
        if method == "notifications/initialized" {
            info!("Client initialized notification received");
            return None;
        }

        // メソッドごとのディスパッチ
        let result_or_error: Result<serde_json::Value, serde_json::Value> = match method {
            "initialize" => {
                let init_result = json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "tasks-mcp",
                        "version": "0.1.0"
                    }
                });
                Ok(init_result)
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(self.tools.list_tools()),
            "tools/call" => {
                let params = parsed.get("params").cloned().unwrap_or(json!({}));
                let name = params
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or_default();
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

                match self.tools.call_tool(name, arguments) {
                    Ok(tool_res) => {
                        let text = serde_json::to_string_pretty(&tool_res)
                            .unwrap_or_else(|_| tool_res.to_string());
                        Ok(json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": text
                                }
                            ],
                            "isError": false
                        }))
                    }
                    Err(err) => {
                        error!(tool_name = %name, error = %err, "Tool execution failed");
                        Ok(json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": format!("Error: {}", err)
                                }
                            ],
                            "isError": true
                        }))
                    }
                }
            }
            _ => {
                // 未知のメソッド
                if is_notification {
                    return None;
                }
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {}", method)
                    }
                });
                return Some(resp.to_string());
            }
        };

        if is_notification {
            return None;
        }

        match result_or_error {
            Ok(result) => {
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result
                });
                Some(resp.to_string())
            }
            Err(err_obj) => {
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": err_obj
                });
                Some(resp.to_string())
            }
        }
    }

    /// stdio トランスポートを実行
    pub async fn run_stdio<R, W>(&self, mut reader: R, mut writer: W) -> Result<(), anyhow::Error>
    where
        R: AsyncBufRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                // EOF
                break;
            }

            if let Some(resp) = self.handle_message(&line).await {
                writer.write_all(resp.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::AsyncWriteExt;
    use crate::task::{TaskManager, TaskStorage};

    fn setup_server() -> (McpServer, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(TaskStorage::new(temp_dir.path()));
        let manager = Arc::new(TaskManager::new(storage));
        let tools = ToolsHandler::new(manager);
        (McpServer::new(tools), temp_dir)
    }

    #[tokio::test]
    async fn test_handle_message_initialize() {
        let (server, _dir) = setup_server();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05"
            }
        });

        let resp_str = server.handle_message(&req.to_string()).await.expect("must return response");
        let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();

        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(resp["result"]["serverInfo"]["name"], "tasks-mcp");
    }

    #[tokio::test]
    async fn test_handle_message_notifications_initialized() {
        let (server, _dir) = setup_server();
        let notif = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });

        let resp_str = server.handle_message(&notif.to_string()).await;
        assert!(resp_str.is_none());
    }

    #[tokio::test]
    async fn test_handle_message_tools_list() {
        let (server, _dir) = setup_server();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });

        let resp_str = server.handle_message(&req.to_string()).await.expect("must return response");
        let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();

        assert_eq!(resp["id"], 2);
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 4);
    }

    #[tokio::test]
    async fn test_handle_message_tools_call_success_and_error() {
        let (server, _dir) = setup_server();

        // 1. tools/call create_task_list
        let create_call = json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "tools/call",
            "params": {
                "name": "create_task_list",
                "arguments": {
                    "tasks": [
                        { "id": "t1", "title": "タスク1", "description": "説明1" },
                        { "id": "t2", "title": "タスク2", "description": "説明2" }
                    ]
                }
            }
        });
        let res_str = server.handle_message(&create_call.to_string()).await.unwrap();
        let res: serde_json::Value = serde_json::from_str(&res_str).unwrap();
        assert_eq!(res["result"]["isError"], false);
        let text = res["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("\"task_count\": 2"));

        // 2. tools/call update_task_status (ガードレール違反)
        let fail_call = json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": {
                "name": "update_task_status",
                "arguments": {
                    "id": "t2",
                    "status": "in_progress"
                }
            }
        });
        let fail_str = server.handle_message(&fail_call.to_string()).await.unwrap();
        let fail_res: serde_json::Value = serde_json::from_str(&fail_str).unwrap();
        assert_eq!(fail_res["result"]["isError"], true);
        let err_text = fail_res["result"]["content"][0]["text"].as_str().unwrap();
        assert!(err_text.contains("Predecessor task 't1'"));
    }

    #[tokio::test]
    async fn test_run_stdio_stream() {
        let (server, _dir) = setup_server();

        let (mut client_write, server_read) = tokio::io::duplex(1024);
        let (server_write, mut client_read) = tokio::io::duplex(1024);

        let server_task = tokio::spawn(async move {
            let buf_reader = tokio::io::BufReader::new(server_read);
            server.run_stdio(buf_reader, server_write).await.unwrap();
        });

        // クライアント側から ping を送信
        let ping_msg = json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "ping"
        }).to_string() + "\n";

        client_write.write_all(ping_msg.as_bytes()).await.unwrap();

        // サーバーからの応答を1行受信
        let mut buf_client_reader = tokio::io::BufReader::new(&mut client_read);
        let mut resp_line = String::new();
        buf_client_reader.read_line(&mut resp_line).await.unwrap();

        let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();
        assert_eq!(resp["id"], 99);
        assert_eq!(resp["result"], json!({}));

        // クライアント書き込みを閉じて EOF を送る
        drop(client_write);
        server_task.await.unwrap();
    }
}
