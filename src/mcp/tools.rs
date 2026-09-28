//! MCP ツール定義 & ディスパッチャー
//!
//! MCP プロトコルに準拠したツール一覧定義の公開（tools/list）および
//! ツール呼び出し（tools/call）をタスクマネージャーへ安全にディスパッチする。

use std::sync::Arc;

use serde_json::json;

use crate::task::{TaskCreateInput, TaskManager, TaskStatus};

/// MCP ツールハンドラー
pub struct ToolsHandler {
    manager: Arc<TaskManager>,
}

impl ToolsHandler {
    /// 新規 ToolsHandler を作成
    pub fn new(manager: Arc<TaskManager>) -> Self {
        Self { manager }
    }

    /// タスクマネージャーの参照を取得
    pub fn manager(&self) -> &Arc<TaskManager> {
        &self.manager
    }

    /// MCP `tools/list` 応答用のツール一覧定義を返却
    pub fn list_tools(&self) -> serde_json::Value {
        json!({
            "tools": [
                {
                    "name": "create_task_list",
                    "description": "Initialize and register an ordered task list. The array index order implicitly dictates the sequential execution pipeline.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "tasks": {
                                "type": "array",
                                "description": "Initial list of tasks to register (in execution order)",
                                "items": {
                                    "type": "object",
                                    "required": ["id", "title", "description"],
                                    "properties": {
                                        "id": {
                                            "type": "string",
                                            "description": "Unique task identifier"
                                        },
                                        "title": {
                                            "type": "string",
                                            "description": "Task title"
                                        },
                                        "description": {
                                            "type": "string",
                                            "description": "Detailed task description"
                                        }
                                    }
                                }
                            }
                        },
                        "required": ["tasks"]
                    }
                },
                {
                    "name": "get_next_task",
                    "description": "Retrieve the single next task to execute. Prioritizes active in-progress tasks, then returns the foremost pending task.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "update_task_status",
                    "description": "Update task status and record work notes. Guardrail rules automatically block skip attempts and concurrent active tasks.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Target task ID"
                            },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed", "failed"],
                                "description": "New status to transition to"
                            },
                            "notes": {
                                "type": "string",
                                "description": "Optional work notes or history"
                            }
                        },
                        "required": ["id", "status"]
                    }
                },
                {
                    "name": "get_task_summary",
                    "description": "Retrieve comprehensive task progress summary (progress percent, current active task, remaining count).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }
            ]
        })
    }

    /// MCP ツール呼び出し（`tools/call`）を実行
    pub fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, anyhow::Error> {
        match name {
            "create_task_list" => self.handle_create_task_list(arguments),
            "get_next_task" => self.handle_get_next_task(),
            "update_task_status" => self.handle_update_task_status(arguments),
            "get_task_summary" => self.handle_get_task_summary(),
            _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
        }
    }

    fn handle_create_task_list(
        &self,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, anyhow::Error> {
        let tasks_val = args
            .get("tasks")
            .ok_or_else(|| anyhow::anyhow!("Missing 'tasks' argument in create_task_list"))?;

        let items: Vec<TaskCreateInput> = serde_json::from_value(tasks_val.clone())
            .map_err(|e| anyhow::anyhow!("Invalid 'tasks' format: {}", e))?;

        let list = self.manager.create_task_list(items)?;

        let next_task = list.tasks.first().cloned();
        let message = match &next_task {
            Some(t) => format!(
                "Task list initialized successfully. Please proceed with the first task '{}'.",
                t.id
            ),
            None => "Task list initialized successfully.".to_string(),
        };

        Ok(json!({
            "success": true,
            "task_count": list.tasks.len(),
            "next_task": next_task,
            "message": message
        }))
    }

    fn handle_get_next_task(&self) -> Result<serde_json::Value, anyhow::Error> {
        let resp = self.manager.get_next_task()?;
        Ok(serde_json::to_value(resp)?)
    }

    fn handle_update_task_status(
        &self,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, anyhow::Error> {
        let id = args
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid 'id' parameter"))?;

        let status_val = args
            .get("status")
            .ok_or_else(|| anyhow::anyhow!("Missing 'status' parameter"))?;
        let status: TaskStatus = serde_json::from_value(status_val.clone())
            .map_err(|e| anyhow::anyhow!("Invalid 'status' value: {}", e))?;

        let notes = args
            .get("notes")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let res = self.manager.update_task_status(id, status, notes)?;

        let message = if res.is_all_completed {
            "All tasks have been completed successfully!".to_string()
        } else if status == TaskStatus::Completed {
            if let Some(ref next) = res.next_task {
                format!(
                    "Task '{}' completed. Please proceed to the next task '{}'.",
                    id, next.id
                )
            } else {
                "Task status updated successfully.".to_string()
            }
        } else {
            "Task status updated successfully.".to_string()
        };

        Ok(json!({
            "success": true,
            "task_id": res.task.id,
            "status": res.task.status,
            "retry_count": res.task.retry_count,
            "next_task": res.next_task,
            "is_all_completed": res.is_all_completed,
            "message": message
        }))
    }

    fn handle_get_task_summary(&self) -> Result<serde_json::Value, anyhow::Error> {
        let summary = self.manager.get_task_summary()?;
        Ok(serde_json::to_value(summary)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::TaskStorage;

    #[test]
    fn test_list_tools_returns_all_four_tools() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(TaskStorage::new(temp_dir.path()));
        let manager = Arc::new(TaskManager::new(storage));
        let handler = ToolsHandler::new(manager);

        let list = handler.list_tools();
        let tools = list["tools"].as_array().expect("tools must be array");
        assert_eq!(tools.len(), 4);

        let tool_names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();

        assert!(tool_names.contains(&"create_task_list"));
        assert!(tool_names.contains(&"get_next_task"));
        assert!(tool_names.contains(&"update_task_status"));
        assert!(tool_names.contains(&"get_task_summary"));
    }

    #[test]
    fn test_call_tool_workflow_and_guardrails() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(TaskStorage::new(temp_dir.path()));
        let manager = Arc::new(TaskManager::new(storage));
        let handler = ToolsHandler::new(manager);

        // 1. create_task_list
        let create_args = json!({
            "tasks": [
                { "id": "t1", "title": "タスク1", "description": "説明1" },
                { "id": "t2", "title": "タスク2", "description": "説明2" }
            ]
        });
        let res_create = handler.call_tool("create_task_list", create_args).unwrap();
        assert_eq!(res_create["success"], true);
        assert_eq!(res_create["task_count"], 2);
        assert_eq!(res_create["next_task"]["id"], "t1");
        assert!(res_create["message"]
            .as_str()
            .unwrap()
            .contains("Please proceed with the first task 't1'."));

        // 2. get_next_task -> t1
        let res_next = handler.call_tool("get_next_task", json!({})).unwrap();
        assert_eq!(res_next["task"]["id"], "t1");
        assert_eq!(res_next["is_all_completed"], false);

        // 3. ガードレール違反: 先行 t1 が pending なのに t2 を in_progress にする
        let err_res = handler.call_tool(
            "update_task_status",
            json!({
                "id": "t2",
                "status": "in_progress"
            }),
        );
        assert!(err_res.is_err());
        let err_msg = err_res.unwrap_err().to_string();
        assert!(err_msg.contains("Predecessor task 't1'"));

        // 4. t1 を in_progress に更新
        let res_update1 = handler
            .call_tool(
                "update_task_status",
                json!({
                    "id": "t1",
                    "status": "in_progress",
                    "notes": "開始"
                }),
            )
            .unwrap();
        assert_eq!(res_update1["success"], true);
        assert_eq!(res_update1["status"], "in_progress");
        assert_eq!(res_update1["next_task"]["id"], "t1");
        assert_eq!(res_update1["is_all_completed"], false);

        // 5. t1 を completed に更新
        let res_update2 = handler
            .call_tool(
                "update_task_status",
                json!({
                    "id": "t1",
                    "status": "completed",
                    "notes": "完了"
                }),
            )
            .unwrap();
        assert_eq!(res_update2["success"], true);
        assert_eq!(res_update2["status"], "completed");
        assert_eq!(res_update2["next_task"]["id"], "t2");
        assert_eq!(res_update2["is_all_completed"], false);
        assert!(res_update2["message"]
            .as_str()
            .unwrap()
            .contains("Please proceed to the next task 't2'."));

        // 6. get_task_summary
        let res_summary = handler.call_tool("get_task_summary", json!({})).unwrap();
        assert_eq!(res_summary["total_tasks"], 2);
        assert_eq!(res_summary["completed_tasks"], 1);
        assert_eq!(res_summary["progress_percent"], 50.0);

        // 7. 未知のツール名呼び出し
        let unknown = handler.call_tool("unknown_tool", json!({}));
        assert!(unknown.is_err());
        assert!(unknown.unwrap_err().to_string().contains("Unknown tool"));
    }
}
