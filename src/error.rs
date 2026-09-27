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
                    "タスク '{current_active_id}' が既に進行中です。新しいタスクを開始する前に、現在のタスクを完了（completed）させてください。"
                )
            }
            Self::PredecessorNotCompleted {
                predecessor_id,
                predecessor_index: _,
                current_id: _,
            } => {
                format!(
                    "先行タスク '{predecessor_id}' が完了していません。リストの順番通りに前のタスクから完了させてください。"
                )
            }
            Self::CircuitBreakerHalted {
                task_id,
                retry_count,
            } => {
                format!(
                    "タスク '{task_id}' は{retry_count}回失敗したため実行中断（blocked）されました。これ以上の自動リトライはできません。人間に支援を求めてください。"
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
                    "タスクID '{task_id}' が重複しています。一意なIDでタスクリストを作成し直してください。"
                )
            }
            Self::EmptyTaskList => {
                "タスクリストが空です。少なくとも1つのタスクを含めて作成してください。".to_string()
            }
            Self::TaskNotFound { task_id } => {
                format!("タスクID '{task_id}' は存在しません。有効なタスクIDを指定してください。")
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
        assert!(err1.guidance_message().contains("タスク 'task-1' が既に進行中です"));

        let err2 = GuardrailError::PredecessorNotCompleted {
            predecessor_id: "task-1".to_string(),
            predecessor_index: 0,
            current_id: "task-2".to_string(),
        };
        assert!(err2.to_string().contains("task-1"));
        assert!(err2.to_string().contains("task-2"));
        assert!(err2.guidance_message().contains("先行タスク 'task-1' が完了していません"));

        let err3 = GuardrailError::CircuitBreakerHalted {
            task_id: "task-1".to_string(),
            retry_count: 3,
        };
        assert!(err3.to_string().contains("task-1"));
        assert!(err3.to_string().contains("3"));
        assert!(err3.guidance_message().contains("タスク 'task-1' は3回失敗したため実行中断"));
    }

    #[test]
    fn test_task_list_error_display_and_guidance() {
        let err1 = TaskListError::DuplicateTaskId {
            task_id: "task-dup".to_string(),
        };
        assert!(err1.to_string().contains("task-dup"));
        assert!(err1.guidance_message().contains("タスクID 'task-dup' が重複しています"));

        let err2 = TaskListError::EmptyTaskList;
        assert_eq!(err2.to_string(), "Task list cannot be empty");
        assert!(err2.guidance_message().contains("タスクリストが空です"));

        let err3 = TaskListError::TaskNotFound {
            task_id: "task-999".to_string(),
        };
        assert!(err3.to_string().contains("task-999"));
        assert!(err3.guidance_message().contains("タスクID 'task-999' は存在しません"));
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
