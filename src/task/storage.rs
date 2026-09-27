//! ストレージ & アトミック操作
//!
//! タスクリストのローカル JSON へのアトミック永続化（Write-Replace）
//! および監査用 JSONL ログの追記（Append-Only）を提供する。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::StorageError;
use crate::task::model::TaskList;

/// 監査用 JSONL ログエントリ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// 記録日時
    pub timestamp: DateTime<Utc>,
    /// イベント種別 ("tool_call", "guardrail_violation", "circuit_breaker_triggered" など)
    pub event_type: String,
    /// 呼び出されたツール名
    pub tool_name: String,
    /// ツール引数
    pub params: serde_json::Value,
    /// 実行結果またはエラー詳細
    pub result: serde_json::Value,
    /// 所要時間（ミリ秒）
    pub duration_ms: f64,
    /// ガードレール違反詳細（違反時のみ）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guardrail_violation: Option<serde_json::Value>,
}

impl LogEntry {
    /// 通常のツール呼び出しログエントリを生成
    pub fn new(
        tool_name: impl Into<String>,
        params: serde_json::Value,
        result: serde_json::Value,
        duration_ms: f64,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            event_type: "tool_call".to_string(),
            tool_name: tool_name.into(),
            params,
            result,
            duration_ms,
            guardrail_violation: None,
        }
    }

    /// ガードレール違反ログエントリを生成
    pub fn new_guardrail_violation(
        tool_name: impl Into<String>,
        params: serde_json::Value,
        result: serde_json::Value,
        duration_ms: f64,
        violation: serde_json::Value,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            event_type: "guardrail_violation".to_string(),
            tool_name: tool_name.into(),
            params,
            result,
            duration_ms,
            guardrail_violation: Some(violation),
        }
    }
}

/// タスク永続化ストレージ
#[derive(Debug, Clone)]
pub struct TaskStorage {
    /// 保存先ディレクトリのパス（例: `.agents/`）
    base_dir: PathBuf,
}

impl TaskStorage {
    /// 新規ストレージインスタンスを作成
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// ベースディレクトリパスを取得
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// tasks.json のパスを取得
    pub fn tasks_file_path(&self) -> PathBuf {
        self.base_dir.join("tasks.json")
    }

    /// tasks.log.jsonl のパスを取得
    pub fn log_file_path(&self) -> PathBuf {
        self.base_dir.join("tasks.log.jsonl")
    }

    /// タスクリストを JSON 形式でアトミックに保存
    ///
    /// 同一ディレクトリ内に一時ファイルを作成し、Pretty JSON を書き込み・flush 後に
    /// リネーム（置換）することで電源断やプロセスキル時でもファイル破損を防止します。
    pub fn save_atomic(&self, list: &TaskList) -> Result<(), StorageError> {
        std::fs::create_dir_all(&self.base_dir)?;

        let mut temp_file = tempfile::NamedTempFile::new_in(&self.base_dir)?;
        let json_bytes = serde_json::to_vec_pretty(list)?;
        temp_file.write_all(&json_bytes)?;
        temp_file.flush()?;

        let target_path = self.tasks_file_path();
        temp_file.persist(&target_path).map_err(|e| e.error)?;

        Ok(())
    }

    /// tasks.json からタスクリストを読み込み
    ///
    /// ファイルが存在しない場合は `Ok(None)` を返却します。
    pub fn load(&self) -> Result<Option<TaskList>, StorageError> {
        let path = self.tasks_file_path();
        if !path.exists() {
            return Ok(None);
        }

        let file = File::open(&path)?;
        let list: TaskList = serde_json::from_reader(file)?;
        Ok(Some(list))
    }

    /// 監査用 JSONL ログに1行追記
    pub fn append_log(&self, entry: &LogEntry) -> Result<(), StorageError> {
        std::fs::create_dir_all(&self.base_dir)?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_file_path())?;

        let json = serde_json::to_string(entry)?;
        writeln!(file, "{json}")?;
        file.flush()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::model::Task;
    use std::io::{BufRead, BufReader};

    #[test]
    fn test_save_atomic_and_load_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = TaskStorage::new(temp_dir.path());

        // 初期状態は存在しないため None
        assert!(storage.load().unwrap().is_none());

        // タスクリスト作成＆保存
        let tasks = vec![
            Task::new("t1", "タスク1", "説明1"),
            Task::new("t2", "タスク2", "説明2"),
        ];
        let list = TaskList::new(tasks).unwrap();
        storage.save_atomic(&list).unwrap();

        // 読み込み検証
        let loaded = storage.load().unwrap().expect("should find tasks.json");
        assert_eq!(loaded.version, "1.0.0");
        assert_eq!(loaded.tasks.len(), 2);
        assert_eq!(loaded.tasks[0].id, "t1");
        assert_eq!(loaded.tasks[1].id, "t2");
    }

    #[test]
    fn test_save_atomic_overwrite() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = TaskStorage::new(temp_dir.path());

        // 初回保存
        let tasks1 = vec![Task::new("t1", "タスク1", "説明1")];
        let list1 = TaskList::new(tasks1).unwrap();
        storage.save_atomic(&list1).unwrap();

        // 上書き保存
        let tasks2 = vec![
            Task::new("t1", "タスク1改", "説明1改"),
            Task::new("t2", "タスク2", "説明2"),
        ];
        let list2 = TaskList::new(tasks2).unwrap();
        storage.save_atomic(&list2).unwrap();

        // 検証
        let loaded = storage.load().unwrap().unwrap();
        assert_eq!(loaded.tasks.len(), 2);
        assert_eq!(loaded.tasks[0].title, "タスク1改");
    }

    #[test]
    fn test_append_log_jsonl() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = TaskStorage::new(temp_dir.path());

        let entry1 = LogEntry::new(
            "create_task_list",
            serde_json::json!({"count": 2}),
            serde_json::json!({"success": true}),
            2.5,
        );
        let entry2 = LogEntry::new_guardrail_violation(
            "update_task_status",
            serde_json::json!({"id": "t2"}),
            serde_json::json!({"success": false}),
            1.2,
            serde_json::json!({"rule": "PredecessorNotCompleted"}),
        );

        storage.append_log(&entry1).unwrap();
        storage.append_log(&entry2).unwrap();

        // ログファイルを読み込み、行ごとにパース検証
        let log_file = File::open(storage.log_file_path()).unwrap();
        let reader = BufReader::new(log_file);
        let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();

        assert_eq!(lines.len(), 2);

        let parsed1: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(parsed1["tool_name"], "create_task_list");
        assert_eq!(parsed1["event_type"], "tool_call");
        assert!(parsed1.get("guardrail_violation").is_none());

        let parsed2: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(parsed2["tool_name"], "update_task_status");
        assert_eq!(parsed2["event_type"], "guardrail_violation");
        assert_eq!(
            parsed2["guardrail_violation"]["rule"],
            "PredecessorNotCompleted"
        );
    }
}
