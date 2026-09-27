use std::collections::HashSet;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::TaskListError;

/// タスクの状態を表す列挙型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// 未着手
    Pending,
    /// 進行中
    InProgress,
    /// 完了
    Completed,
    /// 失敗
    Failed,
    /// サーキットブレーカー作動による中断状態
    Blocked,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Blocked => write!(f, "blocked"),
        }
    }
}

/// タスク履歴エントリ
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskHistoryEntry {
    /// 記録日時
    pub timestamp: DateTime<Utc>,
    /// 遷移先ステータス
    pub status: TaskStatus,
    /// 作業メモ
    #[serde(default)]
    pub notes: Option<String>,
}

/// 単一タスクを表す構造体
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// 一意なタスクID
    pub id: String,
    /// タイトル
    pub title: String,
    /// 説明
    pub description: String,
    /// 現在のステータス
    pub status: TaskStatus,
    /// 失敗・リトライ回数
    pub retry_count: u32,
    /// 履歴
    #[serde(default)]
    pub history: Vec<TaskHistoryEntry>,
}

impl Task {
    /// 新規タスクインスタンスを作成
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: description.into(),
            status: TaskStatus::Pending,
            retry_count: 0,
            history: Vec::new(),
        }
    }
}

/// 順序付けられたタスクリスト
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskList {
    /// スキーマバージョン
    pub version: String,
    /// 最終更新日時
    pub updated_at: DateTime<Utc>,
    /// タスク一覧（配列順序が実行順序）
    pub tasks: Vec<Task>,
}

impl TaskList {
    /// 新しいタスクリストを作成し、バリデーションを実施
    pub fn new(tasks: Vec<Task>) -> Result<Self, TaskListError> {
        if tasks.is_empty() {
            return Err(TaskListError::EmptyTaskList);
        }

        let mut seen = HashSet::with_capacity(tasks.len());
        for task in &tasks {
            if !seen.insert(&task.id) {
                return Err(TaskListError::DuplicateTaskId {
                    task_id: task.id.clone(),
                });
            }
        }

        Ok(Self {
            version: "1.0.0".to_string(),
            updated_at: Utc::now(),
            tasks,
        })
    }

    /// IDでタスクを検索
    pub fn find_by_id(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    /// IDでタスクを可変参照として検索
    pub fn find_by_id_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    /// IDでタスクのインデックスを検索
    pub fn find_index(&self, id: &str) -> Option<usize> {
        self.tasks.iter().position(|t| t.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_status_display_and_serde() {
        assert_eq!(TaskStatus::Pending.to_string(), "pending");
        assert_eq!(TaskStatus::InProgress.to_string(), "in_progress");
        assert_eq!(TaskStatus::Completed.to_string(), "completed");
        assert_eq!(TaskStatus::Failed.to_string(), "failed");
        assert_eq!(TaskStatus::Blocked.to_string(), "blocked");

        let json = serde_json::to_string(&TaskStatus::InProgress).unwrap();
        assert_eq!(json, "\"in_progress\"");

        let deserialized: TaskStatus = serde_json::from_str("\"in_progress\"").unwrap();
        assert_eq!(deserialized, TaskStatus::InProgress);
    }

    #[test]
    fn test_task_list_validation_empty() {
        let result = TaskList::new(vec![]);
        assert!(matches!(result, Err(TaskListError::EmptyTaskList)));
    }

    #[test]
    fn test_task_list_validation_duplicate_id() {
        let tasks = vec![
            Task::new("t1", "Title 1", "Desc 1"),
            Task::new("t1", "Title 2", "Desc 2"),
        ];
        let result = TaskList::new(tasks);
        assert!(matches!(
            result,
            Err(TaskListError::DuplicateTaskId { task_id }) if task_id == "t1"
        ));
    }

    #[test]
    fn test_task_list_find_and_modify() {
        let tasks = vec![
            Task::new("t1", "Title 1", "Desc 1"),
            Task::new("t2", "Title 2", "Desc 2"),
        ];
        let mut list = TaskList::new(tasks).unwrap();

        assert_eq!(list.find_index("t1"), Some(0));
        assert_eq!(list.find_index("t2"), Some(1));
        assert_eq!(list.find_index("t3"), None);

        assert_eq!(list.find_by_id("t1").map(|t| t.title.as_str()), Some("Title 1"));
        assert_eq!(list.find_by_id("t999"), None);

        if let Some(task) = list.find_by_id_mut("t1") {
            task.status = TaskStatus::InProgress;
            task.retry_count = 1;
            task.history.push(TaskHistoryEntry {
                timestamp: Utc::now(),
                status: TaskStatus::InProgress,
                notes: Some("作業開始".to_string()),
            });
        }

        assert_eq!(list.find_by_id("t1").unwrap().status, TaskStatus::InProgress);
        assert_eq!(list.find_by_id("t1").unwrap().retry_count, 1);
        assert_eq!(list.find_by_id("t1").unwrap().history.len(), 1);
    }

    #[test]
    fn test_task_list_json_roundtrip() {
        let tasks = vec![
            Task::new("t1", "Title 1", "Desc 1"),
            Task::new("t2", "Title 2", "Desc 2"),
        ];
        let list = TaskList::new(tasks).unwrap();

        let json = serde_json::to_string_pretty(&list).unwrap();
        let restored: TaskList = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.version, "1.0.0");
        assert_eq!(restored.tasks.len(), 2);
        assert_eq!(restored.tasks[0].id, "t1");
        assert_eq!(restored.tasks[0].status, TaskStatus::Pending);
        assert_eq!(restored.tasks[1].id, "t2");
    }
}
