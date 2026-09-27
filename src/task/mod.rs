//! タスク管理・ガードレール・ストレージドメインモジュール
//!
//! MCP プロトコルに依存しない独立したコアロジック層。
//! タスクリストのライフサイクル管理、状態遷移、厳格な一直線リニア実行の強制、
//! 単一アクティブ制約、サーキットブレーカー判定、アトミック永続化および監査用 JSONL ログ記録を提供する。

pub mod guardrail;
pub mod manager;
pub mod model;
pub mod storage;

// 主要型の re-export
pub use guardrail::{
    check_circuit_breaker, validate_predecessors, validate_single_active, Guardrail,
    MAX_RETRY_COUNT,
};
pub use manager::{
    NextTaskResponse, TaskCreateInput, TaskManager, TaskStatusItem, TaskSummary, UpdateTaskResponse,
};
pub use model::{Task, TaskHistoryEntry, TaskList, TaskStatus};
pub use storage::{LogEntry, TaskStorage};
