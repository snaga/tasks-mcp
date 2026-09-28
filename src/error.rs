use thiserror::Error;

/// ガードレール違反エラー
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GuardrailError {
    /// 既に他のタスクが進行中
    #[error("Multiple active tasks: task '{current_active_id}' is already in progress")]
    MultipleActiveTasks {
        /// 現在進行中のタスクID
        current_active_id: String,
    },

    /// 先行タスクが未完了
    #[error("Predecessor task '{predecessor_id}' (index {predecessor_index}) is not completed; cannot start '{current_id}'")]
    PredecessorNotCompleted {
        /// 未完了の先行タスクID
        predecessor_id: String,
        /// 先行タスクのインデックス
        predecessor_index: usize,
        /// 開始しようとしたタスクID
        current_id: String,
    },

    /// サーキットブレーカー作動による中断
    #[error("Circuit breaker triggered: task '{task_id}' failed {retry_count} times")]
    CircuitBreakerHalted {
        /// 中断されたタスクID
        task_id: String,
        /// 失敗回数
        retry_count: u32,
    },
}

impl GuardrailError {
    /// エージェントが次に行うべきアクションを提示するガイダンスメッセージを取得
    pub fn guidance_message(&self) -> String {
        match self {
            Self::MultipleActiveTasks { current_active_id } => {
                format!(
                    "Task '{current_active_id}' is already in progress. Please complete the current task before starting a new one."
                )
            }
            Self::PredecessorNotCompleted {
                predecessor_id,
                predecessor_index: _,
                current_id: _,
            } => {
                format!(
                    "Predecessor task '{predecessor_id}' is not completed. Please complete preceding tasks in sequential order."
                )
            }
            Self::CircuitBreakerHalted {
                task_id,
                retry_count,
            } => {
                format!(
                    "Task '{task_id}' failed {retry_count} times and has been blocked. Automatic retry is stopped. Please ask for human assistance."
                )
            }
        }
    }
}

/// タスクリスト操作エラー
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TaskListError {
    /// タスクID重複
    #[error("Duplicate task ID: '{task_id}'")]
    DuplicateTaskId {
        /// 重複したタスクID
        task_id: String,
    },

    /// タスクリストが空
    #[error("Task list cannot be empty")]
    EmptyTaskList,

    /// 指定されたタスクIDが存在しない
    #[error("Task not found: '{task_id}'")]
    TaskNotFound {
        /// 見つからなかったタスクID
        task_id: String,
    },
}

impl TaskListError {
    /// エージェントが次に行うべきアクションを提示するガイダンスメッセージを取得
    pub fn guidance_message(&self) -> String {
        match self {
            Self::DuplicateTaskId { task_id } => {
                format!(
                    "Task ID '{task_id}' is duplicated. Please re-create the task list with unique IDs."
                )
            }
            Self::EmptyTaskList => {
                "Task list cannot be empty. Please include at least one task.".to_string()
            }
            Self::TaskNotFound { task_id } => {
                format!("Task ID '{task_id}' does not exist. Please specify a valid task ID.")
            }
        }
    }
}

/// ストレージ入出力エラー
#[derive(Debug, Error)]
pub enum StorageError {
    /// I/Oエラー
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// シリアライズ / デシリアライズエラー
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// 無効なストレージデータ
    #[error("Invalid storage data: {0}")]
    InvalidData(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guardrail_error_display_and_guidance() {
        let err1 = GuardrailError::MultipleActiveTasks {
            current_active_id: "task-1".to_string(),
        };
        assert!(err1.to_string().contains("task-1"));
        assert!(err1.guidance_message().contains("is already in progress"));

        let err2 = GuardrailError::PredecessorNotCompleted {
            predecessor_id: "task-1".to_string(),
            predecessor_index: 0,
            current_id: "task-2".to_string(),
        };
        assert!(err2.to_string().contains("task-1"));
        assert!(err2.to_string().contains("task-2"));
        assert!(err2.guidance_message().contains("is not completed"));

        let err3 = GuardrailError::CircuitBreakerHalted {
            task_id: "task-1".to_string(),
            retry_count: 3,
        };
        assert!(err3.to_string().contains("task-1"));
        assert!(err3.to_string().contains("3"));
        assert!(err3.guidance_message().contains("has been blocked"));
    }

    #[test]
    fn test_task_list_error_display_and_guidance() {
        let err1 = TaskListError::DuplicateTaskId {
            task_id: "task-dup".to_string(),
        };
        assert!(err1.to_string().contains("task-dup"));
        assert!(err1.guidance_message().contains("is duplicated"));

        let err2 = TaskListError::EmptyTaskList;
        assert_eq!(err2.to_string(), "Task list cannot be empty");
        assert!(err2.guidance_message().contains("cannot be empty"));

        let err3 = TaskListError::TaskNotFound {
            task_id: "task-999".to_string(),
        };
        assert!(err3.to_string().contains("task-999"));
        assert!(err3.guidance_message().contains("does not exist"));
    }

    #[test]
    fn test_storage_error_display() {
        let io_err = StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(io_err.to_string().contains("I/O error"));

        let json_err: Result<(), serde_json::Error> = serde_json::from_str("{ invalid json }");
        let storage_json_err = StorageError::Serialization(json_err.unwrap_err());
        assert!(storage_json_err.to_string().contains("Serialization error"));

        let invalid_err = StorageError::InvalidData("Corrupt header".to_string());
        assert_eq!(invalid_err.to_string(), "Invalid storage data: Corrupt header");
    }
}
