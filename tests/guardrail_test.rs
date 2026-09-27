//! ガードレール統合シナリオテスト
//!
//! 設計書 (doc/design.md) に定義された以下の振る舞いを E2E 結合検証する:
//! 1. 一直線実行（Linear Pipeline Happy Path）
//! 2. スキップ試行の物理的遮断（PredecessorNotCompleted ガードレール）
//! 3. 単一アクティブ制約の強制（MultipleActiveTasks ガードレール）
//! 4. サーキットブレーカー作動および人間による外部ファイル修正での復旧

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::Arc;

use tasks_mcp::error::GuardrailError;
use tasks_mcp::task::{
    TaskCreateInput, TaskList, TaskManager, TaskStatus, TaskStorage,
};

fn setup_test_manager() -> (Arc<TaskManager>, Arc<TaskStorage>, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(TaskStorage::new(temp_dir.path()));
    let manager = Arc::new(TaskManager::new(storage.clone()));
    (manager, storage, temp_dir)
}

#[test]
fn test_scenario_linear_pipeline_happy_path() {
    let (manager, storage, _dir) = setup_test_manager();

    // 1. 3件のタスクを作成
    let inputs = vec![
        TaskCreateInput {
            id: "t1".to_string(),
            title: "要件確認".to_string(),
            description: "設計ドキュメントの精読".to_string(),
        },
        TaskCreateInput {
            id: "t2".to_string(),
            title: "コア実装".to_string(),
            description: "ドメインロジックの実装".to_string(),
        },
        TaskCreateInput {
            id: "t3".to_string(),
            title: "統合テスト".to_string(),
            description: "シナリオテストの実行".to_string(),
        },
    ];

    let list = manager.create_task_list(inputs).expect("task list creation should succeed");
    assert_eq!(list.tasks.len(), 3);
    assert!(storage.tasks_file_path().exists());

    // 2. get_next_task -> t1 (pending)
    let next1 = manager.get_next_task().unwrap();
    assert_eq!(next1.task.as_ref().unwrap().id, "t1");
    assert_eq!(next1.task.as_ref().unwrap().status, TaskStatus::Pending);
    assert_eq!(next1.completed_tasks, 0);
    assert!(!next1.is_all_completed);

    // 3. t1: in_progress -> completed
    let t1_ip = manager
        .update_task_status("t1", TaskStatus::InProgress, Some("t1 開始".to_string()))
        .unwrap();
    assert_eq!(t1_ip.status, TaskStatus::InProgress);

    let t1_comp = manager
        .update_task_status("t1", TaskStatus::Completed, Some("t1 完了".to_string()))
        .unwrap();
    assert_eq!(t1_comp.status, TaskStatus::Completed);

    // 4. get_next_task -> t2 (pending)
    let next2 = manager.get_next_task().unwrap();
    assert_eq!(next2.task.as_ref().unwrap().id, "t2");
    assert_eq!(next2.completed_tasks, 1);

    // 5. t2: in_progress -> completed
    manager
        .update_task_status("t2", TaskStatus::InProgress, Some("t2 開始".to_string()))
        .unwrap();
    manager
        .update_task_status("t2", TaskStatus::Completed, Some("t2 完了".to_string()))
        .unwrap();

    // 6. get_next_task -> t3 (pending)
    let next3 = manager.get_next_task().unwrap();
    assert_eq!(next3.task.as_ref().unwrap().id, "t3");
    assert_eq!(next3.completed_tasks, 2);

    // 7. t3: in_progress -> completed
    manager
        .update_task_status("t3", TaskStatus::InProgress, Some("t3 開始".to_string()))
        .unwrap();
    manager
        .update_task_status("t3", TaskStatus::Completed, Some("t3 完了".to_string()))
        .unwrap();

    // 8. get_next_task -> is_all_completed: true
    let next_final = manager.get_next_task().unwrap();
    assert!(next_final.is_all_completed);
    assert_eq!(next_final.completed_tasks, 3);
    assert!(next_final.task.is_none());

    // 9. get_task_summary -> 3/3 (100.0%)
    let summary = manager.get_task_summary().unwrap();
    assert_eq!(summary.total_tasks, 3);
    assert_eq!(summary.completed_tasks, 3);
    assert_eq!(summary.progress_percent, 100.0);
    assert!(summary.formatted_summary.contains("3/3 (100.0%)"));

    // 10. ストレージ & ログ追記の確認
    let loaded = storage.load().unwrap().expect("tasks.json must be loadable");
    assert_eq!(loaded.tasks.len(), 3);
    assert!(loaded.tasks.iter().all(|t| t.status == TaskStatus::Completed));

    let log_file = File::open(storage.log_file_path()).unwrap();
    let reader = BufReader::new(log_file);
    let log_count = reader.lines().count();
    // create (1) + t1 (2) + t2 (2) + t3 (2) = 7 イベント
    assert_eq!(log_count, 7);
}

