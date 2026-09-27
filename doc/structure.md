# プロジェクト構造 (structure.md)

## 📁 フォルダ構成

```text
.
├── Cargo.toml              # Rust パッケージ構成・依存クレート定義
├── Cargo.lock              # 依存関係バージョン固定
├── src/                    # ソースコード
│   ├── main.rs             # アプリケーションエントリポイント (CLI引数パース、MCPサーバー起動)
│   ├── lib.rs              # ライブラリ基盤 (モジュール公開)
│   ├── error.rs            # ドメイン固有エラー・ガードレール違反エラー定義
│   ├── mcp/                # MCP プロトコル・ツール層
│   │   ├── mod.rs          # MCP モジュール公開
│   │   ├── server.rs       # stdio JSON-RPC MCP サーバー
│   │   └── tools.rs        # MCP ツールハンドラー (init, get, update, summary)
│   └── task/               # タスク管理・ガードレールコア層
│       ├── mod.rs          # タスクモジュール公開
│       ├── model.rs        # Task, TaskList, TaskStatus 等の型定義
│       ├── guardrail.rs    # シングルアクティブ制約・依存関係バリデーション
│       └── storage.rs      # .agent/tasks.json 読み書き・アトミック永続化
├── tests/                  # 統合テスト・ガードレール検証
│   ├── common/             # テスト用ヘルパー
│   └── guardrail_test.rs   # ガードレール制約・ツール呼び出し検証テスト
├── doc/                    # 🌍 公開用ドキュメント・仕様書マスター (Git管理)
│   ├── adr/                # アーキテクチャ決定レコード
│   ├── product.md          # 製品概要・憲法
│   ├── tech.md             # 技術スタック仕様
│   ├── structure.md        # プロジェクト構造（このファイル）
│   ├── requirements.md     # 要件定義 (EARS記法)
│   └── design.md           # 詳細設計書 (Mermaid/IPO)
└── .gitignore              # Git 除外設定
```

> [!IMPORTANT]
> **仕様書と作業領域の分離ルール**:
> - **公開仕様マスター (`doc/`)**: 要件・設計・憲法・ADRなど、リポジトリ公開に必要な正式仕様書のみを配置します。
> - **非公開作業領域**: 実装タスク計画や開発用作業メモは、`.gitignore` で完全除外した非公開ディレクトリで運用し、この公開用 `doc/structure.md` には掲載しません。

## 🏷️ 命名規則
- **ファイル名**: `snake_case.rs`（Rust 標準規約に準拠）
- **型名・構造体・Enum**: `PascalCase`（例: `Task`, `TaskStatus`, `GuardrailViolation`）
- **関数名・メソッド名・変数名**: `snake_case`（例: `get_next_task`, `validate_dependencies`）
- **定数名**: `SCREAMING_SNAKE_CASE`（例: `DEFAULT_STORAGE_PATH`）
- **MCP ツール名**: `snake_case`（例: `create_task_list`, `get_next_task`, `update_task_status`, `get_task_summary`）

## 🏗️ アーキテクチャの方針
- **レイヤード ＆ ドメイン分離**:
  - **Transport / Protocol 層 (`src/mcp`)**: MCP プロトコル（JSON-RPC 2.0 / stdio）の入出力、リクエストのディスパッチ、レスポンスの整形に専念する。
  - **Domain / Guardrail 層 (`src/task`)**: タスクのライフサイクル、状態遷移、先行依存関係の検証、シングルアクティブ制約の強制などのビジネスルールを完結させる。プロトコル層への依存を持たない。
  - **Storage 層 (`src/task/storage.rs`)**: ローカルファイル（`.agent/tasks.json`）の読み込み・書き込み、アトミック置換（Write-Replace）によるデータ整合性担保を担う。
- **Fail-Fast なガードレール検証**:
  - 制約違反（2つ目のタスクを勝手に着手しようとしたり、未完了依存タスクを飛ばそうとする等）が発生した場合は、速やかに明確なエラーメッセージを返し、エージェントを正しい軌道へ誘導する。

## 🛠️ インポートパターン
- プロジェクト内モジュール参照は `crate::` を起点とする絶対パスインポートを基本とし、可読性とリファクタリング耐性を高める。
- 標準ライブラリ (`std::*`)、外部クレート (`serde::*`, `tokio::*`)、内部モジュール (`crate::*`) の順でグループ化して記述する。

## 🔗 その他設計の決定事項
- **stdout / stderr の厳格な分離**:
  - stdio ベースの MCP サーバーにおいて、標準出力（stdout）への不正な文字列出力は JSON-RPC パースエラーを引き起こす致命傷となるため、ロギングは必ず `tracing` を通じて標準エラー出力（stderr）に出力する。
- **安全なアトミックファイル永続化**:
  - タスク状態更新時は直接上書きせず、一時ファイルに書き出した上でアトミックにリネーム（置換）することで、プロセス異常終了時のファイル破損リスクをゼロにする。

## 🛡️ Git除外方針 (.gitignore)
プロジェクトの健全性とセキュリティのため、以下のカテゴリを `.gitignore` で確実に除外します。
- **AIエージェント設定・プライベート設定**: `AGENTS.md`, `GEMINI.md`, `/.agent/`, `/.gemini/`, `/.claude/`
- **非公開開発作業領域**: 非公開作業用ディレクトリ
- **秘密情報・環境変数**: `.env`, `credentials.json`, `token.json`
- **ビルド・パッケージ成果物**: `target/`, 実行バイナリ
- **テスト・カバレッジ出力**: `coverage/`, `*.out`, `cov*`
