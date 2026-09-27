//! ガードレール検証エンジン
//!
//! タスクの実行順序、同時進行数、リトライ上限を強制し、
//! エージェントの逸脱や無限ループを防止する。

use crate::error::GuardrailError;
use crate::task::model::{Task, TaskStatus};

/// 同一タスクの最大リトライ許容回数（これ以上失敗するとサーキットブレーカー発動）
pub const MAX_RETRY_COUNT: u32 = 3;

/// ガードレール判定エンジン
pub struct Guardrail;

impl Guardrail {
    /// 単一アクティブ制約の検証
    ///
    /// 自身（`target_id`）以外のタスクに `InProgress` 状態のものが存在する場合、
    /// `GuardrailError::MultipleActiveTasks` を返却します。
    pub fn validate_single_active(tasks: &[Task], target_id: &str) -> Result<(), GuardrailError> {
        validate_single_active(tasks, target_id)
    }

    /// 暗黙的先行依存制約の検証
    ///
    /// インデックス `0 <= i < target_index` の先行タスクがすべて `Completed` であるかを検証します。
    /// 未完了の先行タスクが見つかった場合、`GuardrailError::PredecessorNotCompleted` を返却します。
    pub fn validate_predecessors(
        tasks: &[Task],
        target_index: usize,
        current_id: &str,
    ) -> Result<(), GuardrailError> {
        validate_predecessors(tasks, target_index, current_id)
    }

    /// サーキットブレーカー作動判定
    ///
    /// リトライ回数が `MAX_RETRY_COUNT` (3) 以上に達しているかを判定します。
    pub fn check_circuit_breaker(retry_count: u32) -> bool {
        check_circuit_breaker(retry_count)
    }
}

/// 単一アクティブ制約の検証関数
pub fn validate_single_active(tasks: &[Task], target_id: &str) -> Result<(), GuardrailError> {
    for task in tasks {
        if task.id != target_id && task.status == TaskStatus::InProgress {
            return Err(GuardrailError::MultipleActiveTasks {
                current_active_id: task.id.clone(),
            });
        }
    }
    Ok(())
}

/// 暗黙的先行依存制約の検証関数
pub fn validate_predecessors(
    tasks: &[Task],
    target_index: usize,
    current_id: &str,
) -> Result<(), GuardrailError> {
    let limit = target_index.min(tasks.len());
    for (i, task) in tasks.iter().take(limit).enumerate() {
        if task.status != TaskStatus::Completed {
            return Err(GuardrailError::PredecessorNotCompleted {
                predecessor_id: task.id.clone(),
                predecessor_index: i,
                current_id: current_id.to_string(),
            });
        }
    }
    Ok(())
}

/// サーキットブレーカー作動判定関数
pub fn check_circuit_breaker(retry_count: u32) -> bool {
    retry_count >= MAX_RETRY_COUNT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_predecessors_not_completed() {
        let tasks = vec![
            Task::new("t1", "Task 1", "Desc 1"),
            Task::new("t2", "Task 2", "Desc 2"),
        ];

        // t1 は Pending なので、t2 (インデックス 1) を着手しようとするとエラー
        let result = validate_predecessors(&tasks, 1, "t2");
        assert!(matches!(
            result,
            Err(GuardrailError::PredecessorNotCompleted {
                predecessor_id,
                predecessor_index: 0,
                current_id,
            }) if predecessor_id == "t1" && current_id == "t2"
        ));
    }

    #[test]
    fn test_validate_predecessors_all_completed() {
        let mut t1 = Task::new("t1", "Task 1", "Desc 1");
        t1.status = TaskStatus::Completed;

        let mut t2 = Task::new("t2", "Task 2", "Desc 2");
        t2.status = TaskStatus::Completed;

        let t3 = Task::new("t3", "Task 3", "Desc 3");

        let tasks = vec![t1, t2, t3];

        // t1, t2 が Completed なので、t3 (インデックス 2) は正常パス
        let result = validate_predecessors(&tasks, 2, "t3");
        assert!(result.is_ok());

        // 先頭タスク (インデックス 0) は先行タスクがないため常にパス
        let result_0 = validate_predecessors(&tasks, 0, "t1");
        assert!(result_0.is_ok());
    }

    #[test]
    fn test_validate_single_active_violation() {
        let mut t1 = Task::new("t1", "Task 1", "Desc 1");
        t1.status = TaskStatus::InProgress;

        let t2 = Task::new("t2", "Task 2", "Desc 2");

        let tasks = vec![t1, t2];

        // t1 が InProgress の状態で t2 を InProgress にしようとすると違反
        let result = validate_single_active(&tasks, "t2");
        assert!(matches!(
            result,
            Err(GuardrailError::MultipleActiveTasks { current_active_id }) if current_active_id == "t1"
        ));
    }

    #[test]
    fn test_validate_single_active_self_is_in_progress() {
        let mut t1 = Task::new("t1", "Task 1", "Desc 1");
        t1.status = TaskStatus::InProgress;

        let t2 = Task::new("t2", "Task 2", "Desc 2");

        let tasks = vec![t1, t2];

        // 自身 (t1) が既に InProgress の場合、自分自身は違反とみなさない
        let result = validate_single_active(&tasks, "t1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_circuit_breaker() {
        assert!(!check_circuit_breaker(0));
        assert!(!check_circuit_breaker(1));
        assert!(!check_circuit_breaker(2));
        assert!(check_circuit_breaker(3));
        assert!(check_circuit_breaker(4));
    }
}
