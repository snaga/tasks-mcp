//! タスクオーケストレーター (TaskManager)
//!
//! タスクリストのライフサイクル管理、状態遷移、
//! ガードレール強制、進捗サマリー生成を統括する。

use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::{GuardrailError, TaskListError};
use crate::task::guardrail::{check_circuit_breaker, validate_predecessors, validate_single_active};
use crate::task::model::{Task, TaskHistoryEntry, TaskList, TaskStatus};
use crate::task::storage::{LogEntry, TaskStorage};

/// タスク作成用の入力パラメータ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCreateInput {
    /// タスクID
    pub id: String,
    /// タイトル
    pub title: String,
    /// 詳細説明
    pub description: String,
}

/// 次タスク取得のレスポンス
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextTaskResponse {
    /// 着手すべきタスク（全完了または候補なし時は None）
    pub task: Option<Task>,
    /// 全タスク完了フラグ
    pub is_all_completed: bool,
    /// サーキットブレーカー作動中（ブロックされたタスクが存在する）フラグ
    pub is_blocked: bool,
    /// 総タスク数
    pub total_tasks: usize,
    /// 完了タスク数
    pub completed_tasks: usize,
}

/// タスク更新のレスポンス
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTaskResponse {
    /// 更新されたタスク
    pub task: Task,
    /// 次に着手すべきタスク（全完了または候補なし時は None）
    pub next_task: Option<Task>,
    /// 全タスク完了フラグ
    pub is_all_completed: bool,
}

/// タスクステータスサマリー項目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatusItem {
    /// タスクID
    pub id: String,
    /// タイトル
    pub title: String,
    /// ステータス
    pub status: TaskStatus,
}

/// タスク全体の進捗サマリー
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    /// 総タスク数
    pub total_tasks: usize,
    /// 完了タスク数
    pub completed_tasks: usize,
    /// 完了率（%）
    pub progress_percent: f64,
    /// 現在進行中タスクのID
    pub current_active_task_id: Option<String>,
    /// 全タスクのステータス一覧
    pub status_summary: Vec<TaskStatusItem>,
    /// 人間・LLM向けの整形済みテキストサマリー
    pub formatted_summary: String,
}

/// タスクマネージャー
pub struct TaskManager {
    storage: Arc<TaskStorage>,
}

impl TaskManager {
    /// 新規 TaskManager インスタンスを作成
    pub fn new(storage: Arc<TaskStorage>) -> Self {
        Self { storage }
    }

    /// ストレージ参照を取得
    pub fn storage(&self) -> &Arc<TaskStorage> {
        &self.storage
    }

    /// タスクリストを新規作成・初期登録
    pub fn create_task_list(
        &self,
        items: Vec<TaskCreateInput>,
    ) -> Result<TaskList, anyhow::Error> {
        let start_time = Instant::now();
        let tasks: Vec<Task> = items
            .into_iter()
            .map(|item| Task::new(item.id, item.title, item.description))
            .collect();

        let list = TaskList::new(tasks)?;
        self.storage.save_atomic(&list)?;

        let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
        let log_entry = LogEntry::new(
            "create_task_list",
            serde_json::json!({ "task_count": list.tasks.len() }),
            serde_json::json!({ "success": true, "task_count": list.tasks.len() }),
            duration_ms,
        );
        let _ = self.storage.append_log(&log_entry);

        Ok(list)
    }

    /// 次に着手すべきタスクを取得
    pub fn get_next_task(&self) -> Result<NextTaskResponse, anyhow::Error> {
        let list = self
            .storage
            .load()?
            .ok_or_else(|| anyhow::anyhow!("タスクリストが初期化されていません。先に create_task_list を実行してください。"))?;

        let total_tasks = list.tasks.len();
        let completed_tasks = list
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .count();
        let is_blocked = list.tasks.iter().any(|t| t.status == TaskStatus::Blocked);
        let is_all_completed = total_tasks > 0 && completed_tasks == total_tasks;

        // 優先度:
        // 1. in_progress
        // 2. リトライ可能 failed (retry_count < 3)
        // 3. 先頭の pending
        let candidate = list
            .tasks
            .iter()
            .find(|t| t.status == TaskStatus::InProgress)
            .or_else(|| {
                list.tasks
                    .iter()
                    .find(|t| t.status == TaskStatus::Failed && t.retry_count < 3)
            })
            .or_else(|| list.tasks.iter().find(|t| t.status == TaskStatus::Pending));

        Ok(NextTaskResponse {
            task: candidate.cloned(),
            is_all_completed,
            is_blocked,
            total_tasks,
            completed_tasks,
        })
    }