#[test]
fn test_scenario_guardrail_blocks_skip_attempt() {
    let (manager, storage, _dir) = setup_test_manager();

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

    // t1 が pending の状態で t2 を in_progress にしようと試行
    let err_in_progress = manager.update_task_status("t2", TaskStatus::InProgress, None);
    assert!(err_in_progress.is_err());
    let err = err_in_progress.unwrap_err();
    match err.downcast_ref::<GuardrailError>() {
        Some(GuardrailError::PredecessorNotCompleted {
            predecessor_id,
            predecessor_index,
            current_id,
        }) => {
            assert_eq!(predecessor_id, "t1");
            assert_eq!(*predecessor_index, 0);
            assert_eq!(current_id, "t2");
        }
        _ => panic!("Expected PredecessorNotCompleted error, got: {:?}", err),
    }

    // t1 が pending の状態で t2 を completed に直接しようと試行
    let err_completed = manager.update_task_status("t2", TaskStatus::Completed, None);
    assert!(err_completed.is_err());

    // 監査ログに guardrail_violation イベントが記録されているか検証
    let log_file = File::open(storage.log_file_path()).unwrap();
    let reader = BufReader::new(log_file);
    let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();

    // 1行目: create_task_list
    // 2行目: guardrail_violation (in_progress スキップ)
    // 3行目: guardrail_violation (completed スキップ)
    assert_eq!(lines.len(), 3);

    let violation_entry: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
    assert_eq!(violation_entry["event_type"], "guardrail_violation");
    assert_eq!(
        violation_entry["guardrail_violation"]["rule"],
        "PredecessorNotCompleted"
    );
}

#[test]
fn test_scenario_guardrail_enforces_single_active() {
    let (manager, _storage, _dir) = setup_test_manager();

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

    // t1 を in_progress にする
    manager.update_task_status("t1", TaskStatus::InProgress, None).unwrap();

    // t1 が in_progress の状態で、先行完了を偽装せずとも t2 を in_progress にしようとすると
    // SingleActive または Predecessor のいずれかのガードレールで遮断される
    let err = manager.update_task_status("t2", TaskStatus::InProgress, None).unwrap_err();
    let guardrail_err = err.downcast_ref::<GuardrailError>().expect("Must be GuardrailError");

    // MultipleActiveTasks が優先して検出される
    assert!(matches!(
        guardrail_err,
        GuardrailError::MultipleActiveTasks { current_active_id } if current_active_id == "t1"
    ));
}

#[test]
fn test_scenario_circuit_breaker_and_human_recovery() {
    let (manager, storage, _dir) = setup_test_manager();

    let inputs = vec![
        TaskCreateInput {
            id: "t1".to_string(),
            title: "難解タスク".to_string(),
            description: "失敗しやすいタスク".to_string(),
        },
        TaskCreateInput {
            id: "t2".to_string(),
            title: "後続タスク".to_string(),
            description: "t1 完了後に実行するタスク".to_string(),
        },
    ];
    manager.create_task_list(inputs).unwrap();

    // 着手
    manager.update_task_status("t1", TaskStatus::InProgress, None).unwrap();

    // 1回目の失敗
    let f1 = manager
        .update_task_status("t1", TaskStatus::Failed, Some("ビルド失敗 1回目".to_string()))
        .unwrap();
    assert_eq!(f1.status, TaskStatus::Failed);
    assert_eq!(f1.retry_count, 1);

    // 2回目の失敗
    let f2 = manager
        .update_task_status("t1", TaskStatus::Failed, Some("テスト失敗 2回目".to_string()))
        .unwrap();
    assert_eq!(f2.status, TaskStatus::Failed);
    assert_eq!(f2.retry_count, 2);

    // 3回目の失敗 -> 自動的に Blocked (サーキットブレーカー発動)
    let f3 = manager
        .update_task_status("t1", TaskStatus::Failed, Some("再試行失敗 3回目".to_string()))
        .unwrap();
    assert_eq!(f3.status, TaskStatus::Blocked);
    assert_eq!(f3.retry_count, 3);

    // get_next_task で is_blocked: true が報告される
    let next = manager.get_next_task().unwrap();
    assert!(next.is_blocked);

    // ----------------------------------------------------
    // 人間アクターによる介入シミュレーション
    // .agent/tasks.json を人間が直接エディタで開き、t1 を Completed に修正して保存
    // ----------------------------------------------------
    let mut tasks_list: TaskList = storage.load().unwrap().expect("tasks.json must exist");
    {
        let t1 = tasks_list.find_by_id_mut("t1").unwrap();
        t1.status = TaskStatus::Completed;
    }
    // 人間による保存
    storage.save_atomic(&tasks_list).unwrap();

    // 人間の修正後、エージェントが再度 get_next_task を呼ぶと t2 (pending) が取得され復帰！
    let next_after_recovery = manager.get_next_task().unwrap();
    assert!(!next_after_recovery.is_blocked);
    let current_task = next_after_recovery.task.expect("t2 should be next task");
    assert_eq!(current_task.id, "t2");
    assert_eq!(current_task.status, TaskStatus::Pending);
    assert_eq!(next_after_recovery.completed_tasks, 1);

    // t2 を正常に着手・完了できる
    manager.update_task_status("t2", TaskStatus::InProgress, None).unwrap();
    manager.update_task_status("t2", TaskStatus::Completed, None).unwrap();

    let final_summary = manager.get_task_summary().unwrap();
    assert_eq!(final_summary.completed_tasks, 2);
    assert_eq!(final_summary.progress_percent, 100.0);
}