    /// タスクステータスを更新し、履歴メモを記録
    pub fn update_task_status(
        &self,
        id: &str,
        status: TaskStatus,
        notes: Option<String>,
    ) -> Result<UpdateTaskResponse, anyhow::Error> {
        let start_time = Instant::now();
        let mut list = self
            .storage
            .load()?
            .ok_or_else(|| anyhow::anyhow!("Task list is not initialized. Please run create_task_list first."))?;

        let target_index = list.find_index(id).ok_or_else(|| {
            anyhow::anyhow!(TaskListError::TaskNotFound {
                task_id: id.to_string()
            })
        })?;

        // ガードレール検証
        let guardrail_check = match status {
            TaskStatus::InProgress => {
                validate_single_active(&list.tasks, id)
                    .and_then(|_| validate_predecessors(&list.tasks, target_index, id))
            }
            TaskStatus::Completed => validate_predecessors(&list.tasks, target_index, id),
            _ => Ok(()),
        };

        if let Err(err) = guardrail_check {
            let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
            let rule_name = match &err {
                GuardrailError::MultipleActiveTasks { .. } => "MultipleActiveTasks",
                GuardrailError::PredecessorNotCompleted { .. } => "PredecessorNotCompleted",
                GuardrailError::CircuitBreakerHalted { .. } => "CircuitBreakerHalted",
            };
            let violation_log = LogEntry::new_guardrail_violation(
                "update_task_status",
                serde_json::json!({ "id": id, "status": status, "notes": notes }),
                serde_json::json!({ "success": false, "error": err.to_string() }),
                duration_ms,
                serde_json::json!({
                    "rule": rule_name,
                    "detail": err.to_string(),
                    "guidance": err.guidance_message(),
                }),
            );
            let _ = self.storage.append_log(&violation_log);
            return Err(anyhow::anyhow!(err));
        }

        // 状態更新 & サーキットブレーカー判定
        let task = &mut list.tasks[target_index];
        let next_status = match status {
            TaskStatus::Failed => {
                task.retry_count += 1;
                if check_circuit_breaker(task.retry_count) {
                    TaskStatus::Blocked
                } else {
                    TaskStatus::Failed
                }
            }
            _ => status,
        };

        task.status = next_status;
        task.history.push(TaskHistoryEntry {
            timestamp: Utc::now(),
            status: next_status,
            notes: notes.clone(),
        });
        list.updated_at = Utc::now();

        let updated_task = task.clone();
        self.storage.save_atomic(&list)?;

        let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
        let event_type = if next_status == TaskStatus::Blocked {
            "circuit_breaker_triggered"
        } else {
            "tool_call"
        };
        let mut log_entry = LogEntry::new(
            "update_task_status",
            serde_json::json!({ "id": id, "status": status, "notes": notes }),
            serde_json::json!({
                "success": true,
                "task_id": updated_task.id,
                "status": updated_task.status,
                "retry_count": updated_task.retry_count,
            }),
            duration_ms,
        );
        log_entry.event_type = event_type.to_string();
        let _ = self.storage.append_log(&log_entry);

        let total_tasks = list.tasks.len();
        let completed_tasks = list
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .count();
        let is_all_completed = total_tasks > 0 && completed_tasks == total_tasks;

        let next_task = if is_all_completed {
            None
        } else {
            list.tasks
                .iter()
                .find(|t| t.status == TaskStatus::InProgress)
                .or_else(|| {
                    list.tasks
                        .iter()
                        .find(|t| t.status == TaskStatus::Failed && t.retry_count < 3)
                })
                .or_else(|| list.tasks.iter().find(|t| t.status == TaskStatus::Pending))
                .cloned()
        };

        Ok(UpdateTaskResponse {
            task: updated_task,
            next_task,
            is_all_completed,
        })
    }

    /// 全体進捗状況サマリーを取得
    pub fn get_task_summary(&self) -> Result<TaskSummary, anyhow::Error> {
        let loaded = self.storage.load()?;
        let (tasks, total_tasks) = match &loaded {
            Some(list) => (list.tasks.as_slice(), list.tasks.len()),
            None => (&[][..], 0),
        };

        let completed_tasks = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .count();
        let progress_percent = if total_tasks == 0 {
            0.0
        } else {
            (completed_tasks as f64 / total_tasks as f64) * 100.0
        };

        let current_active_task = tasks.iter().find(|t| t.status == TaskStatus::InProgress);
        let current_active_task_id = current_active_task.map(|t| t.id.clone());

        let remaining_tasks = total_tasks.saturating_sub(completed_tasks);
        let active_desc = match current_active_task {
            Some(t) => format!("[{}] {}", t.id, t.title),
            None => "None".to_string(),
        };

        let formatted_summary = format!(
            "Progress: {}/{} ({:.1}%)\nIn Progress: {}\nRemaining: {} task(s)",
            completed_tasks, total_tasks, progress_percent, active_desc, remaining_tasks
        );

        let status_summary = tasks
            .iter()
            .map(|t| TaskStatusItem {
                id: t.id.clone(),
                title: t.title.clone(),
                status: t.status,
            })
            .collect();

        Ok(TaskSummary {
            total_tasks,
            completed_tasks,
            progress_percent,
            current_active_task_id,
            status_summary,
            formatted_summary,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    #[test]
    fn test_task_manager_lifecycle_and_circuit_breaker() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(TaskStorage::new(temp_dir.path()));
        let manager = TaskManager::new(storage.clone());

        // 1. タスクリスト初期登録
        let inputs = vec![
            TaskCreateInput {
                id: "t1".to_string(),
                title: "タスク1".to_string(),
                description: "説明1".to_string(),
            },
            TaskCreateInput {
                id: "t2".to_string(),
                title: "タスク2".to_string(),
                description: "説明2".to_string(),
            },
        ];
        let list = manager.create_task_list(inputs).unwrap();
        assert_eq!(list.tasks.len(), 2);

        // 2. 次タスク取得 (t1 が pending で取得される)
        let next1 = manager.get_next_task().unwrap();
        assert_eq!(next1.task.as_ref().unwrap().id, "t1");
        assert_eq!(next1.completed_tasks, 0);
        assert!(!next1.is_all_completed);

        // 3. t1 を InProgress に更新
        let updated1 = manager
            .update_task_status("t1", TaskStatus::InProgress, Some("着手".to_string()))
            .unwrap();
        assert_eq!(updated1.task.status, TaskStatus::InProgress);
        assert_eq!(updated1.next_task.as_ref().unwrap().id, "t1");
        assert!(!updated1.is_all_completed);

        // 4. t1 を Completed に更新
        let updated1_comp = manager
            .update_task_status("t1", TaskStatus::Completed, Some("完了".to_string()))
            .unwrap();
        assert_eq!(updated1_comp.task.status, TaskStatus::Completed);
        assert_eq!(updated1_comp.next_task.as_ref().unwrap().id, "t2");
        assert!(!updated1_comp.is_all_completed);

        // 5. 次タスク取得 (t2 が取得される)
        let next2 = manager.get_next_task().unwrap();
        assert_eq!(next2.task.as_ref().unwrap().id, "t2");

        // 6. t2 を InProgress にして、3回連続 Failed をシミュレート
        manager
            .update_task_status("t2", TaskStatus::InProgress, None)
            .unwrap();

        let fail1 = manager
            .update_task_status("t2", TaskStatus::Failed, Some("エラー1".to_string()))
            .unwrap();
        assert_eq!(fail1.task.status, TaskStatus::Failed);
        assert_eq!(fail1.task.retry_count, 1);

        let fail2 = manager
            .update_task_status("t2", TaskStatus::Failed, Some("エラー2".to_string()))
            .unwrap();
        assert_eq!(fail2.task.status, TaskStatus::Failed);
        assert_eq!(fail2.task.retry_count, 2);

        let fail3 = manager
            .update_task_status("t2", TaskStatus::Failed, Some("エラー3".to_string()))
            .unwrap();
        // 3回目で自動的に Blocked へ遷移
        assert_eq!(fail3.task.status, TaskStatus::Blocked);
        assert_eq!(fail3.task.retry_count, 3);

        // 7. 進捗サマリー確認
        let summary = manager.get_task_summary().unwrap();
        assert_eq!(summary.total_tasks, 2);
        assert_eq!(summary.completed_tasks, 1);
        assert_eq!(summary.progress_percent, 50.0);
        assert!(summary.formatted_summary.contains("50.0%"));
    }

    #[test]
    fn test_task_manager_guardrail_violation_and_log() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(TaskStorage::new(temp_dir.path()));
        let manager = TaskManager::new(storage.clone());

        let inputs = vec![
            TaskCreateInput {
                id: "t1".to_string(),
                title: "タスク1".to_string(),
                description: "説明1".to_string(),
            },
            TaskCreateInput {
                id: "t2".to_string(),
                title: "タスク2".to_string(),
                description: "説明2".to_string(),
            },
        ];
        manager.create_task_list(inputs).unwrap();

        // t1 が未完了の状態で t2 を InProgress にしようとする (PredecessorNotCompleted 違反)
        let result = manager.update_task_status("t2", TaskStatus::InProgress, None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.downcast_ref::<GuardrailError>().is_some());

        // 監査ログに guardrail_violation が記録されているか検証
        let log_file = File::open(storage.log_file_path()).unwrap();
        let reader = BufReader::new(log_file);
        let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();

        // 1行目は create_task_list, 2行目は guardrail_violation
        assert_eq!(lines.len(), 2);
        let log_json: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(log_json["event_type"], "guardrail_violation");
        assert_eq!(
            log_json["guardrail_violation"]["rule"],
            "PredecessorNotCompleted"
        );
    }
}
